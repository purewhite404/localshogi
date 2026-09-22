// Smoke-tests the exact wasm-loading path www/engine.js and www/worker.js actually use
// (wasm-bindgen's async default-export `init()`, fetching shogi_engine_bg.wasm as a
// real Response) rather than the lower-level `initSync` path tools/selfplay-check.mjs
// exercises. Node has no http(s)/file fetch for local files by itself, so this
// polyfills fetch() for just the wasm URL with a real Response over the file's bytes
// — close enough to a browser's Response to exercise WebAssembly.instantiateStreaming
// the same way.
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const here = path.dirname(fileURLToPath(import.meta.url));
const pkgDir = path.join(here, '..', 'www', 'pkg');

const realFetch = globalThis.fetch;
globalThis.fetch = async (url) => {
  const u = String(url);
  if (u.endsWith('shogi_engine_bg.wasm')) {
    const bytes = await readFile(path.join(pkgDir, 'shogi_engine_bg.wasm'));
    return new Response(bytes, { status: 200, headers: { 'Content-Type': 'application/wasm' } });
  }
  return realFetch(url);
};

const mod = await import(path.join(pkgDir, 'shogi_engine.js'));
await mod.default();
const e = new mod.Engine(0);
if (e.board().length !== 81) throw new Error(`expected 81 board cells, got ${e.board().length}`);
if (e.legal_moves().length !== 30) throw new Error(`expected 30 legal moves at startpos, got ${e.legal_moves().length}`);
console.log('ok: async init() path (the one www/engine.js and www/worker.js use) loads and runs correctly');
