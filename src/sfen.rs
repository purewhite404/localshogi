//! SFEN (position) and USI (move) text formats.
use crate::hand::Hand;
use crate::mov::Move;
use crate::position::Position;
use crate::types::*;

fn piece_letter(pt: PieceType) -> char {
    match pt {
        FU | TO => 'P',
        KYO | NKYO => 'L',
        KEI | NKEI => 'N',
        GIN | NGIN => 'S',
        KIN => 'G',
        KAKU | UMA => 'B',
        HI | RYU => 'R',
        OU => 'K',
        _ => '?',
    }
}

fn letter_piece(c: char) -> Option<PieceType> {
    match c.to_ascii_uppercase() {
        'P' => Some(FU),
        'L' => Some(KYO),
        'N' => Some(KEI),
        'S' => Some(GIN),
        'G' => Some(KIN),
        'B' => Some(KAKU),
        'R' => Some(HI),
        'K' => Some(OU),
        _ => None,
    }
}

pub fn parse_sfen(s: &str) -> Result<Position, String> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() < 3 {
        return Err(format!("sfen needs at least 3 fields, got {}", parts.len()));
    }
    let mut board = [NO_PIECE; 81];
    let rows: Vec<&str> = parts[0].split('/').collect();
    if rows.len() != 9 {
        return Err(format!("sfen board needs 9 ranks, got {}", rows.len()));
    }
    for (row, row_str) in rows.iter().enumerate() {
        let mut col = 0usize;
        let mut chars = row_str.chars().peekable();
        while let Some(c) = chars.next() {
            if col >= 9 {
                return Err(format!("rank {row} overflows 9 files"));
            }
            if c.is_ascii_digit() {
                let n = c.to_digit(10).unwrap() as usize;
                col += n;
                continue;
            }
            let promoted = c == '+';
            let letter = if promoted { chars.next().ok_or("dangling '+' in sfen")? } else { c };
            let base = letter_piece(letter).ok_or_else(|| format!("bad piece letter '{letter}'"))?;
            let pt = if promoted { promoted_of(base)? } else { base };
            let color = if letter.is_ascii_uppercase() { BLACK } else { WHITE };
            board[make_square(row as u8, col as u8) as usize] = make_piece(color, pt);
            col += 1;
        }
        if col != 9 {
            return Err(format!("rank {row} has {col} files, need 9"));
        }
    }

    let side = match parts[1] {
        "b" => BLACK,
        "w" => WHITE,
        other => return Err(format!("bad side-to-move '{other}'")),
    };

    let mut hands = [Hand::EMPTY; 2];
    if parts[2] != "-" {
        let mut chars = parts[2].chars().peekable();
        while let Some(c) = chars.next() {
            let mut count = 0i32;
            let mut has_digit = false;
            let mut cur = c;
            while cur.is_ascii_digit() {
                has_digit = true;
                count = count * 10 + cur.to_digit(10).unwrap() as i32;
                cur = chars.next().ok_or("dangling digit in hand field")?;
            }
            if !has_digit {
                count = 1;
            }
            let pt = letter_piece(cur).ok_or_else(|| format!("bad hand piece letter '{cur}'"))?;
            let color = if cur.is_ascii_uppercase() { BLACK } else { WHITE };
            for _ in 0..count {
                hands[color as usize].add(pt);
            }
        }
    }

    let ply: u32 = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);
    Ok(Position::from_parts(board, hands, side, ply))
}

fn promoted_of(pt: PieceType) -> Result<PieceType, String> {
    if !is_promotable(pt) {
        return Err(format!("piece type {pt} cannot be promoted"));
    }
    Ok(promoted(pt))
}

pub fn to_sfen(pos: &Position) -> String {
    let mut out = String::new();
    for row in 0..9u8 {
        let mut empty_run = 0u32;
        for col in 0..9u8 {
            let p = pos.piece_at(make_square(row, col));
            if is_none(p) {
                empty_run += 1;
                continue;
            }
            if empty_run > 0 {
                out.push_str(&empty_run.to_string());
                empty_run = 0;
            }
            let pt = piece_type(p);
            let letter = piece_letter(unpromoted(pt));
            let letter = if piece_color(p) == BLACK { letter.to_ascii_uppercase() } else { letter.to_ascii_lowercase() };
            if is_promoted(pt) {
                out.push('+');
            }
            out.push(letter);
        }
        if empty_run > 0 {
            out.push_str(&empty_run.to_string());
        }
        if row != 8 {
            out.push('/');
        }
    }
    out.push(' ');
    out.push(if pos.side_to_move() == BLACK { 'b' } else { 'w' });
    out.push(' ');
    let hands = pos.hands();
    let mut hand_str = String::new();
    // Conventional order: R B G S N L P, black first then white.
    for &(color, upper) in &[(BLACK, true), (WHITE, false)] {
        for &pt in &[HI, KAKU, KIN, GIN, KEI, KYO, FU] {
            let n = hands[color as usize].count(pt);
            if n > 0 {
                if n > 1 {
                    hand_str.push_str(&n.to_string());
                }
                let l = piece_letter(pt);
                hand_str.push(if upper { l.to_ascii_uppercase() } else { l.to_ascii_lowercase() });
            }
        }
    }
    if hand_str.is_empty() {
        out.push('-');
    } else {
        out.push_str(&hand_str);
    }
    out.push(' ');
    out.push_str(&pos.ply().max(1).to_string());
    out
}

fn parse_square(s: &str) -> Result<Square, String> {
    let mut chars = s.chars();
    let file_c = chars.next().ok_or("empty square")?;
    let rank_c = chars.next().ok_or("truncated square")?;
    let file = file_c.to_digit(10).ok_or("bad file digit")? as u8;
    if !(1..=9).contains(&file) {
        return Err("file out of range".into());
    }
    if !('a'..='i').contains(&rank_c) {
        return Err("bad rank letter".into());
    }
    let row = rank_c as u8 - b'a';
    let col = 9 - file;
    Ok(make_square(row, col))
}

fn square_to_usi(sq: Square) -> String {
    let file = 9 - col_of(sq);
    let rank = (b'a' + row_of(sq)) as char;
    format!("{file}{rank}")
}

/// Parse a USI move string ("7g7f", "8h2b+", "P*5e") against `pos` to fill in the
/// moved/captured piece-type fields the packed `Move` needs.
pub fn parse_usi_move(pos: &Position, s: &str) -> Result<Move, String> {
    let bytes: Vec<char> = s.chars().collect();
    if bytes.len() >= 2 && bytes[1] == '*' {
        let pt = letter_piece(bytes[0]).ok_or("bad drop piece letter")?;
        let to = parse_square(&s[2..])?;
        return Ok(Move::new_drop(pt, to));
    }
    let promote = s.ends_with('+');
    let core = if promote { &s[..s.len() - 1] } else { s };
    if core.len() != 4 {
        return Err(format!("bad usi move '{s}'"));
    }
    let from = parse_square(&core[0..2])?;
    let to = parse_square(&core[2..4])?;
    let moved = pos.piece_at(from);
    if is_none(moved) {
        return Err(format!("no piece at source square in move '{s}'"));
    }
    let captured = piece_type(pos.piece_at(to));
    Ok(Move::new_board(from, to, piece_type(moved), promote, captured))
}

pub fn move_to_usi(mv: Move) -> String {
    if mv.is_drop() {
        format!("{}*{}", piece_letter(mv.moved_piece_type()), square_to_usi(mv.to()))
    } else {
        format!("{}{}{}", square_to_usi(mv.from()), square_to_usi(mv.to()), if mv.is_promotion() { "+" } else { "" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HIRATE: &str = "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1";

    #[test]
    fn hirate_roundtrip() {
        let pos = parse_sfen(HIRATE).expect("parse");
        assert_eq!(to_sfen(&pos), HIRATE);
    }

    #[test]
    fn hirate_matches_startpos() {
        let parsed = parse_sfen(HIRATE).unwrap();
        let built = Position::startpos();
        assert_eq!(to_sfen(&parsed), to_sfen(&built));
    }

    #[test]
    fn usi_move_roundtrip() {
        let pos = Position::startpos();
        let mv = parse_usi_move(&pos, "7g7f").unwrap();
        assert_eq!(move_to_usi(mv), "7g7f");
        assert_eq!(mv.moved_piece_type(), FU);
    }

    #[test]
    fn usi_drop_roundtrip() {
        let s = "P*5e";
        // Build a position with a pawn in black's hand for a valid parse target square.
        let sfen = "9/9/9/9/4k4/9/9/9/4K4 b P 1";
        let pos = parse_sfen(sfen).unwrap();
        let mv = parse_usi_move(&pos, s).unwrap();
        assert_eq!(move_to_usi(mv), s);
        assert!(mv.is_drop());
    }

    #[test]
    fn hand_field_roundtrip() {
        let sfen = "lnsgkgsnl/1r5b1/pppppppp1/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b Pp 1";
        let pos = parse_sfen(sfen).unwrap();
        assert_eq!(pos.hand(BLACK, FU), 1);
        assert_eq!(pos.hand(WHITE, FU), 1);
        assert_eq!(to_sfen(&pos), sfen);
    }
}
