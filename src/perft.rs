//! Perft (performance test / node-count) — the correctness gate for move generation.
use crate::mov::Move;
use crate::movegen::generate_legal;
use crate::position::Position;

pub fn perft(pos: &mut Position, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }
    let mut moves = Vec::with_capacity(96);
    generate_legal(pos, &mut moves);
    if depth == 1 {
        return moves.len() as u64;
    }
    let mut nodes = 0u64;
    for mv in moves {
        pos.do_move(mv);
        nodes += perft(pos, depth - 1);
        pos.undo_move();
    }
    nodes
}

/// Like `perft`, but also asserts `Position::check_consistency` at every node — much
/// slower, used to bisect a perft mismatch rather than for the main test suite.
#[cfg(any(test, feature = "native"))]
pub fn perft_checked(pos: &mut Position, depth: u32) -> u64 {
    pos.check_consistency().unwrap_or_else(|e| panic!("consistency: {e}"));
    if depth == 0 {
        return 1;
    }
    let mut moves = Vec::with_capacity(96);
    generate_legal(pos, &mut moves);
    let mut nodes = 0u64;
    for mv in moves {
        let before = pos.clone();
        let before_key = pos.key();
        pos.do_move(mv);
        nodes += perft_checked(pos, depth - 1);
        pos.undo_move();
        assert!(*pos == before, "do/undo did not round-trip for {mv:?}");
        assert_eq!(pos.key(), before_key, "key did not round-trip for {mv:?}");
    }
    nodes
}

pub fn perft_divide(pos: &mut Position, depth: u32) -> Vec<(Move, u64)> {
    let mut moves = Vec::with_capacity(96);
    generate_legal(pos, &mut moves);
    let mut out = Vec::with_capacity(moves.len());
    for mv in moves {
        pos.do_move(mv);
        let n = if depth <= 1 { 1 } else { perft(pos, depth - 1) };
        out.push((mv, n));
        pos.undo_move();
    }
    out
}
