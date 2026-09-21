//! 81-square bitboard as two u64 words, split at column 7 (sq 63) so that every
//! file (9 contiguous squares, since sq = col*9+row) lives entirely inside one word.
use crate::types::Square;
use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign, Not};

pub const WORD0_SQUARES: u32 = 63; // cols 0..=6, 7*9
pub const WORD1_SQUARES: u32 = 18; // cols 7..=8, 2*9

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
#[repr(align(16))]
pub struct Bitboard {
    pub p: [u64; 2],
}

impl Bitboard {
    pub const EMPTY: Bitboard = Bitboard { p: [0, 0] };

    #[inline]
    pub const fn from_word(word: usize, bits: u64) -> Bitboard {
        if word == 0 {
            Bitboard { p: [bits, 0] }
        } else {
            Bitboard { p: [0, bits] }
        }
    }

    #[inline]
    pub const fn single(sq: Square) -> Bitboard {
        let sq = sq as u32;
        if sq < WORD0_SQUARES {
            Bitboard { p: [1u64 << sq, 0] }
        } else {
            Bitboard { p: [0, 1u64 << (sq - WORD0_SQUARES)] }
        }
    }

    #[inline]
    pub const fn contains(self, sq: Square) -> bool {
        let sq = sq as u32;
        if sq < WORD0_SQUARES {
            (self.p[0] >> sq) & 1 != 0
        } else {
            (self.p[1] >> (sq - WORD0_SQUARES)) & 1 != 0
        }
    }

    #[inline]
    pub fn set(&mut self, sq: Square) {
        *self |= Bitboard::single(sq);
    }

    #[inline]
    pub fn clear(&mut self, sq: Square) {
        *self &= !Bitboard::single(sq);
    }

    #[inline]
    pub fn toggle(&mut self, sq: Square) {
        *self ^= Bitboard::single(sq);
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.p[0] == 0 && self.p[1] == 0
    }

    #[inline]
    pub const fn count_ones(self) -> u32 {
        self.p[0].count_ones() + self.p[1].count_ones()
    }

    /// Lowest-numbered set square, or None if empty.
    #[inline]
    pub fn lsb(self) -> Option<Square> {
        if self.p[0] != 0 {
            Some(self.p[0].trailing_zeros() as Square)
        } else if self.p[1] != 0 {
            Some((WORD0_SQUARES + self.p[1].trailing_zeros()) as Square)
        } else {
            None
        }
    }

    /// Highest-numbered set square, or None if empty.
    #[inline]
    pub fn msb(self) -> Option<Square> {
        if self.p[1] != 0 {
            Some((WORD0_SQUARES + 63 - self.p[1].leading_zeros()) as Square)
        } else if self.p[0] != 0 {
            Some((63 - self.p[0].leading_zeros()) as Square)
        } else {
            None
        }
    }

    /// Pop and return the lowest-numbered set square.
    #[inline]
    pub fn pop_lsb(&mut self) -> Option<Square> {
        let sq = self.lsb()?;
        self.clear(sq);
        Some(sq)
    }

    pub fn iter(self) -> BitboardIter {
        BitboardIter(self)
    }
}

pub struct BitboardIter(Bitboard);
impl Iterator for BitboardIter {
    type Item = Square;
    #[inline]
    fn next(&mut self) -> Option<Square> {
        self.0.pop_lsb()
    }
}

impl BitAnd for Bitboard {
    type Output = Bitboard;
    #[inline]
    fn bitand(self, rhs: Bitboard) -> Bitboard {
        Bitboard { p: [self.p[0] & rhs.p[0], self.p[1] & rhs.p[1]] }
    }
}
impl BitAndAssign for Bitboard {
    #[inline]
    fn bitand_assign(&mut self, rhs: Bitboard) {
        self.p[0] &= rhs.p[0];
        self.p[1] &= rhs.p[1];
    }
}
impl BitOr for Bitboard {
    type Output = Bitboard;
    #[inline]
    fn bitor(self, rhs: Bitboard) -> Bitboard {
        Bitboard { p: [self.p[0] | rhs.p[0], self.p[1] | rhs.p[1]] }
    }
}
impl BitOrAssign for Bitboard {
    #[inline]
    fn bitor_assign(&mut self, rhs: Bitboard) {
        self.p[0] |= rhs.p[0];
        self.p[1] |= rhs.p[1];
    }
}
impl BitXor for Bitboard {
    type Output = Bitboard;
    #[inline]
    fn bitxor(self, rhs: Bitboard) -> Bitboard {
        Bitboard { p: [self.p[0] ^ rhs.p[0], self.p[1] ^ rhs.p[1]] }
    }
}
impl BitXorAssign for Bitboard {
    #[inline]
    fn bitxor_assign(&mut self, rhs: Bitboard) {
        self.p[0] ^= rhs.p[0];
        self.p[1] ^= rhs.p[1];
    }
}
impl Not for Bitboard {
    type Output = Bitboard;
    #[inline]
    fn not(self) -> Bitboard {
        // Mask to exactly 81 valid bits so stray high bits never leak into counts/iteration.
        Bitboard { p: [!self.p[0] & ((1u64 << WORD0_SQUARES) - 1), !self.p[1] & ((1u64 << WORD1_SQUARES) - 1)] }
    }
}

impl Bitboard {
    #[inline]
    pub fn andnot(self, rhs: Bitboard) -> Bitboard {
        self & !rhs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_and_contains_all_squares() {
        for sq in 0u8..81 {
            let b = Bitboard::single(sq);
            assert_eq!(b.count_ones(), 1);
            for other in 0u8..81 {
                assert_eq!(b.contains(other), other == sq, "sq={sq} other={other}");
            }
        }
    }

    #[test]
    fn word_split_boundary() {
        // sq 62 is the last square in word0 (col 6, row 8); sq 63 is first of word1 (col 7, row 0).
        assert_eq!(Bitboard::single(62).p, [1u64 << 62, 0]);
        assert_eq!(Bitboard::single(63).p, [0, 1]);
        assert_eq!(Bitboard::single(80).p, [0, 1u64 << 17]);
    }

    #[test]
    fn iter_matches_contains() {
        let mut b = Bitboard::EMPTY;
        for sq in [0, 5, 40, 63, 80] {
            b.set(sq);
        }
        let collected: Vec<u8> = b.iter().collect();
        assert_eq!(collected, vec![0, 5, 40, 63, 80]);
    }

    #[test]
    fn not_masks_to_81_bits() {
        let full = !Bitboard::EMPTY;
        assert_eq!(full.count_ones(), 81);
        for sq in 0u8..81 {
            assert!(full.contains(sq));
        }
    }

    #[test]
    fn pop_lsb_drains_all() {
        let mut b = Bitboard::EMPTY;
        for sq in [3u8, 70, 10, 80, 0] {
            b.set(sq);
        }
        let mut got = vec![];
        while let Some(sq) = b.pop_lsb() {
            got.push(sq);
        }
        got.sort();
        assert_eq!(got, vec![0, 3, 10, 70, 80]);
        assert!(b.is_empty());
    }
}
