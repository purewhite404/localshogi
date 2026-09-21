// DOM-free wasm facade: loading, packed-move helpers, and the compile-once /
// instantiate-twice (main thread + worker) pattern. Deliberately free of any `window`/
// `document` access so it can also be exercised from a plain Node script for testing.
import initSync, { Engine } from './pkg/shogi_engine.js';

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

const WASM_URL = new URL('./pkg/shogi_engine_bg.wasm', import.meta.url);

let cachedModulePromise = null;

async function loadModule() {
  if (cachedModulePromise) return cachedModulePromise;
  cachedModulePromise = (async () => {
    const resp = await fetch(WASM_URL);
    if (!resp.ok) throw new Error(`wasm fetch failed: ${resp.status}`);
    try {
      return await WebAssembly.compileStreaming(resp);
    } catch (e) {
      // Fallback for a dev server that serves .wasm with the wrong MIME type.
      const bytes = await resp.arrayBuffer();
      return await WebAssembly.compile(bytes);
    }
  })();
  return cachedModulePromise;
}

/** Loads the wasm module once and creates a main-thread rules-only Engine (no TT). */
export async function bootMainThread() {
  const module = await loadModule();
  initSync({ module });
  return { module, engine: new Engine(0) };
}

export function simd128Supported() {
  const SIMD_PROBE = new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0, 1, 5, 1, 96, 0, 1, 123, 3, 2, 1, 0, 10, 10, 1, 8, 0, 65, 0, 253, 15, 253, 98, 11]);
  try {
    return WebAssembly.validate(SIMD_PROBE);
  } catch {
    return false;
  }
}
