// Runs inside a module Worker. Owns its own Engine instance (with a real transposition
// table) so search state survives between moves. Never told "apply this move" directly
// — every search request carries the full {rootSfen, history}, so the worker's board
// can never silently diverge from the main thread's; it just resyncs its own do_move
// calls to match, replaying only the unmatched tail.
//
// Uses wasm-bindgen's own async `init()` (a fresh fetch+compile, not a Module shared
// from the main thread via structured clone) — see engine.js's top comment for why.
import init, { Engine } from './pkg/shogi_engine.js';

let engine = null;
let syncedRoot = null;
let syncedHistory = [];

function resync(rootSfen, history) {
  let common = 0;
  if (syncedRoot === rootSfen) {
    const n = Math.min(syncedHistory.length, history.length);
    while (common < n && syncedHistory[common] === history[common]) common++;
  }
  if (syncedRoot !== rootSfen || common < syncedHistory.length) {
    engine.set_sfen(rootSfen);
    syncedRoot = rootSfen;
    syncedHistory = [];
    common = 0;
  }
  for (let i = common; i < history.length; i++) {
    engine.do_move(history[i]);
    syncedHistory.push(history[i]);
  }
}

self.onmessage = async ({ data }) => {
  try {
    if (data.t === 'init') {
      await init();
      engine = new Engine(16);
      self.postMessage({ t: 'ready' });
      return;
    }
    if (data.t === 'search') {
      resync(data.rootSfen, Array.from(data.history));
      if (data.evaluator && engine.evaluator_name() !== data.evaluator) engine.set_evaluator(data.evaluator);
      const onIter = (depth, seldepth, scoreCp, mateIn, nodes, elapsedMs) => {
        self.postMessage({ t: 'info', id: data.id, depth, seldepth, scoreCp, mateIn, nodes, elapsedMs });
      };
      const limit = data.limit || {};
      const r = engine.search(limit.depth || 0, limit.movetimeMs || 0, onIter);
      self.postMessage({
        t: 'bestmove',
        id: data.id,
        move: r.best_move,
        scoreCp: r.score_cp,
        mateIn: r.mate_in,
        depth: r.depth,
        nodes: r.nodes,
        elapsedMs: r.elapsed_ms,
        aborted: r.aborted,
        pv: Array.from(r.pv()),
      });
      return;
    }
  } catch (e) {
    self.postMessage({ t: 'error', id: data.id ?? null, message: String(e && e.message ? e.message : e) });
  }
};
