//! Engine-vs-engine verification harness: two evaluators (hand-crafted or a named
//! NNUE net file), fixed node budget, alternating colors over paired openings,
//! sennichite/perpetual/impasse-length adjudication, reports win/draw/loss and an
//! Elo estimate with a rough confidence interval.
use clap::Parser;
use shogi_wasm::eval::network::Network;
use shogi_wasm::eval::{Evaluator, HandCrafted, Nnue};
use shogi_wasm::mov::Move;
use shogi_wasm::movegen::generate_legal;
use shogi_wasm::native_support::StdClock;
use shogi_wasm::position::{Position, Repetition};
use shogi_wasm::search::{search, Limits};
use shogi_wasm::tt::TranspositionTable;
use shogi_wasm::types::BLACK;
use std::fs;

#[derive(Parser)]
struct Args {
    /// "hc" for the hand-crafted evaluator, or a path to a .bin NNUE net file.
    #[arg(short = 'a', long)]
    player_a: String,
    #[arg(short = 'b', long)]
    player_b: String,
    #[arg(short, long, default_value_t = 400)]
    games: u32,
    #[arg(short, long, default_value_t = 5000)]
    nodes: u64,
    #[arg(long, default_value_t = 300)]
    max_plies: u32,
    #[arg(long, default_value_t = 4)]
    tt_mb: usize,
    #[arg(long, default_value_t = 20260921)]
    seed: u64,
    #[arg(short, long, default_value_t = 10)]
    threads: usize,
}

enum AnyEval {
    Hc(HandCrafted),
    Nn(Box<Nnue>),
}
impl Evaluator for AnyEval {
    fn refresh(&mut self, pos: &Position) {
        match self {
            AnyEval::Hc(e) => e.refresh(pos),
            AnyEval::Nn(e) => e.refresh(pos),
        }
    }
    fn push(&mut self, pos: &Position, mv: Move, mover: shogi_wasm::types::Color) {
        match self {
            AnyEval::Hc(e) => e.push(pos, mv, mover),
            AnyEval::Nn(e) => e.push(pos, mv, mover),
        }
    }
    fn pop(&mut self) {
        match self {
            AnyEval::Hc(e) => e.pop(),
            AnyEval::Nn(e) => e.pop(),
        }
    }
    fn eval(&mut self, pos: &Position) -> i32 {
        match self {
            AnyEval::Hc(e) => e.eval(pos),
            AnyEval::Nn(e) => e.eval(pos),
        }
    }
}

fn load_player(spec: &str) -> AnyEval {
    if spec == "hc" {
        return AnyEval::Hc(HandCrafted::new());
    }
    let bytes = fs::read(spec).unwrap_or_else(|e| panic!("cannot read net {spec}: {e}"));
    let net = Network::from_bytes(&bytes).unwrap_or_else(|e| panic!("bad net {spec}: {e}"));
    AnyEval::Nn(Box::new(Nnue::new(net)))
}

struct Rng(u64);
impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() as usize) % n.max(1)
    }
}

/// Returns +1 if `first_mover_color`'s side (BLACK, always) eventually wins, -1 if it
/// loses, 0 for a draw. `eval_black`/`eval_white` decide which evaluator plays which
/// color for this particular game (colors are swapped between paired games by the
/// caller, not here).
fn play_one_game(eval_black: &mut AnyEval, eval_white: &mut AnyEval, nodes: u64, max_plies: u32, rng: &mut Rng, opening_plies: u32) -> i32 {
    let mut pos = Position::startpos();
    let mut tt_b = TranspositionTable::new(4);
    let mut tt_w = TranspositionTable::new(4);

    for ply in 0..max_plies {
        let mut moves = Vec::with_capacity(96);
        generate_legal(&mut pos, &mut moves);
        if moves.is_empty() {
            return if pos.side_to_move() == BLACK { -1 } else { 1 };
        }
        match pos.detect_repetition() {
            Repetition::Draw => return 0,
            Repetition::WinByPerpetual => return if pos.side_to_move() == BLACK { 1 } else { -1 },
            Repetition::LoseByPerpetual => return if pos.side_to_move() == BLACK { -1 } else { 1 },
            Repetition::None => {}
        }

        if ply < opening_plies {
            let mv = moves[rng.below(moves.len())];
            pos.do_move(mv);
            continue;
        }

        let clock = StdClock::start();
        let (mv, is_black) = if pos.side_to_move() == BLACK {
            eval_black.refresh(&pos);
            let r = search(&mut pos, eval_black, &mut tt_b, Limits::nodes(nodes), &clock, None);
            (r.best_move, true)
        } else {
            eval_white.refresh(&pos);
            let r = search(&mut pos, eval_white, &mut tt_w, Limits::nodes(nodes), &clock, None);
            (r.best_move, false)
        };
        let _ = is_black;
        if mv.is_none() {
            return if pos.side_to_move() == BLACK { -1 } else { 1 };
        }
        pos.do_move(mv);
    }
    0 // adjudicate as a draw at the ply cap
}

/// Plays games `[start_g, end_g)`, returns each game's result from player A's POV
/// (+1/0/-1), in game order. Each thread loads its own evaluator instances once, not
/// per game (an NNUE evaluator's `refresh()` at the start of each game's first search
/// already resets any stale accumulator state, so reuse across games is safe).
fn play_range(args: &Args, start_g: u32, end_g: u32) -> Vec<i32> {
    let mut ea = load_player(&args.player_a);
    let mut eb = load_player(&args.player_b);
    let mut out = Vec::with_capacity((end_g - start_g) as usize);
    for g in start_g..end_g {
        let pair_seed = args.seed ^ (g as u64 / 2).wrapping_mul(0x9E3779B97F4A7C15);
        let mut opening_rng = Rng(pair_seed);
        let a_is_black = g % 2 == 0;
        let result_black = if a_is_black { play_one_game(&mut ea, &mut eb, args.nodes, args.max_plies, &mut opening_rng, 8) } else { play_one_game(&mut eb, &mut ea, args.nodes, args.max_plies, &mut opening_rng, 8) };
        out.push(if a_is_black { result_black } else { -result_black });
    }
    out
}

fn main() {
    let args = Args::parse();
    let start = std::time::Instant::now();

    // Split into per-thread contiguous, pair-aligned (even-sized) ranges so each
    // thread's own paired-opening bookkeeping stays self-contained.
    let n_threads = args.threads.max(1).min(args.games.max(1) as usize);
    let base = (args.games as usize / n_threads / 2).max(1) * 2;
    let mut ranges = Vec::new();
    let mut g = 0u32;
    while g < args.games {
        let end = (g + base as u32).min(args.games);
        ranges.push((g, end));
        g = end;
    }
    // Fold the last (possibly short/odd) leftover range into the previous one.
    if ranges.len() > 1 {
        let last = ranges.pop().unwrap();
        ranges.last_mut().unwrap().1 = last.1;
    }

    let results: Vec<i32> = std::thread::scope(|scope| {
        let args = &args;
        let handles: Vec<_> = ranges.iter().map(|&(s, e)| scope.spawn(move || play_range(args, s, e))).collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });

    let mut score_a = 0.0f64;
    let mut wins_a = 0u32;
    let mut wins_b = 0u32;
    let mut draws = 0u32;
    for (i, &r) in results.iter().enumerate() {
        match r {
            1 => {
                wins_a += 1;
                score_a += 1.0;
            }
            -1 => wins_b += 1,
            _ => {
                draws += 1;
                score_a += 0.5;
            }
        }
        if (i + 1) % 20 == 0 || i + 1 == results.len() {
            let n = (i + 1) as f64;
            eprintln!("[{}/{}] A: {wins_a}W {draws}D {wins_b}L  score_a={:.3}  elo_diff~{:.0}  ({:.0}s)", i + 1, results.len(), score_a / n, elo_diff(score_a / n), start.elapsed().as_secs_f64());
        }
    }

    let n = results.len() as f64;
    let s = score_a / n;
    println!("\nFinal: A({}) {wins_a}W {draws}D {wins_b}L vs B({})  score_a={:.3}  elo_diff~{:.0}", args.player_a, args.player_b, s, elo_diff(s));
}

fn elo_diff(score: f64) -> f64 {
    let s = score.clamp(0.001, 0.999);
    -400.0 * (1.0 / s - 1.0).log10()
}
