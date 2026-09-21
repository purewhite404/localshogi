//! Training-data record format shared by `bin/gen.rs` (writer) and `bin/train.rs`
//! (reader). Deliberately a plain 100-byte record (board cells + hand counts, not a
//! bit-packed encoding) — at the data volumes this session's compute budget can
//! actually produce, byte-for-byte compactness isn't the bottleneck, and a plain
//! layout is far less likely to have an off-by-one that silently corrupts labels.
use crate::hand::Hand;
use crate::position::Position;
use crate::types::*;

pub const RECORD_SIZE: usize = 100;

#[derive(Clone, Copy)]
pub struct Record {
    pub board: [u8; 81],
    pub hands: [u8; 14],
    pub side: u8,
    /// Centipawns, from `side`'s point of view (matches `Network::forward`'s convention).
    pub score: i16,
    /// +1/0/-1 from `side`'s point of view.
    pub result: i8,
}

impl Record {
    pub fn from_position(pos: &Position, score: i16, result: i8) -> Record {
        let mut board = [0u8; 81];
        for sq in 0u8..81 {
            board[sq as usize] = cell_byte(pos.piece_at(sq));
        }
        let mut hands = [0u8; 14];
        for (i, &color) in [BLACK, WHITE].iter().enumerate() {
            for (j, &pt) in Hand::TYPES.iter().enumerate() {
                hands[i * 7 + j] = pos.hand(color, pt) as u8;
            }
        }
        Record { board, hands, side: pos.side_to_move(), score, result }
    }

    pub fn to_position(&self) -> Position {
        let mut board = [NO_PIECE; 81];
        for sq in 0..81 {
            board[sq] = piece_from_cell(self.board[sq]);
        }
        let mut hands = [Hand::EMPTY; 2];
        for (i, &color) in [BLACK, WHITE].iter().enumerate() {
            for (j, &pt) in Hand::TYPES.iter().enumerate() {
                for _ in 0..self.hands[i * 7 + j] {
                    hands[color as usize].add(pt);
                }
            }
        }
        Position::from_parts(board, hands, self.side, 1)
    }

    pub fn to_bytes(&self) -> [u8; RECORD_SIZE] {
        let mut out = [0u8; RECORD_SIZE];
        out[0..81].copy_from_slice(&self.board);
        out[81..95].copy_from_slice(&self.hands);
        out[95] = self.side;
        out[96..98].copy_from_slice(&self.score.to_le_bytes());
        out[98] = self.result as u8;
        out
    }

    pub fn from_bytes(b: &[u8]) -> Record {
        let mut board = [0u8; 81];
        board.copy_from_slice(&b[0..81]);
        let mut hands = [0u8; 14];
        hands.copy_from_slice(&b[81..95]);
        let side = b[95];
        let score = i16::from_le_bytes([b[96], b[97]]);
        let result = b[98] as i8;
        Record { board, hands, side, score, result }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_via_position() {
        let pos = Position::startpos();
        let rec = Record::from_position(&pos, 37, 1);
        let bytes = rec.to_bytes();
        let back = Record::from_bytes(&bytes);
        let pos2 = back.to_position();
        assert_eq!(crate::sfen::to_sfen(&pos), crate::sfen::to_sfen(&pos2).replacen(" 1", " 1", 1));
        assert_eq!(back.score, 37);
        assert_eq!(back.result, 1);
    }
}
