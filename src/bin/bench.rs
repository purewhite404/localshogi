//! Fixed-position search benchmark: reports nodes/sec for the current search+eval, the
//! number this project's throughput estimates (data-gen rate, expected reachable
//! depth) are grounded in. Run after any change to search.rs or the evaluator.
use clap::Parser;
use shogi_wasm::eval::HandCrafted;
use shogi_wasm::native_support::StdClock;
use shogi_wasm::search::{search, Limits};
use shogi_wasm::sfen::parse_sfen;
use shogi_wasm::tt::TranspositionTable;

#[derive(Parser)]
struct Args {
    #[arg(short, long, default_value_t = 5)]
    depth: u32,
    #[arg(long, default_value_t = 64)]
    tt_mb: usize,
    /// Fail (nonzero exit) if the aggregate nps drops below this. Used as a CI
    /// regression guard, not as a strength claim.
    #[arg(long)]
    min_nps: Option<f64>,
}

// Deliberately ordinary middlegame-ish positions. A position with both hands full and
// an open board (e.g. the classic 593-legal-move position) is combinatorially far
// harder for plain alpha-beta and belongs in a separate, explicitly-opted-into stress
// benchmark, not the routine one this file's default depth is tuned against.
const POSITIONS: &[&str] = &[
    "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
    "lnsgkgsnl/1r5b1/ppppppppp/9/9/2P6/PP1PPPPPP/1B5R1/LNSGKGSNL w - 2",
    "lnsg1gsnl/1r2k2b1/pppppp1pp/6p2/9/2P6/PP1PPPPPP/1B3S1R1/LNSGKG1NL b - 5",
];

fn main() {
    let args = Args::parse();
    let mut total_nodes = 0u64;
    let mut total_ms = 0f64;
    for sfen in POSITIONS {
        let mut pos = parse_sfen(sfen).expect("bad sfen");
        let mut eval = HandCrafted::new();
        let mut tt = TranspositionTable::new(args.tt_mb);
        let clock = StdClock::start();
        let result = search(&mut pos, &mut eval, &mut tt, Limits::depth(args.depth), &clock, None);
        let nps = result.nodes as f64 / (result.elapsed_ms as f64 / 1000.0).max(1e-3);
        println!(
            "{sfen}\n  depth={} nodes={} time={}ms nps={:.0} score={} best={}",
            result.depth,
            result.nodes,
            result.elapsed_ms,
            nps,
            result.score_cp,
            shogi_wasm::sfen::move_to_usi(result.best_move)
        );
        total_nodes += result.nodes;
        total_ms += result.elapsed_ms as f64;
    }
    let agg_nps = total_nodes as f64 / (total_ms / 1000.0).max(1e-3);
    println!("\naggregate: {total_nodes} nodes in {total_ms:.0}ms = {agg_nps:.0} nps");
    if let Some(min) = args.min_nps {
        if agg_nps < min {
            eprintln!("FAIL: {agg_nps:.0} nps < required minimum {min:.0}");
            std::process::exit(1);
        }
    }
}
