//! Pure-Rust NNUE trainer: reads dataset shards written by `bin/gen.rs`, trains a
//! `FloatNet` with Adam against a blended sigmoid-CE loss (search-score label + game
//! result), then quantizes and writes a `.bin` net file `bin/match.rs` /
//! `wasm_api.rs` can load.
use clap::Parser;
use rayon::prelude::*;
use shogi_wasm::dataset::{Record, RECORD_SIZE};
use shogi_wasm::eval::features::active_features;
use shogi_wasm::eval::network::{FloatNet, L0, L1};
use shogi_wasm::types::{BLACK, WHITE};
use std::fs;
use std::io::Read;
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    #[arg(short, long)]
    data_dir: String,
    #[arg(short, long, default_value = "nets/current.bin")]
    out: String,
    #[arg(long, default_value_t = 15)]
    epochs: u32,
    #[arg(long, default_value_t = 4096)]
    batch_size: usize,
    #[arg(short, long, default_value_t = 10)]
    threads: usize,
    #[arg(long, default_value_t = 1e-3)]
    lr: f32,
    #[arg(long, default_value_t = 1.0)]
    lambda: f32,
    #[arg(long, default_value_t = 600.0)]
    k: f32,
    #[arg(long, default_value_t = 20260921)]
    seed: u64,
    /// Warm-start from an existing net's float checkpoint (not the quantized .bin —
    /// pass the .fnet sidecar this tool also writes alongside the quantized net).
    #[arg(long)]
    warm_start: Option<String>,
}

fn load_records(dir: &str) -> Vec<Record> {
    let mut records = Vec::new();
    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot read {dir}: {e}")) {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("bin") {
            continue;
        }
        let mut buf = Vec::new();
        fs::File::open(&path).unwrap().read_to_end(&mut buf).unwrap();
        let n = buf.len() / RECORD_SIZE;
        for i in 0..n {
            records.push(Record::from_bytes(&buf[i * RECORD_SIZE..(i + 1) * RECORD_SIZE]));
        }
        eprintln!("loaded {n} records from {}", path.display());
    }
    records
}

struct Grad {
    ft_weight: Vec<f32>,
    ft_bias: [f32; L0],
    l1_weight: [[f32; 2 * L0]; L1],
    l1_bias: [f32; L1],
    l2_weight: [f32; L1],
    l2_bias: f32,
}
impl Grad {
    fn zeroed() -> Grad {
        Grad { ft_weight: vec![0.0; shogi_wasm::eval::features::N_FEATURES * L0], ft_bias: [0.0; L0], l1_weight: [[0.0; 2 * L0]; L1], l1_bias: [0.0; L1], l2_weight: [0.0; L1], l2_bias: 0.0 }
    }
    fn add_assign(&mut self, other: &Grad) {
        for i in 0..self.ft_weight.len() {
            self.ft_weight[i] += other.ft_weight[i];
        }
        for j in 0..L0 {
            self.ft_bias[j] += other.ft_bias[j];
        }
        for k in 0..L1 {
            for i in 0..2 * L0 {
                self.l1_weight[k][i] += other.l1_weight[k][i];
            }
            self.l1_bias[k] += other.l1_bias[k];
            self.l2_weight[k] += other.l2_weight[k];
        }
        self.l2_bias += other.l2_bias;
    }
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// Forward + backward for one sample, accumulating into `grad`. Returns the CE loss
/// (for reporting only). `active_features` is the SAME function `nnue.rs` uses at
/// inference time — the whole point of keeping it in one place.
fn train_one(net: &FloatNet, grad: &mut Grad, rec: &Record, lambda: f32, k: f32) -> f32 {
    let pos = rec.to_position();
    let mut feats = [Vec::new(), Vec::new()];
    active_features(&pos, BLACK, &mut feats[0]);
    active_features(&pos, WHITE, &mut feats[1]);

    let mut acc = [[0f32; L0]; 2]; // indexed by ABSOLUTE color
    for c in 0..2 {
        acc[c] = net.ft_bias;
        for &f in &feats[c] {
            let row = &net.ft_weight[f as usize * L0..(f as usize + 1) * L0];
            for j in 0..L0 {
                acc[c][j] += row[j];
            }
        }
    }
    let stm = pos.side_to_move();
    let order = if stm == BLACK { [0usize, 1usize] } else { [1, 0] };
    let acc_ord = [acc[order[0]], acc[order[1]]];

    let mut x0 = [0f32; 2 * L0];
    let mut x0_gate = [0f32; 2 * L0];
    for p in 0..2 {
        for j in 0..L0 {
            let v = acc_ord[p][j];
            x0[p * L0 + j] = v.clamp(0.0, 1.0);
            x0_gate[p * L0 + j] = if v > 0.0 && v < 1.0 { 1.0 } else { 0.0 };
        }
    }
    let mut y1 = [0f32; L1];
    let mut x1 = [0f32; L1];
    let mut x1_gate = [0f32; L1];
    for k_ in 0..L1 {
        let mut y = net.l1_bias[k_];
        for i in 0..2 * L0 {
            y += x0[i] * net.l1_weight[k_][i];
        }
        y1[k_] = y;
        x1[k_] = y.clamp(0.0, 1.0);
        x1_gate[k_] = if y > 0.0 && y < 1.0 { 1.0 } else { 0.0 };
    }
    let mut y2 = net.l2_bias;
    for i in 0..L1 {
        y2 += x1[i] * net.l2_weight[i];
    }

    let k_pawn = k / 100.0;
    let q = sigmoid(y2 / k_pawn);
    let label_q = sigmoid(rec.score as f32 / k);
    let result_q = (rec.result as f32 + 1.0) / 2.0;
    let t = lambda * label_q + (1.0 - lambda) * result_q;
    let eps = 1e-6;
    let loss = -(t * (q + eps).ln() + (1.0 - t) * (1.0 - q + eps).ln());

    let g2 = (q - t) / k_pawn; // dL/dy2
    grad.l2_bias += g2;
    for i in 0..L1 {
        grad.l2_weight[i] += g2 * x1[i];
    }
    let mut dy1 = [0f32; L1];
    for i in 0..L1 {
        dy1[i] = g2 * net.l2_weight[i] * x1_gate[i];
    }
    for k_ in 0..L1 {
        grad.l1_bias[k_] += dy1[k_];
        for i in 0..2 * L0 {
            grad.l1_weight[k_][i] += dy1[k_] * x0[i];
        }
    }
    let mut dx0 = [0f32; 2 * L0];
    for i in 0..2 * L0 {
        let mut s = 0.0;
        for k_ in 0..L1 {
            s += dy1[k_] * net.l1_weight[k_][i];
        }
        dx0[i] = s;
    }
    let mut dacc_ord = [[0f32; L0]; 2];
    for p in 0..2 {
        for j in 0..L0 {
            dacc_ord[p][j] = dx0[p * L0 + j] * x0_gate[p * L0 + j];
        }
    }
    for j in 0..L0 {
        grad.ft_bias[j] += dacc_ord[0][j] + dacc_ord[1][j];
    }
    let mut dacc = [[0f32; L0]; 2]; // back to absolute color indexing
    dacc[order[0]] = dacc_ord[0];
    dacc[order[1]] = dacc_ord[1];
    for c in 0..2 {
        for &f in &feats[c] {
            let base = f as usize * L0;
            for j in 0..L0 {
                grad.ft_weight[base + j] += dacc[c][j];
            }
        }
    }

    loss
}

struct Adam {
    m: Grad,
    v: Grad,
    t: u64,
}
impl Adam {
    fn new() -> Adam {
        Adam { m: Grad::zeroed(), v: Grad::zeroed(), t: 0 }
    }
    fn step(&mut self, net: &mut FloatNet, grad: &Grad, lr: f32) {
        self.t += 1;
        let b1 = 0.9f32;
        let b2 = 0.999f32;
        let eps = 1e-8f32;
        let bc1 = 1.0 - b1.powi(self.t as i32);
        let bc2 = 1.0 - b2.powi(self.t as i32);
        let upd = |m: &mut f32, v: &mut f32, w: &mut f32, g: f32| {
            *m = b1 * *m + (1.0 - b1) * g;
            *v = b2 * *v + (1.0 - b2) * g * g;
            let mhat = *m / bc1;
            let vhat = *v / bc2;
            *w -= lr * mhat / (vhat.sqrt() + eps);
        };
        for i in 0..net.ft_weight.len() {
            upd(&mut self.m.ft_weight[i], &mut self.v.ft_weight[i], &mut net.ft_weight[i], grad.ft_weight[i]);
        }
        for j in 0..L0 {
            upd(&mut self.m.ft_bias[j], &mut self.v.ft_bias[j], &mut net.ft_bias[j], grad.ft_bias[j]);
        }
        for k in 0..L1 {
            for i in 0..2 * L0 {
                upd(&mut self.m.l1_weight[k][i], &mut self.v.l1_weight[k][i], &mut net.l1_weight[k][i], grad.l1_weight[k][i]);
            }
            upd(&mut self.m.l1_bias[k], &mut self.v.l1_bias[k], &mut net.l1_bias[k], grad.l1_bias[k]);
            upd(&mut self.m.l2_weight[k], &mut self.v.l2_weight[k], &mut net.l2_weight[k], grad.l2_weight[k]);
        }
        upd(&mut self.m.l2_bias, &mut self.v.l2_bias, &mut net.l2_bias, grad.l2_bias);
    }
}

fn main() {
    let args = Args::parse();
    rayon::ThreadPoolBuilder::new().num_threads(args.threads).build_global().expect("build rayon thread pool");
    let mut records = load_records(&args.data_dir);
    eprintln!("total records: {}", records.len());
    assert!(!records.is_empty(), "no training data found in {}", args.data_dir);

    // Shuffle (xorshift, deterministic from --seed). NOTE: this is a position-level
    // shuffle/split, not a game-level one — a real train/val split should hold out
    // whole games to avoid leakage between near-duplicate consecutive positions; cut
    // for time here, so treat the reported validation loss as optimistic.
    let mut rng = args.seed.max(1);
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };
    for i in (1..records.len()).rev() {
        let j = (next() as usize) % (i + 1);
        records.swap(i, j);
    }
    let val_n = (records.len() / 20).max(1).min(records.len() - 1);
    let (val_set, train_set) = records.split_at(val_n);
    eprintln!("train: {} val: {}", train_set.len(), val_set.len());

    let mut net = match &args.warm_start {
        Some(path) => load_float_net(path),
        None => FloatNet::randomized(args.seed),
    };
    net.clamp_weights();
    let mut adam = Adam::new();

    let n_threads = rayon::current_num_threads().max(1);
    let mut order: Vec<usize> = (0..train_set.len()).collect();
    let mut shuffle_rng = args.seed ^ 0xD1B54A32D192ED03;
    let start = std::time::Instant::now();

    // Keep the best-by-validation-loss checkpoint, not just whatever the last epoch
    // happens to be: with ~300K parameters and a self-play dataset that's small
    // relative to that, late epochs can overfit even with a low-variance label.
    let mut best_val_loss = f64::INFINITY;
    let mut best_net = net.clone();

    for epoch in 0..args.epochs {
        // Fisher-Yates on the index list (records stay put; avoids re-copying 100B * N).
        for i in (1..order.len()).rev() {
            shuffle_rng ^= shuffle_rng << 13;
            shuffle_rng ^= shuffle_rng >> 7;
            shuffle_rng ^= shuffle_rng << 17;
            let j = (shuffle_rng as usize) % (i + 1);
            order.swap(i, j);
        }

        let mut epoch_loss = 0.0f64;
        let mut n_batches = 0u64;
        for batch_start in (0..order.len()).step_by(args.batch_size) {
            let batch_end = (batch_start + args.batch_size).min(order.len());
            let batch_idx = &order[batch_start..batch_end];

            let chunk = batch_idx.len().div_ceil(n_threads).max(1);
            let partials: Vec<(Grad, f32)> = batch_idx
                .par_chunks(chunk)
                .map(|idx_chunk| {
                    let mut g = Grad::zeroed();
                    let mut loss_sum = 0.0f32;
                    for &idx in idx_chunk {
                        loss_sum += train_one(&net, &mut g, &train_set[idx], args.lambda, args.k);
                    }
                    (g, loss_sum)
                })
                .collect();

            let mut total = Grad::zeroed();
            let mut batch_loss = 0.0f32;
            for (g, l) in &partials {
                total.add_assign(g);
                batch_loss += l;
            }
            let n = batch_idx.len() as f32;
            // Average the gradient over the batch before the Adam step.
            for w in total.ft_weight.iter_mut() {
                *w /= n;
            }
            for w in total.ft_bias.iter_mut() {
                *w /= n;
            }
            for k in 0..L1 {
                for w in total.l1_weight[k].iter_mut() {
                    *w /= n;
                }
                total.l1_bias[k] /= n;
                total.l2_weight[k] /= n;
            }
            total.l2_bias /= n;

            adam.step(&mut net, &total, args.lr);
            net.clamp_weights();

            epoch_loss += (batch_loss / n) as f64;
            n_batches += 1;
        }

        let val_loss = evaluate_loss(&net, val_set, args.lambda, args.k);
        let marker = if val_loss < best_val_loss {
            best_val_loss = val_loss;
            best_net = net.clone();
            " *best*"
        } else {
            ""
        };
        eprintln!(
            "epoch {epoch}: train_loss={:.4} val_loss={:.4} elapsed={:.0}s{marker}",
            epoch_loss / n_batches.max(1) as f64,
            val_loss,
            start.elapsed().as_secs_f64()
        );
    }
    let net = best_net;
    eprintln!("using best checkpoint: val_loss={best_val_loss:.4}");

    fs::create_dir_all(PathBuf::from(&args.out).parent().unwrap_or_else(|| std::path::Path::new("."))).ok();
    let fnet_path = format!("{}.fnet", args.out);
    save_float_net(&net, &fnet_path);
    let q = net.quantize();
    fs::write(&args.out, q.to_bytes()).unwrap_or_else(|e| panic!("write {}: {e}", args.out));
    eprintln!("wrote {} ({} bytes) and float checkpoint {}", args.out, q.to_bytes().len(), fnet_path);
}

fn evaluate_loss(net: &FloatNet, set: &[Record], lambda: f32, k: f32) -> f64 {
    let mut g = Grad::zeroed(); // discarded; train_one always accumulates, we just read the loss
    let mut total = 0.0f64;
    for rec in set {
        total += train_one(net, &mut g, rec, lambda, k) as f64;
    }
    total / set.len().max(1) as f64
}

fn save_float_net(net: &FloatNet, path: &str) {
    let mut out = Vec::new();
    for &w in &net.ft_weight {
        out.extend_from_slice(&w.to_le_bytes());
    }
    for &b in &net.ft_bias {
        out.extend_from_slice(&b.to_le_bytes());
    }
    for row in &net.l1_weight {
        for &w in row {
            out.extend_from_slice(&w.to_le_bytes());
        }
    }
    for &b in &net.l1_bias {
        out.extend_from_slice(&b.to_le_bytes());
    }
    for &w in &net.l2_weight {
        out.extend_from_slice(&w.to_le_bytes());
    }
    out.extend_from_slice(&net.l2_bias.to_le_bytes());
    fs::write(path, out).unwrap_or_else(|e| panic!("write {path}: {e}"));
}

fn load_float_net(path: &str) -> FloatNet {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let mut net = FloatNet::zeroed();
    let mut off = 0usize;
    let rd = |b: &[u8], o: usize| f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    for w in net.ft_weight.iter_mut() {
        *w = rd(&bytes, off);
        off += 4;
    }
    for b in net.ft_bias.iter_mut() {
        *b = rd(&bytes, off);
        off += 4;
    }
    for row in net.l1_weight.iter_mut() {
        for w in row.iter_mut() {
            *w = rd(&bytes, off);
            off += 4;
        }
    }
    for b in net.l1_bias.iter_mut() {
        *b = rd(&bytes, off);
        off += 4;
    }
    for w in net.l2_weight.iter_mut() {
        *w = rd(&bytes, off);
        off += 4;
    }
    net.l2_bias = rd(&bytes, off);
    net
}

#[cfg(test)]
mod tests {
    use super::*;
    use shogi_wasm::position::Position;

    fn sample_record() -> Record {
        let pos = Position::startpos();
        Record::from_position(&pos, 137, 1)
    }

    /// Numerically verifies the hand-written backward pass against finite differences
    /// on a handful of parameters from every layer. This is the single test that
    /// would catch a sign error or wrong-index bug in `train_one`'s backward pass —
    /// exactly the kind of bug that trains "successfully" (loss goes down) while
    /// converging to something subtly wrong.
    #[test]
    fn gradient_matches_finite_difference() {
        let net = FloatNet::randomized(42);
        let rec = sample_record();
        let lambda = 0.8;
        let k = 600.0;

        let mut analytic = Grad::zeroed();
        train_one(&net, &mut analytic, &rec, lambda, k);

        let eps = 1e-3f32;
        let loss_at = |net: &FloatNet| {
            let mut g = Grad::zeroed();
            train_one(net, &mut g, &rec, lambda, k) as f64
        };

        let check = |name: &str, get: &dyn Fn(&FloatNet) -> f32, set: &dyn Fn(&mut FloatNet, f32), analytic_grad: f32| {
            let mut net_plus = net.clone();
            let mut net_minus = net.clone();
            let base = get(&net);
            set(&mut net_plus, base + eps);
            set(&mut net_minus, base - eps);
            let numeric = (loss_at(&net_plus) - loss_at(&net_minus)) / (2.0 * eps as f64);
            let rel_err = ((numeric - analytic_grad as f64).abs()) / (numeric.abs().max(analytic_grad.abs() as f64).max(1e-6));
            assert!(rel_err < 0.02, "{name}: analytic={analytic_grad} numeric={numeric} rel_err={rel_err}");
        };

        // A handful of active FT weights (feature 0's first two components — for the
        // startpos, index 0 corresponds to a real active feature) plus a few
        // structural params from every other layer.
        check("ft_weight[0]", &|n| n.ft_weight[0], &|n, v| n.ft_weight[0] = v, analytic.ft_weight[0]);
        check("ft_weight[500]", &|n| n.ft_weight[500], &|n, v| n.ft_weight[500] = v, analytic.ft_weight[500]);
        check("ft_bias[0]", &|n| n.ft_bias[0], &|n, v| n.ft_bias[0] = v, analytic.ft_bias[0]);
        check("l1_weight[0][0]", &|n| n.l1_weight[0][0], &|n, v| n.l1_weight[0][0] = v, analytic.l1_weight[0][0]);
        check("l1_bias[2]", &|n| n.l1_bias[2], &|n, v| n.l1_bias[2] = v, analytic.l1_bias[2]);
        check("l2_weight[3]", &|n| n.l2_weight[3], &|n, v| n.l2_weight[3] = v, analytic.l2_weight[3]);
        check("l2_bias", &|n| n.l2_bias, &|n, v| n.l2_bias = v, analytic.l2_bias);
    }
}
