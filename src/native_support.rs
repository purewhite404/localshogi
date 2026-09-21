//! Small helpers shared by the native-only binaries (bench/perft/gen/train/match).
//! Kept out of the wasm build via `#[cfg(not(target_arch = "wasm32"))]` in lib.rs.
use crate::search::Clock;
use std::time::Instant;

pub struct StdClock(pub Instant);
impl StdClock {
    pub fn start() -> StdClock {
        StdClock(Instant::now())
    }
}
impl Clock for StdClock {
    fn now_ms(&self) -> f64 {
        self.0.elapsed().as_secs_f64() * 1000.0
    }
}
