// DOM-free wasm facade: loading and packed-move helpers. Deliberately free of any
// `window`/`document` access so it can also be exercised from a plain Node script
// for testing.
//
// Uses wasm-bindgen's own async default-export initializer (`init()`) rather than a
// hand-rolled compile+initSync path. An earlier version compiled the module once on
// the main thread and shared it with the worker via structured clone (avoiding a
// second download/compile) using `initSync()` for synchronous instantiation in both
// places — but a user hit a reproducible "wasm is undefined" failure inside the
// generated Engine constructor on the deployed site that survived a cache purge and
// hard reload, with the served .wasm/.js verified byte-identical to a working local
// build. The generated `initSync` path never threw, so whatever went wrong left no
// diagnosable trace. Falling back to the standard async `init()` path (which every
// wasm-bindgen `--target web` consumer actually exercises, unlike the synchronous
// variant) trades one extra wasm fetch+compile in the worker for eliminating that
// whole class of failure. The second fetch should be a cheap HTTP cache hit in
// practice given GitHub Pages' Cache-Control headers.
import init, { Engine } from './pkg/shogi_engine.js';

export const PIECES = { FU: 1, KYO: 2, KEI: 3, GIN: 4, KIN: 5, KAKU: 6, HI: 7, OU: 8, TO: 9, NKYO: 10, NKEI: 11, NGIN: 12, UMA: 13, RYU: 14 };
export const PNAMES = { 1: '歩', 2: '香', 3: '桂', 4: '銀', 5: '金', 6: '角', 7: '飛', 8: '玉', 9: 'と', 10: '杏', 11: '圭', 12: '全', 13: '馬', 14: '龍' };
export const HAND_ORDER = [PIECES.FU, PIECES.KYO, PIECES.KEI, PIECES.GIN, PIECES.KIN, PIECES.KAKU, PIECES.HI];

// square = col*9 + row, matching the Rust side exactly: row 0 = gote's back rank (top
// of screen), col 0 = leftmost on screen.
export const sqOf = (row, col) => col * 9 + row;
export const rowOf = (sq) => sq % 9;
export const colOf = (sq) => Math.floor(sq / 9);

// Packed move (u32): bits 0-6 to | 7-13 from | 14 promote | 15 drop | 16-20 moved pt | 21-25 captured pt
export const moveTo = (mv) => mv & 0x7f;
export const moveFrom = (mv) => (mv >>> 7) & 0x7f;
export const moveIsPromotion = (mv) => ((mv >>> 14) & 1) !== 0;
export const moveIsDrop = (mv) => ((mv >>> 15) & 1) !== 0;
export const moveMovedType = (mv) => (mv >>> 16) & 0x1f;
export const moveCapturedType = (mv) => (mv >>> 21) & 0x1f;

let initPromise = null;

/** Fetches and instantiates the wasm module (once; cached) via wasm-bindgen's own
 * async initializer, then creates a main-thread rules-only Engine (no TT). */
export async function bootMainThread() {
  if (!initPromise) initPromise = init();
  await initPromise;
  return { engine: new Engine(0) };
}

export function simd128Supported() {
  const SIMD_PROBE = new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0, 1, 5, 1, 96, 0, 1, 123, 3, 2, 1, 0, 10, 10, 1, 8, 0, 65, 0, 253, 15, 253, 98, 11]);
  try {
    return WebAssembly.validate(SIMD_PROBE);
  } catch {
    return false;
  }
}
