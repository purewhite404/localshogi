//! The feature-transformer accumulator: one `[i16; L0]` per perspective. Because the
//! feature set has no king-relative component (see `nnue.rs`), the accumulator is
//! *always* fully incremental — there is no refresh-on-king-move case to get wrong.
use super::features::active_features;
use super::network::{Network, L0};
use crate::position::Position;
use crate::types::{BLACK, WHITE};

pub type Frame = [[i16; L0]; 2]; // [perspective 0=BLACK, 1=WHITE]

pub fn refresh(pos: &Position, net: &Network) -> Frame {
    let mut frame = [[0i16; L0]; 2];
    let mut feats = Vec::with_capacity(48);
    for &perspective in &[BLACK, WHITE] {
        let mut acc = net.ft_bias;
        active_features(pos, perspective, &mut feats);
        for &f in &feats {
            let row = net.ft_row(f as usize);
            for j in 0..L0 {
                acc[j] = acc[j].wrapping_add(row[j]);
            }
        }
        frame[perspective as usize] = acc;
    }
    frame
}

#[inline]
pub fn add_col(acc: &mut [i16; L0], feature: usize, net: &Network) {
    let row = net.ft_row(feature);
    for j in 0..L0 {
        acc[j] = acc[j].wrapping_add(row[j]);
    }
}

#[inline]
pub fn sub_col(acc: &mut [i16; L0], feature: usize, net: &Network) {
    let row = net.ft_row(feature);
    for j in 0..L0 {
        acc[j] = acc[j].wrapping_sub(row[j]);
    }
}
