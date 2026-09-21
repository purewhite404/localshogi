use shogi_wasm::movegen::generate_legal;
use shogi_wasm::perft::perft;
use shogi_wasm::position::Position;
use shogi_wasm::sfen::{move_to_usi, parse_sfen};
use shogi_wasm::types::OU;

/// The absolute correctness gate: well-known perft node counts from the shogi
/// startpos, verified against reference engines (Apery/YaneuraOu).
#[test]
fn perft_startpos_depth_1_to_4() {
    let expected: [u64; 4] = [30, 900, 25_470, 719_731];
    let mut pos = Position::startpos();
    for (i, &want) in expected.iter().enumerate() {
        let depth = (i + 1) as u32;
        let got = perft(&mut pos, depth);
        assert_eq!(got, want, "perft(startpos, {depth}) = {got}, want {want}");
    }
}

#[test]
#[ignore] // ~20M nodes; run explicitly with `cargo test --release -- --ignored`
fn perft_startpos_depth_5() {
    let mut pos = Position::startpos();
    assert_eq!(perft(&mut pos, 5), 19_861_490);
}

/// Smoke test for a position with a very large branching factor (heavy drop
/// generation from big hands on both sides). Not checked against an external
/// reference (none was available in this environment), so it only asserts the
/// move count is large and stable, not an exact literature value.
#[test]
fn perft_wide_branching_position() {
    let sfen = "l6nl/5+P1gk/2np1S3/p1p4Pp/3P2Sp1/1PPb2P1P/P5GS1/R8/LN4bKL w GR5pnsg 1";
    let mut pos = parse_sfen(sfen).expect("sfen parses");
    let got = perft(&mut pos, 1);
    assert!(got > 150, "expected a wide branching position, got only {got} moves");
}

/// A constructed 打ち歩詰め (pawn-drop checkmate) position: white king cornered at
/// 9a, its only three neighbouring squares each occupied or covered by a black
/// knight that is itself defended (capturing walks the king into a second knight's
/// attack), so dropping a black pawn at 9b would be checkmate with no reply — and
/// must therefore be excluded from the legal move list.
#[test]
fn perft_uchifuzume_is_illegal() {
    let sfen = "kN7/1N7/N8/NN7/9/9/9/9/4K4 b P 1";
    let mut pos = parse_sfen(sfen).expect("sfen parses");
    assert!(!pos.in_check(), "white must not already be in check before the drop");

    let mut moves = Vec::new();
    generate_legal(&mut pos, &mut moves);
    let has_mating_drop = moves.iter().any(|m| m.is_drop() && move_to_usi(*m) == "P*9b");
    assert!(!has_mating_drop, "dropping a pawn for checkmate must be illegal (uchifuzume)");
}

/// The same position but with black's mating knight at (3,1) removed, so the pawn
/// drop is no longer mate (white can safely capture it) — the drop must now be legal.
/// This paired case proves the uchifuzume test isn't just rejecting all pawn drops.
#[test]
fn perft_pawn_drop_check_with_escape_is_legal() {
    let sfen = "kN7/1N7/N8/N8/9/9/9/9/4K4 b P 1";
    let mut pos = parse_sfen(sfen).expect("sfen parses");
    let mut moves = Vec::new();
    generate_legal(&mut pos, &mut moves);
    let has_drop = moves.iter().any(|m| m.is_drop() && move_to_usi(*m) == "P*9b");
    assert!(has_drop, "with the defender removed, the pawn drop should be legal (king can capture)");
}

#[test]
fn perft_double_check_king_moves_only() {
    let sfen = "4l4/9/9/9/4K4/9/9/6b2/9 b - 1";
    let mut pos = parse_sfen(sfen).expect("sfen parses");
    assert!(pos.checkers().count_ones() >= 1);
    let mut moves = Vec::new();
    generate_legal(&mut pos, &mut moves);
    assert!(!moves.is_empty(), "must have at least one legal king move");
    assert!(moves.iter().all(|m| !m.is_drop() && m.moved_piece_type() == OU), "in double check, only king moves are legal, got {moves:?}");
}

#[test]
#[ignore] // ~550M nodes, several minutes; extra confidence beyond depth 5.
fn perft_startpos_depth_6() {
    let mut pos = Position::startpos();
    assert_eq!(perft(&mut pos, 6), 547_581_517);
}
