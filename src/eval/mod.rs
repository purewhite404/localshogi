//! Pluggable evaluation. `Evaluator` lets the search stay generic over a cheap
//! hand-crafted evaluator and an NNUE one; the search's hot path is monomorphized,
//! so a no-op `push`/`pop` (as in `HandCrafted`) costs nothing at the call site.
use crate::mov::Move;
use crate::position::Position;
use crate::types::Color;

pub mod handcrafted;
#[cfg(feature = "nnue")]
pub mod accumulator;
#[cfg(feature = "nnue")]
pub mod features;
#[cfg(feature = "nnue")]
pub mod network;
#[cfg(feature = "nnue")]
pub mod nnue;
#[cfg(feature = "nnue")]
pub mod simd;

pub use handcrafted::HandCrafted;

/// Evaluates a position from the side-to-move's point of view, in centipawns.
/// `push`/`pop` let a stateful (NNUE) evaluator maintain an incremental accumulator in
/// lockstep with `Position::do_move`/`undo_move`; a stateless evaluator no-ops them.
pub trait Evaluator {
    fn refresh(&mut self, pos: &Position);
    /// Called immediately after `pos.do_move(mv)` has been applied. `mover` is the
    /// color that made the move (i.e. `opposite(pos.side_to_move())` at this point).
    fn push(&mut self, pos: &Position, mv: Move, mover: Color);
    /// Called immediately after `pos.undo_move()` has been applied.
    fn pop(&mut self);
    fn eval(&mut self, pos: &Position) -> i32;
}
