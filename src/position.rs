//! Board position: mailbox + bitboards, hands, make/unmake, check/pin detection,
//! repetition tracking. The move-generation legality logic lives in `movegen`.
use crate::bitboard::Bitboard;
use crate::hand::Hand;
use crate::mov::Move;
use crate::tables::*;
use crate::types::*;
use crate::zobrist;

#[derive(Clone, Copy, Debug)]
pub struct StateInfo {
    pub board_key: u64,
    pub hand_key: u64,
    pub captured: Piece,
    pub last_move: Move,
    pub checkers: Bitboard,
    /// Plies (2 per own move) of uninterrupted check delivered by each color, used for
    /// the 連続王手の千日手 (perpetual-check sennichite) rule.
    pub continuous_check: [u16; 2],
    pub plies_from_null: u16,
}

impl StateInfo {
    fn root() -> StateInfo {
        StateInfo {
            board_key: 0,
            hand_key: 0,
            captured: NO_PIECE,
            last_move: crate::mov::MOVE_NONE,
            checkers: Bitboard::EMPTY,
            continuous_check: [0, 0],
            plies_from_null: 0,
        }
    }
}

#[derive(Clone)]
pub struct Position {
    board: [Piece; 81],
    by_type: [Bitboard; PIECE_TYPE_COUNT],
    by_color: [Bitboard; 2],
    occupied: Bitboard,
    hands: [Hand; 2],
    king_sq: [Square; 2],
    side: Color,
    /// bit c set = an unpromoted FU of that color sits somewhere in column c (nifu check).
    pawn_files: [u16; 2],
    ply: u32,
    states: Vec<StateInfo>,
}

pub enum Repetition {
    None,
    Draw,
    WinByPerpetual,
    LoseByPerpetual,
}

impl Position {
    pub fn empty() -> Position {
        Position {
            board: [NO_PIECE; 81],
            by_type: [Bitboard::EMPTY; PIECE_TYPE_COUNT],
            by_color: [Bitboard::EMPTY; 2],
            occupied: Bitboard::EMPTY,
            hands: [Hand::EMPTY; 2],
            king_sq: [SQ_NONE, SQ_NONE],
            side: BLACK,
            pawn_files: [0, 0],
            ply: 0,
            states: vec![StateInfo::root()],
        }
    }

    pub fn startpos() -> Position {
        let mut pos = Position::empty();
        let back: [PieceType; 9] = [KYO, KEI, GIN, KIN, OU, KIN, GIN, KEI, KYO];
        for col in 0..9u8 {
            pos.raw_put(make_piece(WHITE, back[col as usize]), make_square(0, col));
            pos.raw_put(make_piece(WHITE, FU), make_square(2, col));
            pos.raw_put(make_piece(BLACK, FU), make_square(6, col));
            pos.raw_put(make_piece(BLACK, back[8 - col as usize]), make_square(8, col));
        }
        pos.raw_put(make_piece(WHITE, HI), make_square(1, 1));
        pos.raw_put(make_piece(WHITE, KAKU), make_square(1, 7));
        pos.raw_put(make_piece(BLACK, HI), make_square(7, 7));
        pos.raw_put(make_piece(BLACK, KAKU), make_square(7, 1));
        pos.side = BLACK;
        pos.finish_setup();
        pos
    }

    /// Build a position from explicit parts (used by the SFEN parser). Recomputes
    /// every derived field (keys, king squares, pawn files, checkers) from scratch.
    pub fn from_parts(board: [Piece; 81], hands: [Hand; 2], side: Color, ply: u32) -> Position {
        let mut pos = Position::empty();
        for sq in 0u8..81 {
            let p = board[sq as usize];
            if !is_none(p) {
                pos.raw_put(p, sq);
            }
        }
        pos.hands = hands;
        pos.side = side;
        pos.ply = ply;
        pos.finish_setup();
        pos
    }

    /// Place a piece directly on the board without touching keys/state (setup only).
    fn raw_put(&mut self, piece: Piece, sq: Square) {
        self.board[sq as usize] = piece;
        self.by_type[piece_type(piece) as usize].set(sq);
        self.by_color[piece_color(piece) as usize].set(sq);
        self.occupied.set(sq);
        if piece_type(piece) == OU {
            self.king_sq[piece_color(piece) as usize] = sq;
        }
        if piece_type(piece) == FU {
            self.pawn_files[piece_color(piece) as usize] |= 1 << col_of(sq);
        }
    }

    /// Recompute keys/checkers for the root state after raw setup (startpos / SFEN).
    fn finish_setup(&mut self) {
        let mut board_key: u64 = if self.side == WHITE { zobrist::SIDE_KEY } else { 0 };
        for sq in 0u8..81 {
            let p = self.board[sq as usize];
            if !is_none(p) {
                board_key ^= zobrist::psq_key(p, sq);
            }
        }
        let mut hand_key: u64 = 0;
        for &c in &[BLACK, WHITE] {
            for &pt in &Hand::TYPES {
                let n = self.hands[c as usize].count(pt);
                for _ in 0..n {
                    hand_key = hand_key.wrapping_add(zobrist::hand_inc(c, pt));
                }
            }
        }
        let checkers = if self.king_sq[self.side as usize] != SQ_NONE {
            self.attackers_to(self.king_sq[self.side as usize], self.occupied, opposite(self.side))
        } else {
            Bitboard::EMPTY
        };
        self.states = vec![StateInfo { board_key, hand_key, captured: NO_PIECE, last_move: crate::mov::MOVE_NONE, checkers, continuous_check: [0, 0], plies_from_null: 0 }];
    }

    // ---------------- accessors ----------------

    #[inline]
    pub fn piece_at(&self, sq: Square) -> Piece {
        self.board[sq as usize]
    }

    #[inline]
    pub fn pieces(&self, color: Color, pt: PieceType) -> Bitboard {
        self.by_type[pt as usize] & self.by_color[color as usize]
    }

    #[inline]
    pub fn pieces_of_type(&self, pt: PieceType) -> Bitboard {
        self.by_type[pt as usize]
    }

    #[inline]
    pub fn pieces_of_color(&self, color: Color) -> Bitboard {
        self.by_color[color as usize]
    }

    #[inline]
    pub fn occupied(&self) -> Bitboard {
        self.occupied
    }

    #[inline]
    pub fn side_to_move(&self) -> Color {
        self.side
    }

    #[inline]
    pub fn ply(&self) -> u32 {
        self.ply
    }

    #[inline]
    pub fn king_square(&self, color: Color) -> Square {
        self.king_sq[color as usize]
    }

    #[inline]
    pub fn hand(&self, color: Color, pt: PieceType) -> i32 {
        self.hands[color as usize].count(pt)
    }

    #[inline]
    pub fn hands(&self) -> [Hand; 2] {
        self.hands
    }

    #[inline]
    pub fn board(&self) -> &[Piece; 81] {
        &self.board
    }

    #[inline]
    fn state(&self) -> &StateInfo {
        self.states.last().unwrap()
    }

    #[inline]
    pub fn checkers(&self) -> Bitboard {
        self.state().checkers
    }

    #[inline]
    pub fn in_check(&self) -> bool {
        !self.checkers().is_empty()
    }

    #[inline]
    pub fn key(&self) -> u64 {
        self.state().board_key ^ self.state().hand_key
    }

    #[inline]
    pub fn last_move(&self) -> Move {
        self.state().last_move
    }

    pub fn game_ply_depth(&self) -> usize {
        self.states.len() - 1
    }

    /// True if `color`'s pawn already occupies `col` on the board (nifu check).
    #[inline]
    pub fn has_pawn_on_col(&self, color: Color, col: u8) -> bool {
        self.pawn_files[color as usize] & (1 << col) != 0
    }

    // ---------------- attacks ----------------

    /// All pieces of `by_color` that attack square `to`, given occupancy `occ`.
    pub fn attackers_to(&self, to: Square, occ: Bitboard, by_color: Color) -> Bitboard {
        let opp = opposite(by_color);
        let mut att = Bitboard::EMPTY;
        att |= STEP_ATTACKS[opp as usize][to as usize][FU as usize] & self.pieces(by_color, FU);
        att |= STEP_ATTACKS[opp as usize][to as usize][KEI as usize] & self.pieces(by_color, KEI);
        att |= STEP_ATTACKS[opp as usize][to as usize][GIN as usize] & self.pieces(by_color, GIN);
        let golds = self.pieces(by_color, KIN) | self.pieces(by_color, TO) | self.pieces(by_color, NKYO) | self.pieces(by_color, NKEI) | self.pieces(by_color, NGIN);
        att |= STEP_ATTACKS[opp as usize][to as usize][KIN as usize] & golds;
        att |= KING_STEPS[to as usize] & self.pieces(by_color, OU);
        let lance_dir = opposite_dir(forward_dir(by_color));
        att |= ray_attacks(to, lance_dir, occ) & self.pieces(by_color, KYO);
        let bishops = self.pieces(by_color, KAKU) | self.pieces(by_color, UMA);
        att |= bishop_attacks(to, occ) & bishops;
        att |= ORTH_STEPS[to as usize] & self.pieces(by_color, UMA);
        let rooks = self.pieces(by_color, HI) | self.pieces(by_color, RYU);
        att |= rook_attacks(to, occ) & rooks;
        att |= DIAG_STEPS[to as usize] & self.pieces(by_color, RYU);
        att
    }

    #[inline]
    pub fn attackers_to_both(&self, to: Square, occ: Bitboard) -> Bitboard {
        self.attackers_to(to, occ, BLACK) | self.attackers_to(to, occ, WHITE)
    }

    /// Pieces of `color` that are pinned against their own king, and (paired) the
    /// enemy sliders pinning them. Recomputed on demand (called once per legal-movegen
    /// pass, not cached in StateInfo) — simpler and cheap enough at these search depths.
    pub fn pinned_pieces(&self, color: Color) -> Bitboard {
        let king = self.king_sq[color as usize];
        if king == SQ_NONE {
            return Bitboard::EMPTY;
        }
        let them = opposite(color);
        let mut pinned = Bitboard::EMPTY;

        // Candidate pinning sliders: lances aligned along the king's forward file,
        // bishops/horses along diagonals, rooks/dragons along rank/file.
        let lance_dir = opposite_dir(forward_dir(color));
        let lance_ray = RAY[king as usize][lance_dir as usize] & self.pieces(them, KYO);
        for pinner in lance_ray.iter() {
            let between_bb = between(king, pinner);
            let blockers = between_bb & self.occupied;
            if blockers.count_ones() == 1 && (blockers & self.by_color[color as usize]).count_ones() == 1 {
                pinned |= blockers;
            }
        }
        for &d in &BISHOP_DIRS {
            let ray = RAY[king as usize][d as usize];
            let candidates = ray & (self.pieces(them, KAKU) | self.pieces(them, UMA));
            if let Some(pinner) = first_on_ray(ray, candidates, king, d) {
                let blockers = between(king, pinner) & self.occupied;
                if blockers.count_ones() == 1 && (blockers & self.by_color[color as usize]).count_ones() == 1 {
                    pinned |= blockers;
                }
            }
        }
        for &d in &ROOK_DIRS {
            let ray = RAY[king as usize][d as usize];
            let candidates = ray & (self.pieces(them, HI) | self.pieces(them, RYU));
            if let Some(pinner) = first_on_ray(ray, candidates, king, d) {
                let blockers = between(king, pinner) & self.occupied;
                if blockers.count_ones() == 1 && (blockers & self.by_color[color as usize]).count_ones() == 1 {
                    pinned |= blockers;
                }
            }
        }
        pinned
    }

    // ---------------- make / unmake ----------------

    fn put_piece(&mut self, piece: Piece, sq: Square) {
        self.board[sq as usize] = piece;
        self.by_type[piece_type(piece) as usize].set(sq);
        self.by_color[piece_color(piece) as usize].set(sq);
        self.occupied.set(sq);
    }

    fn remove_piece(&mut self, piece: Piece, sq: Square) {
        self.board[sq as usize] = NO_PIECE;
        self.by_type[piece_type(piece) as usize].clear(sq);
        self.by_color[piece_color(piece) as usize].clear(sq);
        self.occupied.clear(sq);
    }

    pub fn do_move(&mut self, mv: Move) {
        let us = self.side;
        let them = opposite(us);
        let prev = *self.state();
        let mut board_key = prev.board_key ^ zobrist::SIDE_KEY;
        let mut hand_key = prev.hand_key;
        let mut captured = NO_PIECE;

        if mv.is_drop() {
            let pt = mv.moved_piece_type();
            let to = mv.to();
            self.hands[us as usize].remove(pt);
            hand_key = hand_key.wrapping_sub(zobrist::hand_inc(us, pt));
            let piece = make_piece(us, pt);
            self.put_piece(piece, to);
            board_key ^= zobrist::psq_key(piece, to);
            if pt == FU {
                self.pawn_files[us as usize] |= 1 << col_of(to);
            }
        } else {
            let from = mv.from();
            let to = mv.to();
            let moved_pt = mv.moved_piece_type();
            let moved_piece = make_piece(us, moved_pt);
            let target = self.board[to as usize];
            if !is_none(target) {
                captured = target;
                let cap_pt = piece_type(target);
                self.remove_piece(target, to);
                board_key ^= zobrist::psq_key(target, to);
                let unpro = unpromoted(cap_pt);
                self.hands[us as usize].add(unpro);
                hand_key = hand_key.wrapping_add(zobrist::hand_inc(us, unpro));
                if cap_pt == FU {
                    self.pawn_files[them as usize] &= !(1 << col_of(to));
                }
            }
            self.remove_piece(moved_piece, from);
            board_key ^= zobrist::psq_key(moved_piece, from);
            let final_pt = if mv.is_promotion() { promoted(moved_pt) } else { moved_pt };
            let final_piece = make_piece(us, final_pt);
            self.put_piece(final_piece, to);
            board_key ^= zobrist::psq_key(final_piece, to);
            if moved_pt == OU {
                self.king_sq[us as usize] = to;
            }
            if moved_pt == FU && mv.is_promotion() {
                self.pawn_files[us as usize] &= !(1 << col_of(from));
            }
        }

        self.side = them;
        self.ply += 1;

        let checkers = if self.king_sq[them as usize] != SQ_NONE {
            self.attackers_to(self.king_sq[them as usize], self.occupied, us)
        } else {
            Bitboard::EMPTY
        };
        let gives_check = !checkers.is_empty();
        let mut continuous_check = prev.continuous_check;
        continuous_check[us as usize] = if gives_check { prev.continuous_check[us as usize] + 2 } else { 0 };

        self.states.push(StateInfo {
            board_key,
            hand_key,
            captured,
            last_move: mv,
            checkers,
            continuous_check,
            plies_from_null: prev.plies_from_null + 1,
        });
    }

    pub fn undo_move(&mut self) {
        let popped = self.states.pop().expect("undo without matching do_move");
        let mv = popped.last_move;
        let us = opposite(self.side);
        self.side = us;
        self.ply -= 1;
        let them = opposite(us);

        if mv.is_drop() {
            let pt = mv.moved_piece_type();
            let to = mv.to();
            self.remove_piece(make_piece(us, pt), to);
            self.hands[us as usize].add(pt);
            if pt == FU {
                self.pawn_files[us as usize] &= !(1 << col_of(to));
            }
        } else {
            let from = mv.from();
            let to = mv.to();
            let moved_pt = mv.moved_piece_type();
            let final_pt = if mv.is_promotion() { promoted(moved_pt) } else { moved_pt };
            self.remove_piece(make_piece(us, final_pt), to);
            self.put_piece(make_piece(us, moved_pt), from);
            if moved_pt == OU {
                self.king_sq[us as usize] = from;
            }
            if moved_pt == FU && mv.is_promotion() {
                self.pawn_files[us as usize] |= 1 << col_of(from);
            }
            if !is_none(popped.captured) {
                self.put_piece(popped.captured, to);
                let unpro = unpromoted(piece_type(popped.captured));
                self.hands[us as usize].remove(unpro);
                if piece_type(popped.captured) == FU {
                    self.pawn_files[them as usize] |= 1 << col_of(to);
                }
            }
        }
    }

    /// Null move: side passes, nothing else changes. `plies_from_null` resets so
    /// repetition detection never walks across it.
    pub fn do_null_move(&mut self) {
        let prev = *self.state();
        let board_key = prev.board_key ^ zobrist::SIDE_KEY;
        self.side = opposite(self.side);
        self.ply += 1;
        let checkers = Bitboard::EMPTY; // side just given the move was not in check when it was legal to null
        let mut continuous_check = prev.continuous_check;
        continuous_check[opposite(self.side) as usize] = 0;
        self.states.push(StateInfo { board_key, hand_key: prev.hand_key, captured: NO_PIECE, last_move: crate::mov::MOVE_NULL, checkers, continuous_check, plies_from_null: 0 });
    }

    pub fn undo_null_move(&mut self) {
        self.states.pop();
        self.side = opposite(self.side);
        self.ply -= 1;
    }

    // ---------------- repetition ----------------

    pub fn detect_repetition(&self) -> Repetition {
        let cur = *self.state();
        let stack_depth = self.states.len() as u16 - 1;
        let end = cur.plies_from_null.min(stack_depth);
        let mut count = 0u32;
        let mut i: u16 = 4;
        while i <= end {
            let idx = self.states.len() - 1 - i as usize;
            let prev = self.states[idx];
            if prev.board_key == cur.board_key && prev.hand_key == cur.hand_key {
                count += 1;
                if count == 3 {
                    let us = self.side;
                    let them = opposite(us);
                    if cur.continuous_check[them as usize] >= i {
                        return Repetition::WinByPerpetual;
                    }
                    if cur.continuous_check[us as usize] >= i {
                        return Repetition::LoseByPerpetual;
                    }
                    return Repetition::Draw;
                }
            }
            i += 2;
        }
        Repetition::None
    }

    // ---------------- consistency (debug/test only) ----------------

    #[cfg(any(test, feature = "native"))]
    pub fn check_consistency(&self) -> Result<(), String> {
        let mut occ = Bitboard::EMPTY;
        let mut by_color = [Bitboard::EMPTY; 2];
        let mut by_type = [Bitboard::EMPTY; PIECE_TYPE_COUNT];
        for sq in 0u8..81 {
            let p = self.board[sq as usize];
            if !is_none(p) {
                occ.set(sq);
                by_color[piece_color(p) as usize].set(sq);
                by_type[piece_type(p) as usize].set(sq);
            }
        }
        if occ != self.occupied {
            return Err("occupied mismatch".into());
        }
        if by_color != self.by_color {
            return Err("by_color mismatch".into());
        }
        if by_type != self.by_type {
            return Err("by_type mismatch".into());
        }
        for &c in &[BLACK, WHITE] {
            if self.king_sq[c as usize] != SQ_NONE && self.board[self.king_sq[c as usize] as usize] != make_piece(c, OU) {
                return Err("king_sq mismatch".into());
            }
            let mut pf = 0u16;
            for sq in self.pieces(c, FU).iter() {
                pf |= 1 << col_of(sq);
            }
            if pf != self.pawn_files[c as usize] {
                return Err("pawn_files mismatch".into());
            }
        }
        let mut board_key: u64 = if self.side == WHITE { zobrist::SIDE_KEY } else { 0 };
        for sq in 0u8..81 {
            let p = self.board[sq as usize];
            if !is_none(p) {
                board_key ^= zobrist::psq_key(p, sq);
            }
        }
        if board_key != self.state().board_key {
            return Err("board_key mismatch".into());
        }
        let mut hand_key: u64 = 0;
        for &c in &[BLACK, WHITE] {
            for &pt in &Hand::TYPES {
                for _ in 0..self.hands[c as usize].count(pt) {
                    hand_key = hand_key.wrapping_add(zobrist::hand_inc(c, pt));
                }
            }
        }
        if hand_key != self.state().hand_key {
            return Err("hand_key mismatch".into());
        }
        Ok(())
    }
}

/// Nearest square in `candidates` to `from` along `ray`, i.e. the first blocker a
/// slider based at `from` would meet walking direction `d`. Used for pin detection.
fn first_on_ray(ray: Bitboard, candidates: Bitboard, _from: Square, d: u8) -> Option<Square> {
    let hits = ray & candidates;
    if hits.is_empty() {
        return None;
    }
    if DIR_INCREASING[d as usize] { hits.lsb() } else { hits.msb() }
}

impl PartialEq for Position {
    fn eq(&self, other: &Self) -> bool {
        self.board == other.board
            && self.hands == other.hands
            && self.side == other.side
            && self.king_sq == other.king_sq
            && self.pawn_files == other.pawn_files
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startpos_consistency() {
        let pos = Position::startpos();
        assert!(pos.check_consistency().is_ok());
        assert_eq!(pos.side_to_move(), BLACK);
        assert!(!pos.in_check());
        assert_eq!(pos.king_square(BLACK), make_square(8, 4));
        assert_eq!(pos.king_square(WHITE), make_square(0, 4));
    }

    #[test]
    fn do_undo_roundtrip_quiet_move() {
        let mut pos = Position::startpos();
        let before = pos.clone();
        let mv = Move::new_board(make_square(6, 4), make_square(5, 4), FU, false, 0);
        pos.do_move(mv);
        assert!(pos.check_consistency().is_ok());
        assert_eq!(pos.side_to_move(), WHITE);
        pos.undo_move();
        assert!(pos.check_consistency().is_ok());
        assert!(pos == before);
        assert_eq!(pos.key(), before.key());
    }

    #[test]
    fn do_undo_roundtrip_capture_and_promotion() {
        // Black bishop capture+promotion isn't reachable in one move from startpos;
        // construct a simpler synthetic position instead.
        let mut board = [NO_PIECE; 81];
        board[make_square(8, 4) as usize] = make_piece(BLACK, OU);
        board[make_square(0, 4) as usize] = make_piece(WHITE, OU);
        board[make_square(1, 4) as usize] = make_piece(BLACK, KAKU);
        board[make_square(0, 0) as usize] = make_piece(WHITE, FU);
        let mut pos = Position::from_parts(board, [Hand::EMPTY, Hand::EMPTY], BLACK, 1);
        let before = pos.clone();
        let mv = Move::new_board(make_square(1, 4), make_square(0, 0), KAKU, true, FU);
        pos.do_move(mv);
        assert!(pos.check_consistency().is_ok());
        assert_eq!(pos.hand(BLACK, FU), 1);
        pos.undo_move();
        assert!(pos.check_consistency().is_ok());
        assert!(pos == before);
        assert_eq!(pos.key(), before.key());
    }

    #[test]
    fn nifu_tracking_across_promotion() {
        let mut board = [NO_PIECE; 81];
        board[make_square(8, 4) as usize] = make_piece(BLACK, OU);
        board[make_square(0, 4) as usize] = make_piece(WHITE, OU);
        board[make_square(1, 0) as usize] = make_piece(BLACK, FU);
        let mut pos = Position::from_parts(board, [Hand::EMPTY, Hand::EMPTY], BLACK, 1);
        assert!(pos.has_pawn_on_col(BLACK, 0));
        let mv = Move::new_board(make_square(1, 0), make_square(0, 0), FU, true, 0);
        pos.do_move(mv);
        assert!(!pos.has_pawn_on_col(BLACK, 0), "promoted pawn must free the column for nifu");
        pos.undo_move();
        assert!(pos.has_pawn_on_col(BLACK, 0));
    }
}

#[cfg(test)]
mod attackers_to_property_tests {
    use super::*;
    use crate::movegen::piece_attacks;

    /// Simple xorshift PRNG, no external dependency needed for this deterministic test.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn next_sq(&mut self) -> Square {
            (self.next() % 81) as Square
        }
    }

    /// For every attacker placement and many random board occupancies, `attackers_to`
    /// must agree exactly with the brute-force definition: "square `sq` attacks `to`
    /// iff `to` is in that piece's own attack set from `sq`". This is the cheapest,
    /// most direct check on the primitive every check/legality decision is built on.
    #[test]
    fn attackers_to_matches_brute_force_definition() {
        let mut rng = Rng(0xC0FFEE_1234_5678);
        let all_types = [FU, KYO, KEI, GIN, KIN, KAKU, HI, OU, TO, NKYO, NKEI, NGIN, UMA, RYU];
        for _trial in 0..4000 {
            let target = rng.next_sq();
            let attacker_sq = rng.next_sq();
            if attacker_sq == target {
                continue;
            }
            let color = if rng.next() & 1 == 0 { BLACK } else { WHITE };
            let pt = all_types[(rng.next() as usize) % all_types.len()];

            // Random sparse occupancy (other pieces as blockers), excluding target/attacker.
            let mut occ = Bitboard::EMPTY;
            occ.set(attacker_sq);
            for _ in 0..6 {
                let sq = rng.next_sq();
                if sq != target {
                    occ.set(sq);
                }
            }

            let brute_force_hits = piece_attacks(color, pt, attacker_sq, occ).contains(target);

            // Build a minimal Position with exactly this one attacker on the board
            // (plus the two kings, required by Position's invariants) and query
            // attackers_to at `target` with the SAME occupancy by also placing filler
            // pieces matching `occ` exactly (as black pawns, a piece type irrelevant to
            // the attacker's own contribution) so ray-blocking behaves identically.
            let mut board = [NO_PIECE; 81];
            for sq in occ.iter() {
                board[sq as usize] = make_piece(BLACK, FU); // any non-OU filler piece
            }
            board[attacker_sq as usize] = make_piece(color, pt);
            // Place both kings on squares outside occ/target/attacker to avoid clobbering.
            let bk = find_free_square(&board, target, attacker_sq, SQ_NONE);
            board[bk as usize] = make_piece(BLACK, OU);
            let wk = find_free_square(&board, target, attacker_sq, bk);
            board[wk as usize] = make_piece(WHITE, OU);
            // Re-derive occupancy including the filler squares AND the two kings, since
            // Position::from_parts builds occ from the board itself.
            let pos = Position::from_parts(board, [Hand::EMPTY, Hand::EMPTY], BLACK, 1);

            let full_occ = pos.occupied();
            let via_attackers_to = pos.attackers_to(target, full_occ, color).contains(attacker_sq);

            // Recompute the brute-force answer against the FINAL occupancy (kings added),
            // since extra king squares can legitimately block a ray.
            let brute_force_final = piece_attacks(color, pt, attacker_sq, full_occ).contains(target) && board[attacker_sq as usize] == make_piece(color, pt);
            let _ = brute_force_hits;

            assert_eq!(
                via_attackers_to, brute_force_final,
                "attacker {color}:{pt} at {attacker_sq} vs target {target}: attackers_to={via_attackers_to} brute_force={brute_force_final}"
            );
        }
    }

    fn find_free_square(board: &[Piece; 81], a: Square, b: Square, c: Square) -> Square {
        for sq in 0u8..81 {
            if sq != a && sq != b && sq != c && is_none(board[sq as usize]) {
                return sq;
            }
        }
        panic!("no free square");
    }
}
