//! The quantized NNUE network: architecture, on-disk format, and the forward pass.
//! `FloatNet` (the f32 twin used only during training) lives here too so the
//! architecture is defined in exactly one place.
use super::features::N_FEATURES;

pub const L0: usize = 128; // per-perspective feature-transformer width
pub const L1: usize = 16; // hidden layer width after concatenating both perspectives

pub const FT_SCALE: f32 = 127.0;
pub const WEIGHT_SCALE: f32 = 64.0;
pub const OUT_SCALE: f32 = FT_SCALE * WEIGHT_SCALE; // 8128

const MAGIC: [u8; 4] = *b"SNN1";
const VERSION: u16 = 1;
/// Hashes the architecture constants so a mismatched net fails loudly at load time
/// instead of producing silently-wrong evaluations.
fn arch_hash() -> u32 {
    let mut h: u32 = 2166136261;
    for &b in format!("N={N_FEATURES},L0={L0},L1={L1}").as_bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// Quantized network: i16 feature transformer, i8 hidden/output layers.
#[derive(Clone)]
pub struct Network {
    pub ft_weight: Vec<i16>, // N_FEATURES * L0, feature-major (row f = that feature's L0 weights)
    pub ft_bias: [i16; L0],
    pub l1_weight: [[i8; 2 * L0]; L1],
    pub l1_bias: [i32; L1],
    pub l2_weight: [i8; L1],
    pub l2_bias: i32,
}

impl Network {
    pub fn zeroed() -> Network {
        Network { ft_weight: vec![0; N_FEATURES * L0], ft_bias: [0; L0], l1_weight: [[0; 2 * L0]; L1], l1_bias: [0; L1], l2_weight: [0; L1], l2_bias: 0 }
    }

    #[inline]
    pub fn ft_row(&self, feature: usize) -> &[i16] {
        &self.ft_weight[feature * L0..(feature + 1) * L0]
    }

    /// Forward pass from a completed pair of feature-transformer accumulators
    /// (`acc[0]` = side-to-move's perspective, `acc[1]` = the other side's) to a
    /// centipawn score from the side-to-move's point of view.
    pub fn forward(&self, acc: &[[i16; L0]; 2]) -> i32 {
        let mut x0 = [0i8; 2 * L0];
        for p in 0..2 {
            for j in 0..L0 {
                x0[p * L0 + j] = acc[p][j].clamp(0, 127) as i8;
            }
        }
        let mut x1 = [0i8; L1];
        for k in 0..L1 {
            let mut y = self.l1_bias[k];
            for i in 0..2 * L0 {
                y += x0[i] as i32 * self.l1_weight[k][i] as i32;
            }
            x1[k] = (y >> 6).clamp(0, 127) as i8;
        }
        let mut y2 = self.l2_bias;
        for i in 0..L1 {
            y2 += x1[i] as i32 * self.l2_weight[i] as i32;
        }
        (y2 as i64 * 100 / OUT_SCALE as i64) as i32
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(64 + N_FEATURES * L0 * 2);
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // flags, reserved
        out.extend_from_slice(&arch_hash().to_le_bytes());
        out.extend_from_slice(&(N_FEATURES as u32).to_le_bytes());
        out.extend_from_slice(&(L0 as u16).to_le_bytes());
        out.extend_from_slice(&(L1 as u16).to_le_bytes());
        while out.len() < 32 {
            out.push(0);
        }
        debug_assert_eq!(out.len(), 32);

        for &w in &self.ft_weight {
            out.extend_from_slice(&w.to_le_bytes());
        }
        for &b in &self.ft_bias {
            out.extend_from_slice(&b.to_le_bytes());
        }
        for row in &self.l1_weight {
            out.extend(row.iter().map(|&x| x as u8));
        }
        for &b in &self.l1_bias {
            out.extend_from_slice(&b.to_le_bytes());
        }
        out.extend(self.l2_weight.iter().map(|&x| x as u8));
        out.extend_from_slice(&self.l2_bias.to_le_bytes());
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Network, String> {
        if bytes.len() < 32 {
            return Err("net file too short for header".into());
        }
        if bytes[0..4] != MAGIC {
            return Err("bad magic (not a SNN1 net file)".into());
        }
        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version != VERSION {
            return Err(format!("unsupported net version {version}"));
        }
        let hash = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        if hash != arch_hash() {
            return Err("architecture hash mismatch: this net was built for a different feature/layer layout".into());
        }
        let n_features = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
        let l0 = u16::from_le_bytes([bytes[16], bytes[17]]) as usize;
        let l1 = u16::from_le_bytes([bytes[18], bytes[19]]) as usize;
        if n_features != N_FEATURES || l0 != L0 || l1 != L1 {
            return Err(format!("dimension mismatch: file has N={n_features} L0={l0} L1={l1}, code expects N={N_FEATURES} L0={L0} L1={L1}"));
        }

        let mut off = 32usize;
        let mut net = Network::zeroed();
        let read_i16 = |b: &[u8], o: usize| i16::from_le_bytes([b[o], b[o + 1]]);
        let read_i32 = |b: &[u8], o: usize| i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);

        for f in 0..N_FEATURES {
            for j in 0..L0 {
                net.ft_weight[f * L0 + j] = read_i16(bytes, off);
                off += 2;
            }
        }
        for j in 0..L0 {
            net.ft_bias[j] = read_i16(bytes, off);
            off += 2;
        }
        for k in 0..L1 {
            for i in 0..2 * L0 {
                net.l1_weight[k][i] = bytes[off] as i8;
                off += 1;
            }
        }
        for k in 0..L1 {
            net.l1_bias[k] = read_i32(bytes, off);
            off += 4;
        }
        for i in 0..L1 {
            net.l2_weight[i] = bytes[off] as i8;
            off += 1;
        }
        net.l2_bias = read_i32(bytes, off);
        Ok(net)
    }
}

/// The float twin used only during training; `quantize()` produces the shipped
/// `Network`. Kept in this module (not train.rs) so the architecture is defined once.
#[derive(Clone)]
pub struct FloatNet {
    pub ft_weight: Vec<f32>, // N_FEATURES * L0
    pub ft_bias: [f32; L0],
    pub l1_weight: [[f32; 2 * L0]; L1],
    pub l1_bias: [f32; L1],
    pub l2_weight: [f32; L1],
    pub l2_bias: f32,
}

/// Weight clamp bounds enforced after every optimizer step (quantization-aware
/// training): with these, the accumulator provably cannot overflow i16 — at most 40
/// active features per perspective (see `features.rs`), so
/// `40 * (2.0*127) + 4.0*127 <= 40*254+508 = 10,668 << 32,767`.
pub const FT_WEIGHT_CLAMP: f32 = 2.0;
pub const FT_BIAS_CLAMP: f32 = 4.0;
pub const HIDDEN_WEIGHT_CLAMP: f32 = 127.0 / WEIGHT_SCALE;

impl FloatNet {
    pub fn zeroed() -> FloatNet {
        FloatNet { ft_weight: vec![0.0; N_FEATURES * L0], ft_bias: [0.0; L0], l1_weight: [[0.0; 2 * L0]; L1], l1_bias: [0.0; L1], l2_weight: [0.0; L1], l2_bias: 0.0 }
    }

    /// Small random init: FT weights ~U(-0.1,0.1), hidden weights scaled by fan-in.
    pub fn randomized(seed: u64) -> FloatNet {
        let mut net = FloatNet::zeroed();
        let mut rng = seed.max(1);
        let mut next = move || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            (rng as f64 / u64::MAX as f64) as f32 * 2.0 - 1.0
        };
        for w in net.ft_weight.iter_mut() {
            *w = next() * 0.05;
        }
        for row in net.l1_weight.iter_mut() {
            for w in row.iter_mut() {
                *w = next() * (1.0 / (2.0 * L0 as f32).sqrt());
            }
        }
        for w in net.l2_weight.iter_mut() {
            *w = next() * (1.0 / (L1 as f32).sqrt());
        }
        net
    }

    pub fn clamp_weights(&mut self) {
        for w in self.ft_weight.iter_mut() {
            *w = w.clamp(-FT_WEIGHT_CLAMP, FT_WEIGHT_CLAMP);
        }
        for b in self.ft_bias.iter_mut() {
            *b = b.clamp(-FT_BIAS_CLAMP, FT_BIAS_CLAMP);
        }
        for row in self.l1_weight.iter_mut() {
            for w in row.iter_mut() {
                *w = w.clamp(-HIDDEN_WEIGHT_CLAMP, HIDDEN_WEIGHT_CLAMP);
            }
        }
        for w in self.l2_weight.iter_mut() {
            *w = w.clamp(-HIDDEN_WEIGHT_CLAMP, HIDDEN_WEIGHT_CLAMP);
        }
    }

    pub fn quantize(&self) -> Network {
        let mut q = Network::zeroed();
        for (i, &w) in self.ft_weight.iter().enumerate() {
            q.ft_weight[i] = (w * FT_SCALE).round() as i16;
        }
        for j in 0..L0 {
            q.ft_bias[j] = (self.ft_bias[j] * FT_SCALE).round() as i16;
        }
        for k in 0..L1 {
            for i in 0..2 * L0 {
                q.l1_weight[k][i] = (self.l1_weight[k][i] * WEIGHT_SCALE).round().clamp(-127.0, 127.0) as i8;
            }
            q.l1_bias[k] = (self.l1_bias[k] * OUT_SCALE).round() as i32;
        }
        for i in 0..L1 {
            q.l2_weight[i] = (self.l2_weight[i] * WEIGHT_SCALE).round().clamp(-127.0, 127.0) as i8;
        }
        q.l2_bias = (self.l2_bias * OUT_SCALE).round() as i32;
        q
    }

    /// Forward pass in float, mirroring `Network::forward` exactly (same clipped-ReLU
    /// ranges in normalized [0,1] terms) so quantized and float nets agree by
    /// construction rather than by luck.
    pub fn forward(&self, acc: &[[f32; L0]; 2]) -> f32 {
        let mut x0 = [0f32; 2 * L0];
        for p in 0..2 {
            for j in 0..L0 {
                x0[p * L0 + j] = (acc[p][j] + self.ft_bias[j]).clamp(0.0, 1.0);
            }
        }
        let mut x1 = [0f32; L1];
        for k in 0..L1 {
            let mut y = self.l1_bias[k];
            for i in 0..2 * L0 {
                y += x0[i] * self.l1_weight[k][i];
            }
            x1[k] = y.clamp(0.0, 1.0);
        }
        let mut y2 = self.l2_bias;
        for i in 0..L1 {
            y2 += x1[i] * self.l2_weight[i];
        }
        y2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_bytes() {
        let mut net = Network::zeroed();
        net.ft_weight[0] = 123;
        net.ft_weight[N_FEATURES * L0 - 1] = -45;
        net.ft_bias[3] = 7;
        net.l1_weight[0][0] = 9;
        net.l1_bias[1] = 1000;
        net.l2_weight[5] = -3;
        net.l2_bias = 42;
        let bytes = net.to_bytes();
        let back = Network::from_bytes(&bytes).expect("loads");
        assert_eq!(back.ft_weight, net.ft_weight);
        assert_eq!(back.ft_bias, net.ft_bias);
        assert_eq!(back.l1_weight, net.l1_weight);
        assert_eq!(back.l1_bias, net.l1_bias);
        assert_eq!(back.l2_weight, net.l2_weight);
        assert_eq!(back.l2_bias, net.l2_bias);
    }

    #[test]
    fn rejects_bad_magic() {
        let bytes = vec![0u8; 64];
        assert!(Network::from_bytes(&bytes).is_err());
    }

    #[test]
    fn accumulator_cannot_overflow_i16_even_at_max_weight_and_max_active_features() {
        // 40 active features (the proven max), each contributing the maximum clamped
        // weight, plus the maximum bias, must stay well inside i16 range.
        let max_w = (FT_WEIGHT_CLAMP * FT_SCALE) as i32; // 254
        let max_b = (FT_BIAS_CLAMP * FT_SCALE) as i32; // 508
        let worst = 40 * max_w + max_b;
        assert!(worst < i16::MAX as i32, "worst-case accumulator value {worst} would overflow i16");
    }
}
