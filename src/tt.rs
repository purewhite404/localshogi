//! Transposition table. Single-slot-per-index (no clustering) with a full 64-bit key
//! comparison and a full `Move` stored (not the 16-bit "core" form some engines use) —
//! a deliberate simplicity-over-density tradeoff: it costs more memory per entry but
//! needs no move-reconstruction/validation logic against the current board.
use crate::mov::{Move, MOVE_NONE};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bound {
    None,
    Exact,
    Lower,
    Upper,
}

#[derive(Clone, Copy)]
pub struct TTEntry {
    pub key: u64,
    pub mv: Move,
    pub score: i32,
    pub eval: i32,
    pub depth: i32,
    pub bound: Bound,
}

impl Default for TTEntry {
    fn default() -> Self {
        TTEntry { key: 0, mv: MOVE_NONE, score: 0, eval: 0, depth: -1, bound: Bound::None }
    }
}

pub struct TranspositionTable {
    entries: Vec<TTEntry>,
    mask: usize,
}

impl TranspositionTable {
    pub fn new(mb: usize) -> TranspositionTable {
        let bytes = mb.max(1) * 1024 * 1024;
        let entry_size = std::mem::size_of::<TTEntry>().max(1);
        let mut n = (bytes / entry_size).max(1024);
        n = n.next_power_of_two() / 2; // keep it a power of two, at least halved for safety
        if n == 0 {
            n = 1024;
        }
        TranspositionTable { entries: vec![TTEntry::default(); n], mask: n - 1 }
    }

    #[inline]
    fn index(&self, key: u64) -> usize {
        (key as usize) & self.mask
    }

    pub fn probe(&self, key: u64) -> Option<TTEntry> {
        let e = self.entries[self.index(key)];
        if e.bound != Bound::None && e.key == key {
            Some(e)
        } else {
            None
        }
    }

    pub fn store(&mut self, key: u64, mv: Move, score: i32, eval: i32, depth: i32, bound: Bound) {
        let idx = self.index(key);
        let slot = &mut self.entries[idx];
        // Keep the existing entry if it's for a different, deeper search unless this
        // is a fresh position for the slot or an exact score (always worth keeping).
        if slot.bound != Bound::None && slot.key == key && slot.depth > depth && bound != Bound::Exact {
            return;
        }
        let mv = if mv.is_none() && slot.key == key { slot.mv } else { mv };
        *slot = TTEntry { key, mv, score, eval, depth, bound };
    }

    pub fn clear(&mut self) {
        for e in self.entries.iter_mut() {
            *e = TTEntry::default();
        }
    }
}

pub const MATE: i32 = 30000;
pub const MATE_IN_MAX_PLY: i32 = MATE - 1000;

#[inline]
pub fn mate_in(ply: u32) -> i32 {
    MATE - ply as i32
}

#[inline]
pub fn mated_in(ply: u32) -> i32 {
    -MATE + ply as i32
}

/// Adjust a mate score for storage (ply-from-root -> ply-from-this-node), so a mate
/// score found at a different depth in the tree is still meaningful when reused.
pub fn score_to_tt(score: i32, ply: u32) -> i32 {
    if score >= MATE_IN_MAX_PLY {
        score + ply as i32
    } else if score <= -MATE_IN_MAX_PLY {
        score - ply as i32
    } else {
        score
    }
}

pub fn score_from_tt(score: i32, ply: u32) -> i32 {
    if score >= MATE_IN_MAX_PLY {
        score - ply as i32
    } else if score <= -MATE_IN_MAX_PLY {
        score + ply as i32
    } else {
        score
    }
}
