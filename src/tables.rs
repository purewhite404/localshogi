//! Runtime-generated lookup tables: ray masks, direction-between, and per-piece step
//! attacks. Generated once on first use via `LazyLock` so nothing incompressible is
//! baked into the wasm binary.
use crate::bitboard::Bitboard;
use crate::types::*;
use std::sync::LazyLock;

/// Direction indices, chosen so `opposite(d) = d ^ 1`:
/// 0=N 1=S 2=W 3=E 4=NW 5=SE 6=NE 7=SW  (row,col deltas below)
pub const DIR_DELTA: [(i32, i32); 8] = [
    (-1, 0),
    (1, 0),
    (0, -1),
    (0, 1),
    (-1, -1),
    (1, 1),
    (-1, 1),
    (1, -1),
];
/// Whether walking in this direction increases the raw `sq` value (used to find the
/// nearest blocker on a ray without a general "distance" table: lsb for increasing
/// directions, msb for decreasing ones).
pub const DIR_INCREASING: [bool; 8] = [false, true, false, true, false, true, true, false];

pub const ROOK_DIRS: [u8; 4] = [0, 1, 2, 3];
pub const BISHOP_DIRS: [u8; 4] = [4, 5, 6, 7];

#[inline]
pub const fn opposite_dir(d: u8) -> u8 {
    d ^ 1
}

#[inline]
pub fn sq_step(sq: Square, dr: i32, dc: i32) -> Option<Square> {
    let row = row_of(sq) as i32 + dr;
    let col = col_of(sq) as i32 + dc;
    if (0..9).contains(&row) && (0..9).contains(&col) {
        Some(make_square(row as u8, col as u8))
    } else {
        None
    }
}

/// forward direction index for a color: BLACK moves toward row 0 (N), WHITE toward row 8 (S).
#[inline]
pub const fn forward_dir(color: Color) -> u8 {
    if color == BLACK {
        0
    } else {
        1
    }
}

fn compute_rays() -> [[Bitboard; 8]; 81] {
    let mut rays = [[Bitboard::EMPTY; 8]; 81];
    for sq in 0u8..81 {
        for d in 0u8..8 {
            let (dr, dc) = DIR_DELTA[d as usize];
            let mut cur = sq;
            let mut bb = Bitboard::EMPTY;
            while let Some(next) = sq_step(cur, dr, dc) {
                bb.set(next);
                cur = next;
            }
            rays[sq as usize][d as usize] = bb;
        }
    }
    rays
}

pub static RAY: LazyLock<[[Bitboard; 8]; 81]> = LazyLock::new(compute_rays);

fn compute_directions() -> [[i8; 81]; 81] {
    let mut dirs = [[-1i8; 81]; 81];
    for from in 0u8..81 {
        for d in 0u8..8 {
            for to in RAY[from as usize][d as usize].iter() {
                dirs[from as usize][to as usize] = d as i8;
            }
        }
    }
    dirs
}

/// DIRECTION[from][to] = direction index from `from` toward `to` if they lie on a
/// common rank/file/diagonal, else -1.
pub static DIRECTION: LazyLock<[[i8; 81]; 81]> = LazyLock::new(compute_directions);

#[inline]
pub fn direction_between(a: Square, b: Square) -> Option<u8> {
    let d = DIRECTION[a as usize][b as usize];
    if d < 0 {
        None
    } else {
        Some(d as u8)
    }
}

/// Squares strictly between `a` and `b` if they are aligned, else empty.
#[inline]
pub fn between(a: Square, b: Square) -> Bitboard {
    match direction_between(a, b) {
        Some(d) => RAY[a as usize][d as usize] & RAY[b as usize][opposite_dir(d) as usize],
        None => Bitboard::EMPTY,
    }
}

/// True if `b` and `c` lie on a common line through `a` (a is the pivot, e.g. a king).
#[inline]
pub fn aligned(a: Square, b: Square, c: Square) -> bool {
    match (direction_between(a, b), direction_between(a, c)) {
        (Some(d1), Some(d2)) => d1 == d2 || d1 == opposite_dir(d2),
        _ => false,
    }
}

/// Attack (a.k.a. sliding-ray) bitboard from `sq` in direction `d`, stopping at and
/// including the first occupied square.
#[inline]
pub fn ray_attacks(sq: Square, d: u8, occ: Bitboard) -> Bitboard {
    let ray = RAY[sq as usize][d as usize];
    let blockers = ray & occ;
    if blockers.is_empty() {
        return ray;
    }
    let blocker = if DIR_INCREASING[d as usize] { blockers.lsb() } else { blockers.msb() }.unwrap();
    ray ^ RAY[blocker as usize][d as usize]
}

#[inline]
pub fn bishop_attacks(sq: Square, occ: Bitboard) -> Bitboard {
    let mut bb = Bitboard::EMPTY;
    for &d in &BISHOP_DIRS {
        bb |= ray_attacks(sq, d, occ);
    }
    bb
}

#[inline]
pub fn rook_attacks(sq: Square, occ: Bitboard) -> Bitboard {
    let mut bb = Bitboard::EMPTY;
    for &d in &ROOK_DIRS {
        bb |= ray_attacks(sq, d, occ);
    }
    bb
}

#[inline]
pub fn lance_attacks(color: Color, sq: Square, occ: Bitboard) -> Bitboard {
    ray_attacks(sq, forward_dir(color), occ)
}

/// King-step (single square, all 8 neighbours) bitboard, used both for the king itself
/// and as the extra single-step component of horse/dragon.
fn compute_king_steps() -> [Bitboard; 81] {
    let mut out = [Bitboard::EMPTY; 81];
    for sq in 0u8..81 {
        let mut bb = Bitboard::EMPTY;
        for &(dr, dc) in &DIR_DELTA {
            if let Some(t) = sq_step(sq, dr, dc) {
                bb.set(t);
            }
        }
        out[sq as usize] = bb;
    }
    out
}
pub static KING_STEPS: LazyLock<[Bitboard; 81]> = LazyLock::new(compute_king_steps);

fn compute_orth_steps() -> [Bitboard; 81] {
    let mut out = [Bitboard::EMPTY; 81];
    for sq in 0u8..81 {
        let mut bb = Bitboard::EMPTY;
        for &d in &ROOK_DIRS {
            let (dr, dc) = DIR_DELTA[d as usize];
            if let Some(t) = sq_step(sq, dr, dc) {
                bb.set(t);
            }
        }
        out[sq as usize] = bb;
    }
    out
}
pub static ORTH_STEPS: LazyLock<[Bitboard; 81]> = LazyLock::new(compute_orth_steps);

fn compute_diag_steps() -> [Bitboard; 81] {
    let mut out = [Bitboard::EMPTY; 81];
    for sq in 0u8..81 {
        let mut bb = Bitboard::EMPTY;
        for &d in &BISHOP_DIRS {
            let (dr, dc) = DIR_DELTA[d as usize];
            if let Some(t) = sq_step(sq, dr, dc) {
                bb.set(t);
            }
        }
        out[sq as usize] = bb;
    }
    out
}
pub static DIAG_STEPS: LazyLock<[Bitboard; 81]> = LazyLock::new(compute_diag_steps);

#[inline]
pub fn horse_attacks(sq: Square, occ: Bitboard) -> Bitboard {
    bishop_attacks(sq, occ) | ORTH_STEPS[sq as usize]
}

#[inline]
pub fn dragon_attacks(sq: Square, occ: Bitboard) -> Bitboard {
    rook_attacks(sq, occ) | DIAG_STEPS[sq as usize]
}

/// Step-piece deltas (row, col) for a BLACK-forward-oriented piece; mirrored for WHITE.
fn step_deltas(pt: PieceType) -> &'static [(i32, i32)] {
    match pt {
        FU => &[(-1, 0)],
        KEI => &[(-2, -1), (-2, 1)],
        GIN => &[(-1, -1), (-1, 0), (-1, 1), (1, -1), (1, 1)],
        KIN | TO | NKYO | NKEI | NGIN => &[(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, 0)],
        OU => &[(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)],
        _ => &[],
    }
}

fn compute_step_attacks() -> [[[Bitboard; PIECE_TYPE_COUNT]; 81]; 2] {
    let mut out = [[[Bitboard::EMPTY; PIECE_TYPE_COUNT]; 81]; 2];
    for &color in &[BLACK, WHITE] {
        let sign: i32 = if color == BLACK { 1 } else { -1 };
        for pt in [FU, KEI, GIN, KIN, TO, NKYO, NKEI, NGIN, OU] {
            for sq in 0u8..81 {
                let mut bb = Bitboard::EMPTY;
                for &(dr, dc) in step_deltas(pt) {
                    if let Some(t) = sq_step(sq, dr * sign, dc * sign) {
                        bb.set(t);
                    }
                }
                out[color as usize][sq as usize][pt as usize] = bb;
            }
        }
    }
    out
}

/// STEP_ATTACKS[color][sq][piece_type] = squares a step-moving piece of this
/// color/type at `sq` attacks. Sliders (KYO/KAKU/HI/UMA/RYU) are not included here;
/// use the ray/bishop/rook/horse/dragon functions for those.
pub static STEP_ATTACKS: LazyLock<[[[Bitboard; PIECE_TYPE_COUNT]; 81]; 2]> = LazyLock::new(compute_step_attacks);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_and_between_sanity() {
        // sq = col*9+row. sq0=(row0,col0), sq3=(row3,col0): same column -> vertical line.
        let a = make_square(0, 0);
        let b = make_square(3, 0);
        assert!(direction_between(a, b).is_some());
        let bw = between(a, b);
        assert_eq!(bw.count_ones(), 2); // rows 1,2
        assert!(bw.contains(make_square(1, 0)));
        assert!(bw.contains(make_square(2, 0)));
    }

    #[test]
    fn unaligned_squares_have_no_direction() {
        let a = make_square(0, 0);
        let b = make_square(1, 3); // knight-offset, not aligned
        assert!(direction_between(a, b).is_none());
        assert!(between(a, b).is_empty());
    }

    #[test]
    fn aligned_pivot_both_sides() {
        let k = make_square(4, 4);
        let s1 = make_square(2, 4);
        let s2 = make_square(6, 4);
        assert!(aligned(k, s1, s2));
        let off = make_square(4, 5);
        assert!(!aligned(k, s1, off));
    }

    #[test]
    fn ray_attacks_stops_at_blocker() {
        let sq = make_square(4, 4);
        let mut occ = Bitboard::EMPTY;
        occ.set(make_square(2, 4)); // 2 north of sq along dir0(N)
        let att = ray_attacks(sq, 0, occ);
        assert!(att.contains(make_square(3, 4)));
        assert!(att.contains(make_square(2, 4)));
        assert!(!att.contains(make_square(1, 4)));
        assert!(!att.contains(make_square(0, 4)));
    }

    #[test]
    fn ray_attacks_no_blocker_reaches_edge() {
        let sq = make_square(4, 4);
        let att = ray_attacks(sq, 1, Bitboard::EMPTY); // south, no blockers
        for r in 5..9 {
            assert!(att.contains(make_square(r, 4)));
        }
    }

    #[test]
    fn pawn_step_direction_differs_by_color() {
        let sq = make_square(4, 4);
        let black = STEP_ATTACKS[BLACK as usize][sq as usize][FU as usize];
        let white = STEP_ATTACKS[WHITE as usize][sq as usize][FU as usize];
        assert!(black.contains(make_square(3, 4)));
        assert!(white.contains(make_square(5, 4)));
    }
}
