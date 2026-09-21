//! Zobrist keys generated at runtime with a fixed-seed PRNG, so no incompressible
//! random data is baked into the wasm binary.
use crate::types::*;
use std::sync::LazyLock;

struct SplitMix64(u64);
impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
}

/// PSQ[piece][sq], indexed by piece = make_piece(color,pt) (0..=31). Bit 0 is masked
/// off every entry so it never collides with the side-to-move bit trick (key ^ 1).
fn compute_psq() -> [[u64; 81]; 32] {
    let mut rng = SplitMix64(0x5367_6f67_695f_4e4e); // "Shogi_NN" ascii-ish seed, fixed
    let mut table = [[0u64; 81]; 32];
    for piece in table.iter_mut() {
        for sq in piece.iter_mut() {
            *sq = rng.next() & !1u64;
        }
    }
    table
}
pub static ZOBRIST_PSQ: LazyLock<[[u64; 81]; 32]> = LazyLock::new(compute_psq);

/// Additive increment per (color, piece type) for the hand key. Adding/removing one
/// piece is a single wrapping add/sub, which composes correctly for incremental update.
fn compute_hand_inc() -> [[u64; PIECE_TYPE_COUNT]; 2] {
    let mut rng = SplitMix64(0x486a_6e64_5f5a_6f62); // different fixed seed
    let mut table = [[0u64; PIECE_TYPE_COUNT]; 2];
    for color_table in table.iter_mut() {
        for pt in Hand_TYPES {
            color_table[pt as usize] = rng.next();
        }
    }
    table
}
#[allow(non_upper_case_globals)]
const Hand_TYPES: [PieceType; 7] = [FU, KYO, KEI, GIN, KIN, KAKU, HI];
pub static ZOBRIST_HAND_INC: LazyLock<[[u64; PIECE_TYPE_COUNT]; 2]> = LazyLock::new(compute_hand_inc);

/// XOR with the board key to flip side to move.
pub const SIDE_KEY: u64 = 1;

#[inline]
pub fn psq_key(piece: Piece, sq: Square) -> u64 {
    ZOBRIST_PSQ[piece as usize][sq as usize]
}

#[inline]
pub fn hand_inc(color: Color, pt: PieceType) -> u64 {
    ZOBRIST_HAND_INC[color as usize][pt as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn psq_entries_distinct_and_even() {
        assert_eq!(psq_key(make_piece(BLACK, FU), 0) & 1, 0);
        assert_ne!(psq_key(make_piece(BLACK, FU), 0), psq_key(make_piece(BLACK, FU), 1));
        assert_ne!(psq_key(make_piece(BLACK, FU), 0), psq_key(make_piece(WHITE, FU), 0));
    }

    #[test]
    fn hand_inc_add_sub_cancel() {
        let mut k: u64 = 12345;
        k = k.wrapping_add(hand_inc(BLACK, FU));
        k = k.wrapping_sub(hand_inc(BLACK, FU));
        assert_eq!(k, 12345);
    }
}
