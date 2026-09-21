//! Self-play training-data generation. Each of `--threads` OS threads plays games
//! independently (its own Position/TT/RNG) and writes directly to its own shard file
//! — no cross-thread synchronization beyond the initial spawn, which is what makes
//! throughput scale close to linearly with thread count.
use clap::Parser;
use shogi_wasm::dataset::Record;
use shogi_wasm::eval::{Evaluator, HandCrafted};
use shogi_wasm::movegen::generate_legal;
use shogi_wasm::native_support::StdClock;
use shogi_wasm::position::Position;
use shogi_wasm::search::{search, Limits};
use shogi_wasm::tt::TranspositionTable;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    #[arg(short, long, default_value_t = 8)]
    threads: usize,
    #[arg(short, long, default_value_t = 1000)]
    games_per_thread: u64,
    #[arg(short, long, default_value_t = 3000)]
    nodes_per_move: u64,
    #[arg(long, default_value_t = 256)]
    max_plies: u32,
    #[arg(short, long, default_value = "data/it0")]
    out_dir: String,
    #[arg(long, default_value_t = 20260921)]
    seed: u64,
    #[arg(long, default_value_t = 8)]
    random_opening_plies: u32,
}

struct Rng(u64);
impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() as usize) % n.max(1)
    }
}

/// Plays one game, returns the (position-before-move, score-cp-from-side-to-move,
/// ply, was_capture_or_promo) samples plus the final result for the side that moved
/// first (BLACK), so the caller can derive each record's own-POV result.
fn play_game(args: &Args, rng: &mut Rng, tt: &mut TranspositionTable) -> (Vec<(Position, i16, u32)>, i8) {
    let mut pos = Position::startpos();
    let mut eval = HandCrafted::new();
    let mut samples: Vec<(Position, i16, u32)> = Vec::new();
    tt.clear();

    for ply in 0..args.max_plies {
        let mut moves = Vec::with_capacity(96);
        generate_legal(&mut pos, &mut moves);
        if moves.is_empty() {
            // Checkmate or stalemate-equivalent: side to move loses.
            let result_for_black = if pos.side_to_move() == shogi_wasm::types::BLACK { -1 } else { 1 };
            return (samples, result_for_black);
        }
        match pos.detect_repetition() {
            shogi_wasm::position::Repetition::Draw => return (samples, 0),
            shogi_wasm::position::Repetition::WinByPerpetual => {
                let r = if pos.side_to_move() == shogi_wasm::types::BLACK { 1 } else { -1 };
                return (samples, r);
            }
            shogi_wasm::position::Repetition::LoseByPerpetual => {
                let r = if pos.side_to_move() == shogi_wasm::types::BLACK { -1 } else { 1 };
                return (samples, r);
            }
            shogi_wasm::position::Repetition::None => {}
        }

        if ply < args.random_opening_plies {
            let mv = moves[rng.below(moves.len())];
            pos.do_move(mv);
            continue;
        }

        eval.refresh(&pos);
        let clock = StdClock::start();
        let result = search(&mut pos, &mut eval, tt, Limits::nodes(args.nodes_per_move), &clock, None);
        if result.best_move.is_none() {
            break;
        }

        let in_check = pos.in_check();
        let quiet = !result.best_move.is_capture() && !result.best_move.is_promotion();
        let score_ok = result.score_cp.unsigned_abs() <= 3000;
        if !in_check && quiet && score_ok && ply >= args.random_opening_plies {
            samples.push((pos.clone(), result.score_cp as i16, ply));
        }

        pos.do_move(result.best_move);
    }
    (samples, 0) // hit max_plies: treat as a draw
}

fn write_shard(path: &PathBuf, thread_id: usize, args: &Args) {
    let mut rng = Rng(args.seed ^ ((thread_id as u64) << 32) ^ 0x9E3779B97F4A7C15);
    let mut tt = TranspositionTable::new(8);
    let file = File::create(path).unwrap_or_else(|e| panic!("cannot create {}: {e}", path.display()));
    let mut w = BufWriter::new(file);
    let mut games_done = 0u64;
    let mut positions_written = 0u64;

    for _ in 0..args.games_per_thread {
        let (samples, result_for_black) = play_game(args, &mut rng, &mut tt);
        for (sample_pos, score, _ply) in samples {
            let side = sample_pos.side_to_move();
            let result_for_side = if side == shogi_wasm::types::BLACK { result_for_black } else { -result_for_black };
            let rec = Record::from_position(&sample_pos, score, result_for_side);
            w.write_all(&rec.to_bytes()).expect("write");
            positions_written += 1;
        }
        games_done += 1;
        w.flush().expect("flush");
        if games_done % 50 == 0 {
            eprintln!("[thread {thread_id}] {games_done}/{} games, {positions_written} positions", args.games_per_thread);
        }
    }
    w.flush().expect("flush");
    eprintln!("[thread {thread_id}] done: {games_done} games, {positions_written} positions -> {}", path.display());
}

fn main() {
    let args = Args::parse();
    std::fs::create_dir_all(&args.out_dir).expect("create out_dir");
    let start = std::time::Instant::now();

    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for t in 0..args.threads {
            let args = &args;
            let path = PathBuf::from(&args.out_dir).join(format!("shard_{t:02}.bin"));
            handles.push(scope.spawn(move || write_shard(&path, t, args)));
        }
        for h in handles {
            h.join().unwrap();
        }
    });

    println!("generation finished in {:.1}s", start.elapsed().as_secs_f64());
}
