//! Packed per-side hand-piece counts. Each field has a spare guard bit so no field can
//! ever overflow into its neighbour, even transiently during incremental updates.
use crate::types::*;

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Hand(pub u32);

/// (bit shift, bit width, max count) for each hand-holding piece type, indexed by
/// PieceType. Widths are the minimum needed for `max count` (5 bits for pawn's 18, 3
/// bits for the 4-max pieces, 2 bits for the 2-max pieces) with a spare guard bit
/// between fields. The read mask MUST use `width`, not a fixed width, or two fields
/// packed only a few bits apart (e.g. KIN at shift 18, KAKU at shift 22) alias into
/// each other — this was a real bug caught by a perft mismatch (see tests below).
const LAYOUT: [(u32, u32, i32); PIECE_TYPE_COUNT] = {
    let mut l = [(0u32, 0u32, 0i32); PIECE_TYPE_COUNT];
    l[FU as usize] = (0, 5, 18);
    l[KYO as usize] = (6, 3, 4);
    l[KEI as usize] = (10, 3, 4);
    l[GIN as usize] = (14, 3, 4);
    l[KIN as usize] = (18, 3, 4);
    l[KAKU as usize] = (22, 2, 2);
    l[HI as usize] = (26, 2, 2);
    l
};

impl Hand {
    pub const EMPTY: Hand = Hand(0);

    #[inline]
    pub fn count(self, pt: PieceType) -> i32 {
        let (shift, width, _) = LAYOUT[pt as usize];
        let mask = (1u32 << width) - 1;
        ((self.0 >> shift) & mask) as i32
    }

    #[inline]
    pub fn has(self, pt: PieceType) -> bool {
        self.count(pt) > 0
    }

    #[inline]
    pub fn add(&mut self, pt: PieceType) {
        let (shift, _, _) = LAYOUT[pt as usize];
        self.0 += 1 << shift;
    }

    #[inline]
    pub fn remove(&mut self, pt: PieceType) {
        let (shift, _, _) = LAYOUT[pt as usize];
        debug_assert!(self.count(pt) > 0, "removing from empty hand slot");
        self.0 -= 1 << shift;
    }

    #[inline]
    pub fn max_count(pt: PieceType) -> i32 {
        LAYOUT[pt as usize].2
    }

    /// All seven droppable piece types, for iteration.
    pub const TYPES: [PieceType; 7] = [FU, KYO, KEI, GIN, KIN, KAKU, HI];

    pub fn is_empty(self) -> bool {
        Hand::TYPES.iter().all(|&pt| self.count(pt) == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_remove_roundtrip_all_types() {
        let mut h = Hand::EMPTY;
        for &pt in &Hand::TYPES {
            for _ in 0..Hand::max_count(pt) {
                h.add(pt);
            }
            assert_eq!(h.count(pt), Hand::max_count(pt));
        }
        for &pt in &Hand::TYPES {
            for _ in 0..Hand::max_count(pt) {
                h.remove(pt);
            }
            assert_eq!(h.count(pt), 0);
        }
        assert_eq!(h, Hand::EMPTY);
    }

    #[test]
    fn fields_do_not_bleed_into_neighbours() {
        let mut h = Hand::EMPTY;
        for _ in 0..18 {
            h.add(FU);
        }
        assert_eq!(h.count(FU), 18);
        assert_eq!(h.count(KYO), 0);
        h.add(KYO);
        assert_eq!(h.count(FU), 18);
        assert_eq!(h.count(KYO), 1);
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;

    /// The bug this guards against: `count()` used to mask every field with a fixed
    /// 5 bits (0x1F) regardless of that field's actual allocated width, so a KAKU
    /// (2-bit field at shift 22) bled into KIN's (3-bit field at shift 18) read
    /// window. This was caught by a perft(5) mismatch traced down to a position where
    /// black had a bishop in hand and the engine spuriously believed it also had a
    /// gold. Each field must be independent of every other field's count.
    #[test]
    fn fields_are_independent_at_max_count() {
        for &probe in &Hand::TYPES {
            let mut h = Hand::EMPTY;
            for _ in 0..Hand::max_count(probe) {
                h.add(probe);
            }
            for &other in &Hand::TYPES {
                if other == probe {
                    continue;
                }
                assert_eq!(h.count(other), 0, "{other} leaked from a maxed-out {probe} field");
            }
        }
    }
}
