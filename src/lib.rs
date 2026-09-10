use serde::{Deserialize, Serialize};
use std::cmp::min;
use wasm_bindgen::prelude::*;

const FU: u8 = 1;
const KYO: u8 = 2;
const KEI: u8 = 3;
const GIN: u8 = 4;
const KIN: u8 = 5;
const KAKU: u8 = 6;
const HI: u8 = 7;
const OU: u8 = 8;
const TO: u8 = 9;
const NKYO: u8 = 10;
const NKEI: u8 = 11;
const NGIN: u8 = 12;
const UMA: u8 = 13;
const RYU: u8 = 14;

const VALUES: [i32; 15] = [0, 100, 250, 280, 350, 450, 700, 800, 20_000, 450, 550, 560, 550, 1_000, 1_100];

#[derive(Clone, Copy, Serialize, Deserialize)]
struct Cell { piece: u8, side: i8 }

type Board = Vec<Vec<Option<Cell>>>;
type Hand = std::collections::HashMap<u8, i32>;

#[derive(Deserialize)]
struct Hands {
    sent: Hand,
    gote: Hand,
    side: i8,
}

#[derive(Clone, Serialize, Deserialize)]
struct Move {
    #[serde(default)] drop: bool,
    #[serde(default)] from: Option<[usize; 2]>,
    to: [usize; 2],
    #[serde(default)] piece: u8,
    #[serde(default)] promo: bool,
}

fn promoted(piece: u8) -> u8 { match piece { FU => TO, KYO => NKYO, KEI => NKEI, GIN => NGIN, KAKU => UMA, HI => RYU, _ => piece } }
fn unpromoted(piece: u8) -> u8 { match piece { TO => FU, NKYO => KYO, NKEI => KEI, NGIN => GIN, UMA => KAKU, RYU => HI, _ => piece } }
fn in_board(r: i32, c: i32) -> bool { (0..9).contains(&r) && (0..9).contains(&c) }
fn can_promote(piece: u8, from: usize, to: usize, side: i8) -> bool {
    matches!(piece, FU | KYO | KEI | GIN | KAKU | HI) && ((side == 1 && (from <= 2 || to <= 2)) || (side == -1 && (from >= 6 || to >= 6)))
}
fn must_promote(piece: u8, to: usize, side: i8) -> bool {
    (matches!(piece, FU | KYO) && ((side == 1 && to == 0) || (side == -1 && to == 8))) ||
    (piece == KEI && ((side == 1 && to <= 1) || (side == -1 && to >= 7)))
}

fn dests(board: &Board, r: usize, c: usize, piece: u8, side: i8, include_own: bool) -> Vec<[usize; 2]> {
    let mut out = Vec::new();
    let forward = if side == 1 { -1 } else { 1 };
    let mut add = |dr: i32, dc: i32| -> bool {
        let nr = r as i32 + dr; let nc = c as i32 + dc;
        if !in_board(nr, nc) { return false; }
        let (nr, nc) = (nr as usize, nc as usize);
        if let Some(cell) = board[nr][nc] {
            if cell.side == side { if include_own { out.push([nr, nc]); } return false; }
            out.push([nr, nc]); return false;
        }
        out.push([nr, nc]); true
    };
    let mut slide = |dr: i32, dc: i32| { for i in 1..9 { if !add(dr * i, dc * i) { break; } } };
    match piece {
        FU => { add(forward, 0); }
        KYO => slide(forward, 0),
        KEI => { add(forward * 2, -1); add(forward * 2, 1); }
        GIN => for (dr, dc) in [(forward,-1),(forward,0),(forward,1),(-forward,-1),(-forward,1)] { add(dr, dc); },
        KIN | TO | NKYO | NKEI | NGIN => for (dr, dc) in [(forward,0),(forward,-1),(forward,1),(0,-1),(0,1),(-forward,0)] { add(dr, dc); },
        KAKU => for (dr, dc) in [(-1,-1),(-1,1),(1,-1),(1,1)] { slide(dr, dc); },
        HI => for (dr, dc) in [(-1,0),(1,0),(0,-1),(0,1)] { slide(dr, dc); },
        UMA => { for (dr, dc) in [(-1,-1),(-1,1),(1,-1),(1,1)] { slide(dr, dc); } for (dr, dc) in [(-1,0),(1,0),(0,-1),(0,1)] { add(dr, dc); } },
        RYU => { for (dr, dc) in [(-1,0),(1,0),(0,-1),(0,1)] { slide(dr, dc); } for (dr, dc) in [(-1,-1),(-1,1),(1,-1),(1,1)] { add(dr, dc); } },
        OU => for (dr, dc) in [(-1,-1),(-1,0),(-1,1),(0,-1),(0,1),(1,-1),(1,0),(1,1)] { add(dr, dc); },
        _ => {}
    }
    out
}

fn apply_move(board: &Board, mv: &Move, side: i8, sh: &Hand, gh: &Hand) -> (Board, Hand, Hand) {
    let mut next = board.clone(); let mut nsh = sh.clone(); let mut ngh = gh.clone();
    if mv.drop {
        let hand = if side == 1 { &mut nsh } else { &mut ngh };
        *hand.entry(mv.piece).or_insert(0) -= 1;
        next[mv.to[0]][mv.to[1]] = Some(Cell { piece: mv.piece, side });
    } else if let Some(from) = mv.from {
        if let Some(captured) = next[mv.to[0]][mv.to[1]] {
            let hand = if side == 1 { &mut nsh } else { &mut ngh };
            *hand.entry(unpromoted(captured.piece)).or_insert(0) += 1;
        }
        let mut piece = next[from[0]][from[1]].unwrap().piece;
        if mv.promo { piece = promoted(piece); }
        next[mv.to[0]][mv.to[1]] = Some(Cell { piece, side });
        next[from[0]][from[1]] = None;
    }
    (next, nsh, ngh)
}

fn king_square(board: &Board, side: i8) -> Option<[usize; 2]> {
    for r in 0..9 { for c in 0..9 { if let Some(cell) = board[r][c] { if cell.side == side && cell.piece == OU { return Some([r,c]); } } } }
    None
}
fn attacked(board: &Board, target: [usize; 2], by: i8) -> bool {
    for r in 0..9 { for c in 0..9 { if let Some(cell) = board[r][c] { if cell.side == by && dests(board,r,c,cell.piece,by,false).contains(&target) { return true; } } } }
    false
}
fn in_check(board: &Board, side: i8) -> bool { king_square(board, side).map(|k| attacked(board,k,-side)).unwrap_or(true) }
fn can_drop(piece: u8, r: usize, c: usize, board: &Board, side: i8) -> bool {
    if piece == FU {
        if (0..9).any(|rr| board[rr][c].map(|x| x.side == side && x.piece == FU).unwrap_or(false)) { return false; }
        if (side == 1 && r == 0) || (side == -1 && r == 8) { return false; }
    }
    if piece == KYO && ((side == 1 && r == 0) || (side == -1 && r == 8)) { return false; }
    if piece == KEI && ((side == 1 && r <= 1) || (side == -1 && r >= 7)) { return false; }
    true
}

fn move_score(board: &Board, mv: &Move) -> i32 {
    let mut score = 0;
    if !mv.drop {
        if let Some(captured) = board[mv.to[0]][mv.to[1]] { score += 100_000 + VALUES[captured.piece as usize] * 100 - VALUES[board[mv.from.unwrap()[0]][mv.from.unwrap()[1]].unwrap().piece as usize]; }
        if mv.promo { score += 50_000; }
    }
    score
}
fn get_moves(board: &Board, side: i8, sh: &Hand, gh: &Hand) -> Vec<Move> {
    let mut moves = Vec::new();
    for r in 0..9 { for c in 0..9 { if let Some(cell) = board[r][c] { if cell.side != side { continue; } for to in dests(board,r,c,cell.piece,side,false) {
        if must_promote(cell.piece,to[0],side) { moves.push(Move { drop:false, from:Some([r,c]), to, piece:0, promo:true }); }
        else { moves.push(Move { drop:false, from:Some([r,c]), to, piece:0, promo:false }); if can_promote(cell.piece,r,to[0],side) { moves.push(Move { drop:false, from:Some([r,c]), to, piece:0, promo:true }); } }
    } } } }
    let hand = if side == 1 { sh } else { gh };
    for (&piece, &count) in hand { if count > 0 { for r in 0..9 { for c in 0..9 { if board[r][c].is_none() && can_drop(piece,r,c,board,side) { moves.push(Move { drop:true, from:None, to:[r,c], piece, promo:false }); } } } } }
    moves.retain(|mv| { let (b,_,_) = apply_move(board,mv,side,sh,gh); !in_check(&b,side) });
    moves.sort_by_key(|mv| -move_score(board,mv));
    moves
}

fn effect_map(board: &Board) -> [[[i32; 9]; 9]; 2] {
    let mut effects = [[[0; 9]; 9]; 2];
    for r in 0..9 { for c in 0..9 { if let Some(cell) = board[r][c] {
        let side_index = if cell.side == 1 { 0 } else { 1 };
        for target in dests(board,r,c,cell.piece,cell.side,true) { effects[side_index][target[0]][target[1]] += 1; }
    } } }
    effects
}
fn evaluate(board: &Board, sh: &Hand, gh: &Hand) -> i32 {
    let effects = effect_map(board); let black_king = king_square(board,1).unwrap_or([4,4]); let white_king = king_square(board,-1).unwrap_or([4,4]); let mut score = 0;
    for r in 0..9 { for c in 0..9 {
        let b = min(effects[0][r][c],2); let w = min(effects[1][r][c],2);
        let bd = (black_king[0].abs_diff(r).max(black_king[1].abs_diff(c)) + 1) as i32;
        let wd = (white_king[0].abs_diff(r).max(white_king[1].abs_diff(c)) + 1) as i32;
        score += 83 * b / bd - 92 * w / bd + 92 * b / wd - 83 * w / wd;
        if let Some(cell) = board[r][c] { if cell.piece == OU { score += if cell.side == 1 { 700 - (r as i32 * 30) } else { -(700 - ((8-r) as i32 * 30)) }; } else { score += cell.side as i32 * VALUES[cell.piece as usize] * (1000 + if b > 0 && cell.side == 1 { 33 } else { 0 } + if w > 0 && cell.side == -1 { 33 } else { 0 }) / 1000; } }
    } }
    for (&p,&n) in sh { score += VALUES[p as usize] * n * 8 / 10; } for (&p,&n) in gh { score -= VALUES[p as usize] * n * 8 / 10; }
    score
}
fn negamax(board: &Board, sh: &Hand, gh: &Hand, side: i8, depth: i32, mut alpha: i32, beta: i32) -> i32 {
    let moves = get_moves(board,side,sh,gh); if moves.is_empty() { return -99_999; } if depth <= 0 { return side as i32 * evaluate(board,sh,gh); }
    let mut best = -1_000_000;
    for mv in moves { let (b,ns,ng) = apply_move(board,&mv,side,sh,gh); let value = -negamax(&b,&ns,&ng,-side,depth-1,-beta,-alpha); best = best.max(value); alpha = alpha.max(value); if alpha >= beta { break; } }
    best
}

#[wasm_bindgen]
pub fn best_move(board_json: &str, hands_json: &str, depth: u32) -> Result<String, JsValue> {
    let board: Board = serde_json::from_str(board_json).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let hands: Hands = serde_json::from_str(hands_json).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let side = hands.side;
    let sh = hands.sent;
    let gh = hands.gote;
    let moves = get_moves(&board,side,&sh,&gh); if moves.is_empty() { return Ok("null".into()); }
    let mut best_value = -1_000_000; let mut best = moves[0].clone();
    for mv in moves { let (b,ns,ng) = apply_move(&board,&mv,side,&sh,&gh); let value = -negamax(&b,&ns,&ng,-side,depth.saturating_sub(1) as i32,-1_000_000,1_000_000); if value > best_value { best_value = value; best = mv; } }
    serde_json::to_string(&best).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn initial_board() -> Board {
        let mut board = vec![vec![None; 9]; 9];
        let back = [KYO, KEI, GIN, KIN, OU, KIN, GIN, KEI, KYO];
        for c in 0..9 { board[0][c] = Some(Cell { piece: back[c], side: -1 }); board[2][c] = Some(Cell { piece: FU, side: -1 }); board[6][c] = Some(Cell { piece: FU, side: 1 }); board[8][c] = Some(Cell { piece: back[8-c], side: 1 }); }
        board[1][1] = Some(Cell { piece: HI, side: -1 }); board[1][7] = Some(Cell { piece: KAKU, side: -1 });
        board[7][7] = Some(Cell { piece: HI, side: 1 }); board[7][1] = Some(Cell { piece: KAKU, side: 1 });
        board
    }

    #[test]
    fn initial_position_search_finishes() {
        let board = initial_board(); let hand = Hand::new();
        assert!(!get_moves(&board, -1, &hand, &hand).is_empty());
        let _ = negamax(&board, &hand, &hand, -1, 0, -1_000_000, 1_000_000);
    }
}
