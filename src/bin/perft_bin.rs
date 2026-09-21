//! Native perft/divide CLI, useful for bisecting a movegen regression by hand.
use clap::Parser;
use shogi_wasm::perft::{perft, perft_divide};
use shogi_wasm::position::Position;
use shogi_wasm::sfen::{move_to_usi, parse_sfen};

#[derive(Parser)]
struct Args {
    /// Search depth.
    #[arg(short, long, default_value_t = 5)]
    depth: u32,
    /// Starting SFEN (defaults to the hirate startpos).
    #[arg(short, long)]
    sfen: Option<String>,
    /// Comma-separated USI move prefix to apply before counting/dividing.
    #[arg(short, long, default_value = "")]
    moves: String,
    /// Print a per-root-move breakdown instead of just the total.
    #[arg(long)]
    divide: bool,
}

fn main() {
    let args = Args::parse();
    let mut pos = match &args.sfen {
        Some(s) => parse_sfen(s).expect("bad sfen"),
        None => Position::startpos(),
    };
    for mv_str in args.moves.split(',').filter(|s| !s.is_empty()) {
        let mut legal = Vec::new();
        shogi_wasm::movegen::generate_legal(&mut pos, &mut legal);
        let mv = legal.into_iter().find(|m| move_to_usi(*m) == mv_str).unwrap_or_else(|| panic!("prefix move {mv_str} not legal"));
        pos.do_move(mv);
    }

    if args.divide {
        let mut total = 0u64;
        let mut rows: Vec<(String, u64)> = perft_divide(&mut pos, args.depth).into_iter().map(|(m, n)| (move_to_usi(m), n)).collect();
        rows.sort();
        for (mv, n) in &rows {
            println!("{mv} {n}");
            total += n;
        }
        println!("total {total}");
    } else {
        let start = std::time::Instant::now();
        let nodes = perft(&mut pos, args.depth);
        let secs = start.elapsed().as_secs_f64();
        println!("perft({}) = {nodes}  ({:.0} nps, {:.2}s)", args.depth, nodes as f64 / secs.max(1e-9), secs);
    }
}
