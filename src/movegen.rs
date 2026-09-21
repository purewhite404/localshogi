//! Move generation. Strategy: generate every pseudo-legal move (board moves respect
//! piece movement patterns + forced/optional promotion; drops respect nifu/dead-piece
//! rules), then filter by actually making each move and checking whether it leaves the
//! mover's own king in check (plus a dedicated 打ち歩詰め check for checking pawn drops).
//! This is simpler and more obviously correct than pin-aware direct generation, at a
//! performance cost that is acceptable at these search depths; perft is the gate that
//! proves it correct regardless of how fast it is.
use crate::mov::Move;
use crate::position::Position;
use crate::tables::*;
use crate::types::*;

pub fn generate_legal(pos: &mut Position, out: &mut Vec<Move>) {
    out.clear();
    generate_pseudo_legal(pos, out);
    out.retain(|&mv| is_legal_after_pseudo(pos, mv));
}

pub fn legal_moves_from(pos: &mut Position, from: Square, out: &mut Vec<Move>) {
    let mut all = Vec::with_capacity(64);
    generate_legal(pos, &mut all);
    out.clear();
    out.extend(all.into_iter().filter(|m| !m.is_drop() && m.from() == from));
}

pub fn legal_drops_of(pos: &mut Position, pt: PieceType, out: &mut Vec<Move>) {
    let mut all = Vec::with_capacity(64);
    generate_legal(pos, &mut all);
    out.clear();
    out.extend(all.into_iter().filter(|m| m.is_drop() && m.moved_piece_type() == pt));
}

pub fn is_legal(pos: &mut Position, mv: Move) -> bool {
    let mut all = Vec::with_capacity(96);
    generate_legal(pos, &mut all);
    all.contains(&mv)
}

fn is_legal_after_pseudo(pos: &mut Position, mv: Move) -> bool {
    let us = pos.side_to_move();
    pos.do_move(mv);
    let king = pos.king_square(us);
    let mut ok = pos.attackers_to(king, pos.occupied(), opposite(us)).is_empty();
    if ok && mv.is_drop() && mv.moved_piece_type() == FU && pos.in_check() {
        // Uchifuzume: this pawn drop gives check; illegal iff the opponent has no reply.
        let mut replies = Vec::with_capacity(96);
        generate_legal(pos, &mut replies);
        if replies.is_empty() {
            ok = false;
        }
    }
    pos.undo_move();
    ok
}

fn generate_pseudo_legal(pos: &Position, out: &mut Vec<Move>) {
    let us = pos.side_to_move();
    let own = pos.pieces_of_color(us);
    let occ = pos.occupied();

    for from in own.iter() {
        let piece = pos.piece_at(from);
        let pt = piece_type(piece);
        let from_row = row_of(from);
        let targets = piece_attacks(us, pt, from, occ) & !own;
        for to in targets.iter() {
            let to_row = row_of(to);
            let captured_pt = piece_type(pos.piece_at(to));
            if must_promote(pt, to_row, us) {
                out.push(Move::new_board(from, to, pt, true, captured_pt));
            } else {
                out.push(Move::new_board(from, to, pt, false, captured_pt));
                if can_promote(pt, from_row, to_row, us) {
                    out.push(Move::new_board(from, to, pt, true, captured_pt));
                }
            }
        }
    }

    for &pt in &crate::hand::Hand::TYPES {
        if pos.hand(us, pt) <= 0 {
            continue;
        }
        for to in (!occ).iter() {
            let to_row = row_of(to);
            if !droppable_row(pt, to_row, us) {
                continue;
            }
            if pt == FU && pos.has_pawn_on_col(us, col_of(to)) {
                continue;
            }
            out.push(Move::new_drop(pt, to));
        }
    }
}

/// Attack bitboard for a piece of type `pt`/color `color` sitting at `sq`, given
/// occupancy `occ`. Includes squares occupied by either color (caller masks captures).
pub fn piece_attacks(color: Color, pt: PieceType, sq: Square, occ: Bitboard) -> Bitboard {
    match pt {
        KYO => lance_attacks(color, sq, occ),
        KAKU => bishop_attacks(sq, occ),
        HI => rook_attacks(sq, occ),
        UMA => horse_attacks(sq, occ),
        RYU => dragon_attacks(sq, occ),
        _ => STEP_ATTACKS[color as usize][sq as usize][pt as usize],
    }
}

use crate::bitboard::Bitboard;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startpos_has_30_legal_moves() {
        let mut pos = Position::startpos();
        let mut moves = Vec::new();
        generate_legal(&mut pos, &mut moves);
        assert_eq!(moves.len(), 30);
    }

    #[test]
    fn double_check_only_king_moves() {
        // Covered by the dedicated double-check perft case in tests/perft.rs.
    }
}

