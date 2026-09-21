//! NNUE feature indexing. Simplified from the original design (no king-relative
//! block — see the module doc on `nnue.rs` for why): two blocks only.
//!
//!   A. Absolute piece-square: 81 squares x 14 piece types x 2 (own/opp) = 2268
//!   B. Hand, unary/thermometer: 2 x (18+4+4+4+4+2+2) = 76
//!
//! N = 2344. A perspective's feature set is entirely determined by (board, hands) —
//! there is no king-relative component, so an incremental update never needs a full
//! accumulator refresh: every move touches a small, statically-bounded number of
//! features (see `nnue.rs::push`).
//!
//! Every consumer (inference AND training) MUST go through `active_features` here —
//! never re-derive the indexing elsewhere. A second implementation of this mapping is
//! the single most common way an NNUE silently fails (train/inference mismatch).
use crate::hand::Hand;
use crate::position::Position;
use crate::types::*;

pub const ABS_BASE: usize = 0;
pub const ABS_SIZE: usize = 81 * 14 * 2;
pub const HAND_BASE: usize = ABS_BASE + ABS_SIZE;
pub const HAND_SIZE: usize = 76;
pub const N_FEATURES: usize = HAND_BASE + HAND_SIZE;

/// Per-hand-piece-type (base offset, max count) within one color's 38-wide half of
/// the hand block.
const HAND_LAYOUT: [(usize, i32); PIECE_TYPE_COUNT] = {
    let mut l = [(0usize, 0i32); PIECE_TYPE_COUNT];
    l[FU as usize] = (0, 18);
    l[KYO as usize] = (18, 4);
    l[KEI as usize] = (22, 4);
    l[GIN as usize] = (26, 4);
    l[KIN as usize] = (30, 4);
    l[KAKU as usize] = (34, 2);
    l[HI as usize] = (36, 2);
    l
};
const HAND_HALF_WIDTH: usize = 38;

/// 180-degree rotation: the perspective transform that lets one network serve both
/// colors (own king's forward direction is always "up" for the perspective it's fed
/// with, since a whole-board rotation is a symmetry of the game as far as the
/// feature set below is concerned — nothing here reads absolute rank/file meaning).
#[inline]
pub fn psq(sq: Square, perspective: Color) -> Square {
    if perspective == BLACK {
        sq
    } else {
        80 - sq
    }
}

#[inline]
pub fn abs_index(owner: Color, pt: PieceType, sq: Square, perspective: Color) -> usize {
    let powner = if owner == perspective { 0 } else { 1 };
    ABS_BASE + (powner * 14 + (pt as usize - 1)) * 81 + psq(sq, perspective) as usize
}

/// `k` is a 1-based count level (thermometer): having `n` pieces of `pt` in `owner`'s
/// hand activates indices for k=1..=n simultaneously.
#[inline]
pub fn hand_index(owner: Color, pt: PieceType, k: i32, perspective: Color) -> usize {
    let powner = if owner == perspective { 0 } else { 1 };
    let (base, _max) = HAND_LAYOUT[pt as usize];
    HAND_BASE + powner * HAND_HALF_WIDTH + base + (k as usize - 1)
}

/// All active feature indices for `perspective` at the current position. Exactly
/// `board_piece_count + total_hand_piece_count` = 40 (every one of the 40 pieces in
/// shogi is either on the board or in some hand, never both, never neither) — this
/// invariant is asserted in tests and is the overflow-safety argument for the
/// quantized accumulator (see `network.rs`).
pub fn active_features(pos: &Position, perspective: Color, out: &mut Vec<u16>) {
    out.clear();
    for sq in 0u8..81 {
        let p = pos.piece_at(sq);
        if !is_none(p) {
            out.push(abs_index(piece_color(p), piece_type(p), sq, perspective) as u16);
        }
    }
    for &color in &[BLACK, WHITE] {
        for &pt in &Hand::TYPES {
            let n = pos.hand(color, pt);
            for k in 1..=n {
                out.push(hand_index(color, pt, k, perspective) as u16);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::Position;

    #[test]
    fn feature_count_matches_spec() {
        assert_eq!(N_FEATURES, 2344);
    }

    #[test]
    fn active_feature_count_is_always_40_at_startpos() {
        let pos = Position::startpos();
        let mut feats = Vec::new();
        active_features(&pos, BLACK, &mut feats);
        assert_eq!(feats.len(), 40);
        active_features(&pos, WHITE, &mut feats);
        assert_eq!(feats.len(), 40);
    }

    #[test]
    fn indices_are_in_range_and_perspectives_differ() {
        let pos = Position::startpos();
        let mut b = Vec::new();
        let mut w = Vec::new();
        active_features(&pos, BLACK, &mut b);
        active_features(&pos, WHITE, &mut w);
        for &f in b.iter().chain(w.iter()) {
            assert!((f as usize) < N_FEATURES);
        }
        // Startpos is not perspective-symmetric in raw index space (BLACK's own pieces
        // occupy different squares than WHITE's own pieces even after rotation is
        // applied via psq), so the two active sets should differ.
        assert_ne!(b, w);
    }

    #[test]
    fn thermometer_hand_encoding_is_monotonic() {
        let sfen = "4k4/9/9/9/9/9/9/9/4K4 b 3P 1";
        let pos = crate::sfen::parse_sfen(sfen).unwrap();
        let mut feats = Vec::new();
        active_features(&pos, BLACK, &mut feats);
        let i1 = hand_index(BLACK, FU, 1, BLACK);
        let i2 = hand_index(BLACK, FU, 2, BLACK);
        let i3 = hand_index(BLACK, FU, 3, BLACK);
        for i in [i1, i2, i3] {
            assert!(feats.contains(&(i as u16)), "expected feature {i} active for 3 pawns in hand");
        }
    }
}
