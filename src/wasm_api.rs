//! The wasm-bindgen boundary: a persistent `Engine` object so the transposition table,
//! killer/history tables, and game history (for sennichite) all survive across moves.
//! Everything hot (rule queries called on every click) uses packed integers and typed
//! arrays, never JSON — see the packed `Move` encoding in `mov.rs`.
use wasm_bindgen::prelude::*;

use crate::eval::AnyEval;
use crate::mov::Move;
use crate::movegen::generate_legal;
use crate::position::{Position, Repetition};
use crate::search::{search as run_search, Clock, Limits};
use crate::sfen::{move_to_usi, parse_sfen, parse_usi_move, to_sfen};
use crate::tt::TranspositionTable;
use crate::types::*;

#[cfg(feature = "nnue")]
static EMBEDDED_NET: &[u8] = include_bytes!("../nets/current.bin");

/// The default evaluator. As of the NNUE net currently embedded, this is the
/// hand-crafted evaluator: verified by `match_bin` against the trained net (~69K
/// self-play positions, no king-relative features — see `eval/nnue.rs`'s doc comment)
/// and the hand-crafted evaluator won convincingly. The NNUE net is still shipped and
/// selectable via `set_evaluator("nnue")` / the UI's 評価関数 toggle — re-training on
/// more self-play data is the natural next step, and swapping the default back is a
/// one-line change here once a net actually wins the match.
fn default_eval() -> AnyEval {
    AnyEval::hand_crafted()
}

/// The embedded NNUE net if it parses and its architecture hash matches this build,
/// else silently falls back to the hand-crafted evaluator — a stale/corrupt net file
/// must never turn into a blank page.
#[cfg(feature = "nnue")]
fn nnue_eval() -> AnyEval {
    match crate::eval::network::Network::from_bytes(EMBEDDED_NET) {
        Ok(net) => AnyEval::nnue(net),
        Err(e) => {
            #[cfg(target_arch = "wasm32")]
            web_sys_console_warn(&format!("embedded NNUE net failed to load, falling back to the hand-crafted evaluator: {e}"));
            #[cfg(not(target_arch = "wasm32"))]
            eprintln!("embedded NNUE net failed to load: {e}");
            AnyEval::hand_crafted()
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn web_sys_console_warn(s: &str) {
    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = console)]
        fn warn(s: &str);
    }
    warn(s);
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = performance, js_name = now)]
    fn perf_now() -> f64;
}

struct WasmClock;
impl Clock for WasmClock {
    fn now_ms(&self) -> f64 {
        perf_now()
    }
}

/// Simplified Japanese move notation: piece name + destination + 成/打. Omits the
/// 同 (same-square-as-last-move) contraction and the 上/引/寄/直 disambiguation a full
/// KIF renderer would add when two like pieces could reach the same square — a
/// deliberate v1 scope cut; USI text (`move_to_usi`) is always available as a fallback.
fn kif_text(pos_before: &Position, mv: Move, mover: Color) -> String {
    const PNAMES: [&str; 15] = ["", "歩", "香", "桂", "銀", "金", "角", "飛", "玉", "と", "杏", "圭", "全", "馬", "龍"];
    let side_mark = if mover == BLACK { "▲" } else { "△" };
    let file = 9 - col_of(mv.to());
    let rank = ["一", "二", "三", "四", "五", "六", "七", "八", "九"][row_of(mv.to()) as usize];
    let pt = if mv.is_drop() { mv.moved_piece_type() } else if mv.is_promotion() { promoted(mv.moved_piece_type()) } else { mv.moved_piece_type() };
    let _ = pos_before;
    let suffix = if mv.is_drop() { "打" } else if mv.is_promotion() { "成" } else { "" };
    format!("{side_mark}{file}{rank}{}{suffix}", PNAMES[pt as usize])
}

#[wasm_bindgen]
pub struct Engine {
    pos: Position,
    tt: TranspositionTable,
    eval: AnyEval,
    last_result: Option<crate::search::SearchResult>,
}

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new(tt_mb: u32) -> Engine {
        Engine { pos: Position::startpos(), tt: TranspositionTable::new(tt_mb.max(1) as usize), eval: default_eval(), last_result: None }
    }

    /// "nnue" or "hc" (hand-crafted, the default — see `default_eval`'s doc comment).
    /// Falls back to hand-crafted if NNUE was requested but the embedded net failed to
    /// load. Exposed so the UI can offer an A/B toggle.
    pub fn set_evaluator(&mut self, name: &str) {
        #[cfg(feature = "nnue")]
        {
            self.eval = if name == "nnue" { nnue_eval() } else { AnyEval::hand_crafted() };
        }
        #[cfg(not(feature = "nnue"))]
        {
            let _ = name;
            self.eval = AnyEval::hand_crafted();
        }
    }

    pub fn evaluator_name(&self) -> String {
        if self.eval.is_nnue() { "nnue".into() } else { "hc".into() }
    }

    pub fn reset(&mut self) {
        self.pos = Position::startpos();
        self.tt.clear();
        self.last_result = None;
    }

    pub fn set_sfen(&mut self, sfen: &str) -> Result<(), JsValue> {
        self.pos = parse_sfen(sfen).map_err(|e| JsValue::from_str(&e))?;
        self.tt.clear();
        self.last_result = None;
        Ok(())
    }

    pub fn sfen(&self) -> String {
        to_sfen(&self.pos)
    }

    pub fn side_to_move(&self) -> u8 {
        self.pos.side_to_move()
    }

    pub fn ply(&self) -> u32 {
        self.pos.ply()
    }

    /// Hex string (not a JS number/BigInt) to avoid precision loss on a 64-bit key.
    pub fn key_hex(&self) -> String {
        format!("{:016x}", self.pos.key())
    }

    /// 81 bytes, index = row*9? no: our square = col*9+row, so index IS the square
    /// number directly; cell = 0 empty, else piece_type | (0x80 if gote).
    pub fn board(&self) -> Vec<u8> {
        (0u8..81).map(|sq| cell_byte(self.pos.piece_at(sq))).collect()
    }

    /// 14 bytes: [sente FU,KYO,KEI,GIN,KIN,KAKU,HI] then the same order for gote.
    pub fn hands(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(14);
        for &color in &[BLACK, WHITE] {
            for &pt in &crate::hand::Hand::TYPES {
                out.push(self.pos.hand(color, pt) as u8);
            }
        }
        out
    }

    pub fn last_move(&self) -> u32 {
        self.pos.last_move().0
    }

    pub fn legal_moves(&mut self) -> Vec<u32> {
        let mut moves = Vec::with_capacity(96);
        generate_legal(&mut self.pos, &mut moves);
        moves.into_iter().map(|m| m.0).collect()
    }

    pub fn moves_from(&mut self, from: u8) -> Vec<u32> {
        let mut all = Vec::with_capacity(96);
        generate_legal(&mut self.pos, &mut all);
        all.into_iter().filter(|m| !m.is_drop() && m.from() == from).map(|m| m.0).collect()
    }

    pub fn drops_of(&mut self, piece_type: u8) -> Vec<u32> {
        let mut all = Vec::with_capacity(96);
        generate_legal(&mut self.pos, &mut all);
        all.into_iter().filter(|m| m.is_drop() && m.moved_piece_type() == piece_type).map(|m| m.0).collect()
    }

    /// bit0 = a non-promoting move to `to` is legal, bit1 = a promoting move is legal.
    /// 0 = illegal entirely, 3 = both legal (UI should ask 成る/成らず).
    pub fn move_options(&mut self, from: u8, to: u8) -> u8 {
        let mut all = Vec::with_capacity(96);
        generate_legal(&mut self.pos, &mut all);
        let mut bits = 0u8;
        for m in all {
            if !m.is_drop() && m.from() == from && m.to() == to {
                bits |= if m.is_promotion() { 2 } else { 1 };
            }
        }
        bits
    }

    pub fn drop_option(&mut self, piece_type: u8, to: u8) -> bool {
        let mut all = Vec::with_capacity(96);
        generate_legal(&mut self.pos, &mut all);
        all.into_iter().any(|m| m.is_drop() && m.moved_piece_type() == piece_type && m.to() == to)
    }

    /// Best-effort explanation for why a move a user clicked isn't in the legal list.
    /// 0 = (shouldn't be called for a legal move) 1 = 二歩 2 = 打ち歩詰め
    /// 3 = 行き所のない駒 4 = 自玉が王手になる／その他不正
    pub fn illegal_reason(&self, mv_bits: u32) -> u8 {
        let mv = Move(mv_bits);
        if mv.is_drop() {
            let pt = mv.moved_piece_type();
            let to_row = row_of(mv.to());
            let us = self.pos.side_to_move();
            if pt == FU && self.pos.has_pawn_on_col(us, col_of(mv.to())) {
                return 1;
            }
            if !droppable_row(pt, to_row, us) {
                return 3;
            }
            if pt == FU {
                return 2; // droppable_row and nifu both pass: must be uchifuzume
            }
        }
        4
    }

    pub fn do_move(&mut self, mv_bits: u32) -> Result<String, JsValue> {
        let mv = Move(mv_bits);
        let mut legal = Vec::with_capacity(96);
        generate_legal(&mut self.pos, &mut legal);
        if !legal.contains(&mv) {
            return Err(JsValue::from_str("illegal move"));
        }
        let mover = self.pos.side_to_move();
        let text = kif_text(&self.pos, mv, mover);
        self.pos.do_move(mv);
        Ok(text)
    }

    pub fn parse_usi(&self, s: &str) -> Result<u32, JsValue> {
        parse_usi_move(&self.pos, s).map(|m| m.0).map_err(|e| JsValue::from_str(&e))
    }

    pub fn move_to_usi_str(&self, mv_bits: u32) -> String {
        move_to_usi(Move(mv_bits))
    }

    pub fn undo(&mut self) -> bool {
        if self.pos.game_ply_depth() == 0 {
            return false;
        }
        self.pos.undo_move();
        true
    }

    /// bit0 王手, bit1 合法手なし, bit2 千日手(引き分け),
    /// bit3 連続王手の千日手で手番側の勝ち, bit4 連続王手の千日手で手番側の負け
    pub fn status(&mut self) -> u32 {
        let mut bits = 0u32;
        if self.pos.in_check() {
            bits |= 1;
        }
        let mut moves = Vec::with_capacity(96);
        generate_legal(&mut self.pos, &mut moves);
        if moves.is_empty() {
            bits |= 2;
        }
        match self.pos.detect_repetition() {
            Repetition::Draw => bits |= 4,
            Repetition::WinByPerpetual => bits |= 8,
            Repetition::LoseByPerpetual => bits |= 16,
            Repetition::None => {}
        }
        bits
    }

    pub fn search(&mut self, depth: u32, movetime_ms: u32, on_iteration: Option<js_sys::Function>) -> SearchResultJs {
        let limits = if movetime_ms > 0 { Limits::movetime(movetime_ms) } else { Limits::depth(depth.max(1)) };
        let result = if let Some(f) = on_iteration {
            let mut cb = move |info: &crate::search::SearchInfo| {
                let arr = js_sys::Array::new();
                arr.push(&JsValue::from_f64(info.depth as f64));
                arr.push(&JsValue::from_f64(info.seldepth as f64));
                arr.push(&JsValue::from_f64(info.score_cp as f64));
                arr.push(&JsValue::from_f64(info.mate_in as f64));
                arr.push(&JsValue::from_f64(info.nodes as f64));
                arr.push(&JsValue::from_f64(info.elapsed_ms as f64));
                let _ = f.call1(&JsValue::NULL, &arr);
            };
            run_search(&mut self.pos, &mut self.eval, &mut self.tt, limits, &WasmClock, Some(&mut cb))
        } else {
            run_search(&mut self.pos, &mut self.eval, &mut self.tt, limits, &WasmClock, None)
        };
        let js = SearchResultJs {
            best_move: result.best_move.0,
            score_cp: result.score_cp,
            mate_in: result.mate_in,
            depth: result.depth,
            nodes: result.nodes as f64,
            elapsed_ms: result.elapsed_ms,
            aborted: result.aborted,
            pv: result.pv.iter().map(|m| m.0).collect(),
        };
        self.last_result = Some(result);
        js
    }
}

impl Default for Engine {
    fn default() -> Self {
        Engine::new(16)
    }
}

#[wasm_bindgen]
pub struct SearchResultJs {
    best_move: u32,
    score_cp: i32,
    mate_in: i32,
    depth: u32,
    nodes: f64,
    elapsed_ms: u32,
    aborted: bool,
    pv: Vec<u32>,
}

#[wasm_bindgen]
impl SearchResultJs {
    #[wasm_bindgen(getter)]
    pub fn best_move(&self) -> u32 {
        self.best_move
    }
    #[wasm_bindgen(getter)]
    pub fn score_cp(&self) -> i32 {
        self.score_cp
    }
    #[wasm_bindgen(getter)]
    pub fn mate_in(&self) -> i32 {
        self.mate_in
    }
    #[wasm_bindgen(getter)]
    pub fn depth(&self) -> u32 {
        self.depth
    }
    #[wasm_bindgen(getter)]
    pub fn nodes(&self) -> f64 {
        self.nodes
    }
    #[wasm_bindgen(getter)]
    pub fn elapsed_ms(&self) -> u32 {
        self.elapsed_ms
    }
    #[wasm_bindgen(getter)]
    pub fn aborted(&self) -> bool {
        self.aborted
    }
    pub fn pv(&self) -> Vec<u32> {
        self.pv.clone()
    }
}
