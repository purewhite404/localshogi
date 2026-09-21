//! Bitboard port of the original hand-crafted evaluator (`src/lib.rs` in the previous
//! version): an "effect map" (attack density near each king, capped at 2 attackers per
//! square) plus material with a small bonus for defended/attacked pieces.
//!
//! Fix versus the original: the old code added `700 - row*30` to Black's king score
//! (and the mirror for White), which *rewards marching the king up the board* — a bug,
//! since `row` decreases toward the enemy side for Black. That term is removed; king
//! safety is carried entirely by the attack-density-near-king terms below, which are
//! symmetric and don't reward advancing.
use super::Evaluator;
use crate::bitboard::Bitboard;
use crate::mov::Move;
use crate::movegen::piece_attacks;
use crate::position::Position;
use crate::types::*;

pub struct HandCrafted;

impl HandCrafted {
    pub fn new() -> HandCrafted {
        HandCrafted
    }
}

impl Default for HandCrafted {
    fn default() -> Self {
        Self::new()
    }
}

impl Evaluator for HandCrafted {
    fn refresh(&mut self, _pos: &Position) {}
    fn push(&mut self, _pos: &Position, _mv: Move, _mover: Color) {}
    fn pop(&mut self) {}

    fn eval(&mut self, pos: &Position) -> i32 {
        let side_score = evaluate(pos);
        if pos.side_to_move() == BLACK {
            side_score
        } else {
            -side_score
        }
    }
}

/// Score from Black's (sente's) point of view, in centipawns.
pub fn evaluate(pos: &Position) -> i32 {
    let occ = pos.occupied();
    let mut effects = [[0u8; 81]; 2];
    for &color in &[BLACK, WHITE] {
        let idx = color as usize;
        for sq in pos.pieces_of_color(color).iter() {
            let pt = piece_type(pos.piece_at(sq));
            let targets = piece_attacks(color, pt, sq, occ);
            for t in targets.iter() {
                let c = &mut effects[idx][t as usize];
                *c = (*c + 1).min(2);
            }
        }
    }

    let black_king = pos.king_square(BLACK);
    let white_king = pos.king_square(WHITE);
    let mut score: i32 = 0;

    for sq in 0u8..81 {
        let b = effects[0][sq as usize] as i32;
        let w = effects[1][sq as usize] as i32;
        let bd = king_dist(black_king, sq) + 1;
        let wd = king_dist(white_king, sq) + 1;
        score += 83 * b / bd - 92 * w / bd + 92 * b / wd - 83 * w / wd;

        let piece = pos.piece_at(sq);
        if is_none(piece) {
            continue;
        }
        let pt = piece_type(piece);
        let color = piece_color(piece);
        if pt == OU {
            continue; // king material/position handled entirely by the terms above
        }
        let sign = if color == BLACK { 1 } else { -1 };
        let bonus = (if b > 0 && color == BLACK { 33 } else { 0 }) + (if w > 0 && color == WHITE { 33 } else { 0 });
        score += sign * VALUES[pt as usize] * (1000 + bonus) / 1000;
    }

    for &pt in &crate::hand::Hand::TYPES {
        score += VALUES[pt as usize] * pos.hand(BLACK, pt) * 8 / 10;
        score -= VALUES[pt as usize] * pos.hand(WHITE, pt) * 8 / 10;
    }
    score
}

#[inline]
fn king_dist(king: Square, sq: Square) -> i32 {
    if king == SQ_NONE {
        return 8;
    }
    let dr = (row_of(king) as i32 - row_of(sq) as i32).abs();
    let dc = (col_of(king) as i32 - col_of(sq) as i32).abs();
    dr.max(dc)
}

#[allow(unused_imports)]
use Bitboard as _UnusedBitboardImport;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startpos_is_near_zero() {
        let pos = Position::startpos();
        let s = evaluate(&pos);
        assert!(s.abs() < 60, "startpos eval should be near zero, got {s}");
    }

    #[test]
    fn extra_rook_is_a_large_advantage() {
        let sfen = "9/9/9/9/4k4/9/9/9/4K4 b R 1";
        let pos = crate::sfen::parse_sfen(sfen).unwrap();
        let s = evaluate(&pos);
        assert!(s > 500, "black up a rook in hand should score well above zero, got {s}");
    }
}
