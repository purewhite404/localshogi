//! Packed 32-bit move encoding.
//! bits 0..=6 to | 7..=13 from | 14 promote | 15 drop | 16..=20 moved pt | 21..=25 captured pt
use crate::types::*;
use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Move(pub u32);

pub const MOVE_NONE: Move = Move(0);
/// Sentinel for a null move (side passes); distinct from MOVE_NONE via a reserved bit.
pub const MOVE_NULL: Move = Move(1 << 26);

impl Move {
    #[inline]
    pub fn new_board(from: Square, to: Square, moved: PieceType, promote: bool, captured: PieceType) -> Move {
        let mut bits = (to as u32) | ((from as u32) << 7) | ((moved as u32) << 16) | ((captured as u32) << 21);
        if promote {
            bits |= 1 << 14;
        }
        Move(bits)
    }

    #[inline]
    pub fn new_drop(pt: PieceType, to: Square) -> Move {
        Move((to as u32) | (1 << 15) | ((pt as u32) << 16))
    }

    #[inline]
    pub fn to(self) -> Square {
        (self.0 & 0x7F) as Square
    }

    #[inline]
    pub fn from(self) -> Square {
        ((self.0 >> 7) & 0x7F) as Square
    }

    #[inline]
    pub fn is_promotion(self) -> bool {
        (self.0 >> 14) & 1 != 0
    }

    #[inline]
    pub fn is_drop(self) -> bool {
        (self.0 >> 15) & 1 != 0
    }

    #[inline]
    pub fn moved_piece_type(self) -> PieceType {
        ((self.0 >> 16) & 0x1F) as PieceType
    }

    #[inline]
    pub fn captured_piece_type(self) -> PieceType {
        ((self.0 >> 21) & 0x1F) as PieceType
    }

    #[inline]
    pub fn is_capture(self) -> bool {
        self.captured_piece_type() != 0
    }

    #[inline]
    pub fn is_none(self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub fn is_null(self) -> bool {
        self == MOVE_NULL
    }

    /// The 16-bit "core" used as the TT-stored move: board-independent, so it never
    /// goes stale even though `captured` (board-dependent) is dropped.
    #[inline]
    pub fn core16(self) -> u16 {
        (self.0 & 0xFFFF) as u16
    }
}

impl fmt::Debug for Move {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_none() {
            return write!(f, "none");
        }
        if self.is_drop() {
            write!(f, "{}*{}", self.moved_piece_type(), self.to())
        } else {
            write!(
                f,
                "{}{}{}{}",
                self.from(),
                self.to(),
                if self.is_promotion() { "+" } else { "" },
                if self.is_capture() { "x" } else { "" }
            )
        }
    }
}

/// A move plus its ordering score, used in move lists (avoid storing a score inside
/// `Move` itself so the packed encoding stays canonical).
#[derive(Clone, Copy, Debug, Default)]
pub struct ExtMove {
    pub mv: Move,
    pub score: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_move_roundtrip() {
        let mv = Move::new_board(10, 20, GIN, true, KAKU);
        assert_eq!(mv.from(), 10);
        assert_eq!(mv.to(), 20);
        assert_eq!(mv.moved_piece_type(), GIN);
        assert!(mv.is_promotion());
        assert!(!mv.is_drop());
        assert_eq!(mv.captured_piece_type(), KAKU);
        assert!(mv.is_capture());
    }

    #[test]
    fn drop_move_roundtrip() {
        let mv = Move::new_drop(FU, 42);
        assert!(mv.is_drop());
        assert_eq!(mv.to(), 42);
        assert_eq!(mv.moved_piece_type(), FU);
        assert!(!mv.is_capture());
        assert!(!mv.is_promotion());
    }

    #[test]
    fn move_none_is_zero() {
        assert!(MOVE_NONE.is_none());
        assert_eq!(MOVE_NONE.0, 0);
    }
}
