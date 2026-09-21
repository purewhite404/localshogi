//! Core shogi types shared by every module.

pub type PieceType = u8;

pub const FU: PieceType = 1;
pub const KYO: PieceType = 2;
pub const KEI: PieceType = 3;
pub const GIN: PieceType = 4;
pub const KIN: PieceType = 5;
pub const KAKU: PieceType = 6;
pub const HI: PieceType = 7;
pub const OU: PieceType = 8;
pub const TO: PieceType = 9;
pub const NKYO: PieceType = 10;
pub const NKEI: PieceType = 11;
pub const NGIN: PieceType = 12;
pub const UMA: PieceType = 13;
pub const RYU: PieceType = 14;

pub const PIECE_TYPE_COUNT: usize = 15; // index 0 unused, 1..=14 valid

#[inline]
pub const fn promoted(pt: PieceType) -> PieceType {
    match pt {
        FU => TO,
        KYO => NKYO,
        KEI => NKEI,
        GIN => NGIN,
        KAKU => UMA,
        HI => RYU,
        other => other,
    }
}

#[inline]
pub const fn unpromoted(pt: PieceType) -> PieceType {
    match pt {
        TO => FU,
        NKYO => KYO,
        NKEI => KEI,
        NGIN => GIN,
        UMA => KAKU,
        RYU => HI,
        other => other,
    }
}

#[inline]
pub const fn is_promotable(pt: PieceType) -> bool {
    matches!(pt, FU | KYO | KEI | GIN | KAKU | HI)
}

#[inline]
pub const fn is_promoted(pt: PieceType) -> bool {
    pt >= TO
}

/// Base (unpromoted-equivalent) material value in centipawns, index by PieceType 1..=14.
pub const VALUES: [i32; PIECE_TYPE_COUNT] = [
    0, 90, 315, 405, 495, 540, 855, 990, 20000, 540, 540, 540, 540, 945, 1395,
];

/// CAPTURE_GAIN[pt] = value gained by capturing a piece of this type: it leaves the
/// board AND enters hand (as its unpromoted form), so its swing is roughly double.
pub const fn capture_gain(pt: PieceType) -> i32 {
    VALUES[pt as usize] + VALUES[unpromoted(pt) as usize]
}

pub type Color = u8;
pub const BLACK: Color = 0; // sente (先手)
pub const WHITE: Color = 1; // gote (後手)

#[inline]
pub const fn opposite(c: Color) -> Color {
    c ^ 1
}

/// A square 0..=80. sq = col*9 + row, where (row, col) match the UI's board[row][col]
/// convention: row 0 = gote's back rank (top of screen), col 0 = leftmost on screen.
pub type Square = u8;
pub const SQ_NONE: Square = 255;

#[inline]
pub const fn make_square(row: u8, col: u8) -> Square {
    col * 9 + row
}

#[inline]
pub const fn row_of(sq: Square) -> u8 {
    sq % 9
}

#[inline]
pub const fn col_of(sq: Square) -> u8 {
    sq / 9
}

/// A piece: 0 = empty, else (color<<4)|piece_type i.e. packed byte.
pub type Piece = u8;
pub const NO_PIECE: Piece = 0;

#[inline]
pub const fn make_piece(c: Color, pt: PieceType) -> Piece {
    (c << 4) | pt
}

#[inline]
pub const fn piece_color(p: Piece) -> Color {
    p >> 4
}

#[inline]
pub const fn piece_type(p: Piece) -> PieceType {
    p & 0x0F
}

#[inline]
pub const fn is_none(p: Piece) -> bool {
    p == NO_PIECE
}

/// True if `pt` must promote upon landing on `to_row` for `color`.
#[inline]
pub const fn must_promote(pt: PieceType, to_row: u8, color: Color) -> bool {
    match pt {
        FU | KYO => (color == BLACK && to_row == 0) || (color == WHITE && to_row == 8),
        KEI => (color == BLACK && to_row <= 1) || (color == WHITE && to_row >= 7),
        _ => false,
    }
}

/// True if `pt` can promote given it moved from `from_row` to `to_row` for `color`.
#[inline]
pub const fn can_promote(pt: PieceType, from_row: u8, to_row: u8, color: Color) -> bool {
    if !is_promotable(pt) {
        return false;
    }
    if color == BLACK {
        from_row <= 2 || to_row <= 2
    } else {
        from_row >= 6 || to_row >= 6
    }
}

/// True if dropping `pt` on `to_row` for `color` is legal with respect to the
/// "dead piece" (行き所のない駒) rule. Nifu is handled separately (needs board state).
#[inline]
pub const fn droppable_row(pt: PieceType, to_row: u8, color: Color) -> bool {
    match pt {
        FU | KYO => !((color == BLACK && to_row == 0) || (color == WHITE && to_row == 8)),
        KEI => !((color == BLACK && to_row <= 1) || (color == WHITE && to_row >= 7)),
        _ => true,
    }
}
