//! Iterative-deepening alpha-beta search: transposition table, quiescence search,
//! null-move pruning, late-move reductions, futility/delta pruning, mate-distance
//! pruning. Deliberately a single fail-soft alpha-beta tree (no separate PVS
//! re-search framework) — simpler, and adequate at the shallow depths this engine
//! targets; the PV is reconstructed after the fact from the transposition table.
use crate::eval::Evaluator;
use crate::mov::{Move, MOVE_NONE};
use crate::movegen::generate_legal;
use crate::position::{Position, Repetition};
use crate::tt::{mate_in, mated_in, score_from_tt, score_to_tt, Bound, TranspositionTable, MATE, MATE_IN_MAX_PLY};
use crate::types::*;

pub const MAX_PLY: usize = 128;
const DRAW: i32 = 0;
const INF: i32 = MATE + 1000;

pub trait Clock {
    fn now_ms(&self) -> f64;
}

pub struct Limits {
    /// 0 = no fixed-depth limit (use movetime_ms instead).
    pub max_depth: u32,
    /// 0 = no time limit (use max_depth instead).
    pub movetime_ms: u32,
    /// 0 = no node limit (safety net used alongside a time limit).
    pub node_limit: u64,
}

impl Limits {
    pub fn depth(d: u32) -> Limits {
        Limits { max_depth: d, movetime_ms: 0, node_limit: 0 }
    }
    pub fn movetime(ms: u32) -> Limits {
        Limits { max_depth: 0, movetime_ms: ms, node_limit: ms as u64 * 2_000_000 }
    }
}

#[derive(Clone)]
pub struct SearchInfo {
    pub depth: u32,
    pub seldepth: u32,
    pub score_cp: i32,
    pub mate_in: i32,
    pub nodes: u64,
    pub elapsed_ms: u32,
    pub pv: Vec<Move>,
}

#[derive(Clone)]
pub struct SearchResult {
    pub best_move: Move,
    pub score_cp: i32,
    pub mate_in: i32,
    pub depth: u32,
    pub nodes: u64,
    pub elapsed_ms: u32,
    pub aborted: bool,
    pub pv: Vec<Move>,
}

fn mate_in_plies(score: i32) -> i32 {
    if score >= MATE_IN_MAX_PLY {
        MATE - score
    } else if score <= -MATE_IN_MAX_PLY {
        -(MATE + score)
    } else {
        0
    }
}

fn has_non_pawn_material(pos: &Position, color: Color) -> bool {
    let rest = pos.pieces_of_color(color) & !(pos.pieces_of_type(FU) | pos.pieces_of_type(OU));
    if !rest.is_empty() {
        return true;
    }
    [KYO, KEI, GIN, KIN, KAKU, HI].iter().any(|&pt| pos.hand(color, pt) > 0)
}

struct Searcher<'a, E: Evaluator, C: Clock> {
    pos: &'a mut Position,
    tt: &'a mut TranspositionTable,
    eval: &'a mut E,
    clock: &'a C,
    deadline_ms: f64,
    node_limit: u64,
    nodes: u64,
    stopped: bool,
    killers: Vec<[Move; 2]>,
    history: Box<[[i32; 81]; 32]>,
    seldepth: u32,
}

impl<'a, E: Evaluator, C: Clock> Searcher<'a, E, C> {
    fn make_move(&mut self, mv: Move) {
        let mover = self.pos.side_to_move();
        self.pos.do_move(mv);
        self.eval.push(self.pos, mv, mover);
    }

    fn unmake_move(&mut self) {
        self.pos.undo_move();
        self.eval.pop();
    }

    fn check_time(&mut self) {
        if self.deadline_ms.is_finite() && self.clock.now_ms() >= self.deadline_ms {
            self.stopped = true;
        }
        if self.node_limit > 0 && self.nodes >= self.node_limit {
            self.stopped = true;
        }
    }

    fn score_capture(&self, mv: Move) -> i32 {
        100_000 + capture_gain(mv.captured_piece_type()) * 100 - VALUES[mv.moved_piece_type() as usize]
    }

    fn score_move(&self, mv: Move, tt_move: Move, killers: [Move; 2]) -> i32 {
        if mv == tt_move {
            return 2_000_000;
        }
        if mv.is_capture() {
            return 1_000_000 + self.score_capture(mv);
        }
        if mv.is_promotion() {
            return 60_000;
        }
        if mv == killers[0] {
            return 50_000;
        }
        if mv == killers[1] {
            return 49_000;
        }
        let piece = make_piece(self.pos.side_to_move(), mv.moved_piece_type());
        self.history[piece as usize][mv.to() as usize]
    }

    fn update_killer(&mut self, ply: u32, mv: Move) {
        let k = &mut self.killers[ply as usize];
        if k[0] != mv {
            k[1] = k[0];
            k[0] = mv;
        }
    }

    fn update_history(&mut self, mv: Move, depth: i32) {
        let piece = make_piece(self.pos.side_to_move(), mv.moved_piece_type());
        let slot = &mut self.history[piece as usize][mv.to() as usize];
        *slot = (*slot + depth * depth).min(1_000_000);
    }

    fn qsearch(&mut self, ply: u32, mut alpha: i32, beta: i32, qply: i32) -> i32 {
        self.nodes += 1;
        if self.nodes & 1023 == 0 {
            self.check_time();
        }
        if self.stopped {
            return 0;
        }
        self.seldepth = self.seldepth.max(ply);
        if ply as usize >= MAX_PLY || qply >= 8 {
            return self.eval.eval(self.pos);
        }

        let in_check = self.pos.in_check();
        let mut best;
        if !in_check {
            let stand_pat = self.eval.eval(self.pos);
            if stand_pat >= beta {
                return stand_pat;
            }
            if stand_pat > alpha {
                alpha = stand_pat;
            }
            best = stand_pat;
        } else {
            best = mated_in(ply);
        }

        let mut moves = Vec::with_capacity(48);
        generate_legal(self.pos, &mut moves);
        if in_check && moves.is_empty() {
            return mated_in(ply);
        }
        let mut candidates: Vec<Move> = if in_check {
            moves
        } else {
            moves.into_iter().filter(|m| m.is_capture() || (m.is_promotion() && matches!(m.moved_piece_type(), FU | KAKU | HI))).collect()
        };
        candidates.sort_by_key(|&mv| -self.score_capture(mv));

        for mv in candidates {
            if !in_check && mv.is_capture() {
                let gain = capture_gain(mv.captured_piece_type());
                if best + gain + 200 <= alpha {
                    continue;
                }
            }
            self.make_move(mv);
            let score = -self.qsearch(ply + 1, -beta, -alpha, qply + 1);
            self.unmake_move();
            if self.stopped {
                return 0;
            }
            if score > best {
                best = score;
                if score > alpha {
                    alpha = score;
                }
            }
            if alpha >= beta {
                break;
            }
        }
        best
    }

    fn negamax(&mut self, depth: i32, ply: u32, mut alpha: i32, mut beta: i32) -> i32 {
        self.nodes += 1;
        if self.nodes & 1023 == 0 {
            self.check_time();
        }
        if self.stopped {
            return 0;
        }
        self.seldepth = self.seldepth.max(ply);

        if ply > 0 {
            match self.pos.detect_repetition() {
                Repetition::Draw => return DRAW,
                Repetition::WinByPerpetual => return mate_in(ply),
                Repetition::LoseByPerpetual => return mated_in(ply),
                Repetition::None => {}
            }
            if ply as usize >= MAX_PLY {
                return self.eval.eval(self.pos);
            }
            alpha = alpha.max(mated_in(ply));
            beta = beta.min(mate_in(ply + 1));
            if alpha >= beta {
                return alpha;
            }
        }

        if depth <= 0 {
            return self.qsearch(ply, alpha, beta, 0);
        }

        let key = self.pos.key();
        let tt_entry = self.tt.probe(key);
        let mut tt_move = MOVE_NONE;
        if let Some(e) = tt_entry {
            tt_move = e.mv;
            if e.depth >= depth {
                let score = score_from_tt(e.score, ply);
                match e.bound {
                    Bound::Exact => return score,
                    Bound::Lower if score >= beta => return score,
                    Bound::Upper if score <= alpha => return score,
                    _ => {}
                }
            }
        }

        let in_check = self.pos.in_check();
        let static_eval = if in_check { -MATE } else { self.eval.eval(self.pos) };

        if !in_check && depth >= 3 && ply > 0 && beta < MATE_IN_MAX_PLY && has_non_pawn_material(self.pos, self.pos.side_to_move()) {
            let r = 3 + depth / 6;
            self.pos.do_null_move();
            let score = -self.negamax(depth - 1 - r, ply + 1, -beta, -beta + 1);
            self.pos.undo_null_move();
            if self.stopped {
                return 0;
            }
            if score >= beta {
                return if score >= MATE_IN_MAX_PLY { beta } else { score };
            }
        }

        let mut moves = Vec::with_capacity(96);
        generate_legal(self.pos, &mut moves);
        if moves.is_empty() {
            return mated_in(ply);
        }

        let killers = self.killers[ply as usize];
        let mut scored: Vec<(Move, i32)> = moves.iter().map(|&mv| (mv, self.score_move(mv, tt_move, killers))).collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1));

        let orig_alpha = alpha;
        let mut best_score = -INF;
        let mut best_move = MOVE_NONE;

        for (i, &(mv, _)) in scored.iter().enumerate() {
            let is_capture = mv.is_capture();
            let is_promo = mv.is_promotion();
            let quiet = !is_capture && !is_promo;

            if !in_check && depth <= 2 && quiet && i > 0 {
                let margin = 200 * depth;
                if static_eval + margin <= alpha {
                    continue;
                }
            }

            self.make_move(mv);
            let gives_check = self.pos.in_check();
            let mut new_depth = depth - 1;
            if gives_check && depth < 16 {
                new_depth += 1;
            }

            let score = if i >= 4 && depth >= 3 && quiet && !gives_check {
                let reduced = (new_depth - 1).max(0);
                let s = -self.negamax(reduced, ply + 1, -alpha - 1, -alpha);
                if s > alpha && !self.stopped {
                    -self.negamax(new_depth, ply + 1, -beta, -alpha)
                } else {
                    s
                }
            } else {
                -self.negamax(new_depth, ply + 1, -beta, -alpha)
            };

            self.unmake_move();

            if self.stopped {
                return 0;
            }

            if score > best_score {
                best_score = score;
                best_move = mv;
            }
            if score > alpha {
                alpha = score;
                if quiet {
                    self.update_killer(ply, mv);
                    self.update_history(mv, depth);
                }
            }
            if alpha >= beta {
                break;
            }
        }

        let bound = if best_score <= orig_alpha {
            Bound::Upper
        } else if best_score >= beta {
            Bound::Lower
        } else {
            Bound::Exact
        };
        self.tt.store(key, best_move, score_to_tt(best_score, ply), static_eval, depth, bound);
        best_score
    }

    fn extract_pv(&mut self, max_len: u32) -> Vec<Move> {
        let mut pv = Vec::new();
        let mut seen = std::collections::HashSet::new();
        while (pv.len() as u32) < max_len {
            let key = self.pos.key();
            if !seen.insert(key) {
                break;
            }
            let entry = match self.tt.probe(key) {
                Some(e) => e,
                None => break,
            };
            if entry.mv.is_none() {
                break;
            }
            let mut legal = Vec::with_capacity(96);
            generate_legal(self.pos, &mut legal);
            if !legal.contains(&entry.mv) {
                break;
            }
            self.pos.do_move(entry.mv);
            pv.push(entry.mv);
        }
        for _ in 0..pv.len() {
            self.pos.undo_move();
        }
        pv
    }
}

pub fn search<E: Evaluator, C: Clock>(
    pos: &mut Position,
    eval: &mut E,
    tt: &mut TranspositionTable,
    limits: Limits,
    clock: &C,
    mut info_cb: Option<&mut dyn FnMut(&SearchInfo)>,
) -> SearchResult {
    eval.refresh(pos);
    let start = clock.now_ms();
    let deadline = if limits.movetime_ms > 0 { start + limits.movetime_ms as f64 } else { f64::INFINITY };
    let max_depth = if limits.max_depth > 0 { limits.max_depth } else { MAX_PLY as u32 };

    let mut s = Searcher {
        pos,
        tt,
        eval,
        clock,
        deadline_ms: deadline,
        node_limit: limits.node_limit,
        nodes: 0,
        stopped: false,
        killers: vec![[MOVE_NONE; 2]; MAX_PLY],
        history: Box::new([[0i32; 81]; 32]),
        seldepth: 0,
    };

    let mut best = SearchResult { best_move: MOVE_NONE, score_cp: 0, mate_in: 0, depth: 0, nodes: 0, elapsed_ms: 0, aborted: false, pv: vec![] };
    // Fallback in case even depth 1 gets aborted immediately: at least return a legal move.
    let mut fallback_moves = Vec::with_capacity(64);
    generate_legal(s.pos, &mut fallback_moves);
    if let Some(&mv) = fallback_moves.first() {
        best.best_move = mv;
    }

    for depth in 1..=max_depth {
        s.seldepth = 0;
        let score = s.negamax(depth as i32, 0, -INF, INF);
        if s.stopped && depth > 1 {
            best.aborted = true;
            break;
        }
        let pv = s.extract_pv(depth.max(1));
        let elapsed = (s.clock.now_ms() - start) as u32;
        let bm = pv.first().copied().unwrap_or(best.best_move);
        best = SearchResult { best_move: bm, score_cp: score, mate_in: mate_in_plies(score), depth, nodes: s.nodes, elapsed_ms: elapsed, aborted: s.stopped, pv: pv.clone() };
        if let Some(cb) = info_cb.as_deref_mut() {
            cb(&SearchInfo { depth, seldepth: s.seldepth, score_cp: score, mate_in: best.mate_in, nodes: s.nodes, elapsed_ms: elapsed, pv });
        }
        if s.stopped {
            break;
        }
        if best.mate_in != 0 {
            break;
        }
        if limits.movetime_ms > 0 {
            let now = s.clock.now_ms();
            let used = now - start;
            let remaining = deadline - now;
            if remaining < used * 0.5 {
                break;
            }
        }
    }
    best
}
