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

pub use handcrafted::HandCrafted;
#[cfg(feature = "nnue")]
pub use nnue::Nnue;

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

/// A runtime choice of evaluator, so callers that need a single concrete type (the
/// wasm boundary, the match harness) don't have to be generic over `Evaluator`.
pub enum AnyEval {
    Hc(HandCrafted),
    #[cfg(feature = "nnue")]
    Nn(Box<Nnue>),
}

impl AnyEval {
    pub fn hand_crafted() -> AnyEval {
        AnyEval::Hc(HandCrafted::new())
    }
    #[cfg(feature = "nnue")]
    pub fn nnue(net: network::Network) -> AnyEval {
        AnyEval::Nn(Box::new(Nnue::new(net)))
    }
    pub fn is_nnue(&self) -> bool {
        match self {
            AnyEval::Hc(_) => false,
            #[cfg(feature = "nnue")]
            AnyEval::Nn(_) => true,
        }
    }
}

impl Evaluator for AnyEval {
    fn refresh(&mut self, pos: &Position) {
        match self {
            AnyEval::Hc(e) => e.refresh(pos),
            #[cfg(feature = "nnue")]
            AnyEval::Nn(e) => e.refresh(pos),
        }
    }
    fn push(&mut self, pos: &Position, mv: Move, mover: Color) {
        match self {
            AnyEval::Hc(e) => e.push(pos, mv, mover),
            #[cfg(feature = "nnue")]
            AnyEval::Nn(e) => e.push(pos, mv, mover),
        }
    }
    fn pop(&mut self) {
        match self {
            AnyEval::Hc(e) => e.pop(),
            #[cfg(feature = "nnue")]
            AnyEval::Nn(e) => e.pop(),
        }
    }
    fn eval(&mut self, pos: &Position) -> i32 {
        match self {
            AnyEval::Hc(e) => e.eval(pos),
            #[cfg(feature = "nnue")]
            AnyEval::Nn(e) => e.eval(pos),
        }
    }
}
