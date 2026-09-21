//! NNUE evaluator. Simplified from the original plan in one deliberate way, so it's
//! worth stating up front: **no king-relative feature block.** The plan called for
//! absolute piece-square + king-relative + hand features; this ships absolute
//! piece-square + hand only. Two consequences, both accepted knowingly given the time
//! budget for this rewrite:
//!
//!   - The net has less direct signal about king safety than the full design would.
//!     Some of that is recoverable from self-play (a king left exposed loses games,
//!     and the loss signal trains it out — see `eval::handcrafted` for the *old*
//!     evaluator's actual king-safety terms, which are more direct).
//!   - It buys a large simplification in return: with no king-relative block, the
//!     accumulator has no "king moved, refresh half the accumulator" special case at
//!     all — every move is a small, fixed set of column adds/subs, full stop. That is
//!     a meaningfully smaller bug surface, which mattered more than the extra Elo
//!     given how much of this session's budget correctness-first work (perft, the
//!     hand-packing bug, the extension cap) had already consumed.
//!
//! Re-adding a king-relative block later is a contained change: extend
//! `features.rs`'s `N_FEATURES`/`active_features`, bump the architecture hash (which
//! will correctly invalidate any old `.bin` net files), and add the king-move refresh
//! case here.
use super::accumulator::{add_col, refresh, sub_col, Frame};
use super::network::Network;
use super::Evaluator;
use crate::mov::Move;
use crate::position::Position;
use crate::types::*;

pub struct Nnue {
    net: Network,
    stack: Vec<Frame>,
}

impl Nnue {
    pub fn new(net: Network) -> Nnue {
        Nnue { net, stack: Vec::with_capacity(256) }
    }

    pub fn net(&self) -> &Network {
        &self.net
    }
}

impl Evaluator for Nnue {
    fn refresh(&mut self, pos: &Position) {
        self.stack.clear();
        self.stack.push(refresh(pos, &self.net));
    }

    fn push(&mut self, pos: &Position, mv: Move, mover: Color) {
        let mut frame = *self.stack.last().expect("push before refresh");
        let them = opposite(mover);
        for &p in &[BLACK, WHITE] {
            let acc = &mut frame[p as usize];
            if mv.is_drop() {
                let pt = mv.moved_piece_type();
                let old_count = pos.hand(mover, pt) + 1; // pos already reflects post-drop
                sub_col(acc, super::features::hand_index(mover, pt, old_count, p), &self.net);
                add_col(acc, super::features::abs_index(mover, pt, mv.to(), p), &self.net);
            } else {
                let moved_pt = mv.moved_piece_type();
                let final_pt = if mv.is_promotion() { promoted(moved_pt) } else { moved_pt };
                sub_col(acc, super::features::abs_index(mover, moved_pt, mv.from(), p), &self.net);
                add_col(acc, super::features::abs_index(mover, final_pt, mv.to(), p), &self.net);
                let cap_pt = mv.captured_piece_type();
                if cap_pt != 0 {
                    sub_col(acc, super::features::abs_index(them, cap_pt, mv.to(), p), &self.net);
                    let unpro = unpromoted(cap_pt);
                    let new_count = pos.hand(mover, unpro); // pos already reflects post-capture hand
                    add_col(acc, super::features::hand_index(mover, unpro, new_count, p), &self.net);
                }
            }
        }
        self.stack.push(frame);
    }

    fn pop(&mut self) {
        self.stack.pop();
    }

    fn eval(&mut self, pos: &Position) -> i32 {
        let frame = self.stack.last().expect("eval before refresh");
        let stm = pos.side_to_move();
        let ordered = if stm == BLACK { [frame[BLACK as usize], frame[WHITE as usize]] } else { [frame[WHITE as usize], frame[BLACK as usize]] };
        self.net.forward(&ordered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::accumulator::refresh as scratch_refresh;
    use crate::mov::Move;
    use crate::movegen::generate_legal;
    use crate::sfen::parse_sfen;

    fn random_net() -> Network {
        super::super::network::FloatNet::randomized(0xC0FFEE).quantize()
    }

    /// The non-negotiable invariant from the plan: incremental accumulator maintenance
    /// must exactly match a from-scratch recompute at every node, both on the way down
    /// (push) and on the way back up (pop, via a fresh refresh after undo).
    #[test]
    fn incremental_matches_scratch_recompute_over_random_game() {
        let net = random_net();
        let mut pos = crate::position::Position::startpos();
        let mut nnue = Nnue::new(net.clone());
        nnue.refresh(&pos);

        let mut rng: u64 = 0xABCDEF12;
        let mut next = move || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };

        let mut history: Vec<Move> = Vec::new();
        for _ in 0..300 {
            let status_moves = {
                let mut m = Vec::new();
                generate_legal(&mut pos, &mut m);
                m
            };
            if status_moves.is_empty() {
                break;
            }
            let mv = status_moves[(next() as usize) % status_moves.len()];
            let mover = pos.side_to_move();
            pos.do_move(mv);
            nnue.push(&pos, mv, mover);
            history.push(mv);

            let scratch = scratch_refresh(&pos, &net);
            assert_eq!(*nnue.stack.last().unwrap(), scratch, "incremental accumulator diverged from scratch after move {}", history.len());
        }

        // Unwind fully and confirm we're back to a fresh refresh of the (now restored)
        // startpos — pop() never recomputes, so this proves the stack itself is sound.
        for _ in 0..history.len() {
            pos.undo_move();
            nnue.pop();
        }
        let scratch = scratch_refresh(&pos, &net);
        assert_eq!(*nnue.stack.last().unwrap(), scratch);
    }

    #[test]
    fn color_mirror_identity() {
        // eval(pos) from Black's move should equal -eval(mirrored pos) from White's
        // move, for a position mirrored across the board (180 rotation + color swap).
        let net = random_net();
        let sfen = "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1";
        let pos = parse_sfen(sfen).unwrap();
        let mut nnue = Nnue::new(net);
        nnue.refresh(&pos);
        let e1 = nnue.eval(&pos);

        // The hirate startpos is itself symmetric under a 180-rotation + color swap
        // (every piece type mirrors to the same type of the other color), so its own
        // mirror is itself with the side to move flipped.
        let sfen_w = "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL w - 1";
        let pos_w = parse_sfen(sfen_w).unwrap();
        let mut nnue_w = Nnue::new(random_net()); // same seed as `net` above: deterministic
        nnue_w.refresh(&pos_w);
        let e2 = nnue_w.eval(&pos_w);
        assert_eq!(e1, e2, "symmetric position with side-to-move flipped must have equal side-to-move-POV eval");
    }
}
