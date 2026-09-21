//! Shogi engine core: bitboard position representation, legal move generation,
//! search, and (feature-gated) NNUE evaluation. Compiles both to wasm32 (browser)
//! and natively (data generation / training / match tools).
pub mod bitboard;
pub mod hand;
pub mod mov;
pub mod movegen;
pub mod perft;
pub mod position;
pub mod sfen;
pub mod tables;
pub mod tt;
pub mod types;
pub mod zobrist;

pub mod eval;
pub mod search;

#[cfg(not(target_arch = "wasm32"))]
pub mod native_support;

#[cfg(target_arch = "wasm32")]
pub mod wasm_api;
