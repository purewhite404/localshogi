// Node-only integration harness: exercises the real engine.js/pkg output the way the
// browser UI does, without a browser. Plays random legal games and, at every ply,
// cross-checks that the union of per-square/per-hand-piece queries (what the UI's
// highlighting uses) exactly matches the full legal_moves() list — this is exactly the
// bug class that would show up in the browser as a phantom or missing highlight.
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

globalThis.performance ??= { now: () => Number(process.hrtime.bigint() / 1000000n) };

const here = path.dirname(fileURLToPath(import.meta.url));
const pkgDir = path.join(here, '..', 'www', 'pkg');

const initModule = await import(path.join(pkgDir, 'shogi_engine.js'));
const bytes = await readFile(path.join(pkgDir, 'shogi_engine_bg.wasm'));
const mod = new WebAssembly.Module(bytes);
initModule.initSync({ module: mod });

const { Engine } = initModule;
const HAND_ORDER = [1, 2, 3, 4, 5, 6, 7]; // FU KYO KEI GIN KIN KAKU HI

function moveTo(mv) { return mv & 0x7f; }
function moveFrom(mv) { return (mv >>> 7) & 0x7f; }
function moveIsDrop(mv) { return ((mv >>> 15) & 1) !== 0; }
function moveMovedType(mv) { return (mv >>> 16) & 0x1f; }

function assertSetsEqual(a, b, msg) {
  const sa = new Set(a);
  const sb = new Set(b);
  if (sa.size !== sb.size || ![...sa].every((x) => sb.has(x))) {
    throw new Error(`${msg}: mismatch — a has ${sa.size} b has ${sb.size}`);
  }
}

function xorshift(seed) {
  let s = seed >>> 0 || 1;
  return () => {
    s ^= s << 13; s >>>= 0;
    s ^= s >>> 17;
    s ^= s << 5; s >>>= 0;
    return s / 4294967296;
  };
}

const GAMES = parseInt(process.env.SELFPLAY_GAMES || '20', 10);
const MAX_PLIES = parseInt(process.env.SELFPLAY_PLIES || '200', 10);
const rng = xorshift(20260921);

let totalPlies = 0;
for (let g = 0; g < GAMES; g++) {
  const engine = new Engine(1);
  for (let ply = 0; ply < MAX_PLIES; ply++) {
    const status = engine.status();
    if (status & (2 | 4 | 8 | 16)) break; // no moves / draw / perpetual either way

    const moves = engine.legal_moves();
    if (moves.length === 0) throw new Error('status() said moves exist but legal_moves() is empty');

    // Cross-check 1: union of per-square/per-hand-piece queries == full legal_moves().
    const board = engine.board();
    const fromSquares = new Set();
    const handTypes = new Set();
    for (const mv of moves) {
      if (moveIsDrop(mv)) handTypes.add(moveMovedType(mv));
      else fromSquares.add(moveFrom(mv));
    }
    let unionMoves = [];
    for (const sq of fromSquares) unionMoves.push(...engine.moves_from(sq));
    for (const pt of handTypes) unionMoves.push(...engine.drops_of(pt));
    assertSetsEqual(unionMoves, moves, `game ${g} ply ${ply}: per-square/hand union vs legal_moves`);

    // Cross-check 2: move_options/drop_option agree with legal_moves for every move.
    for (const mv of moves) {
      if (moveIsDrop(mv)) {
        if (!engine.drop_option(moveMovedType(mv), moveTo(mv))) {
          throw new Error(`game ${g} ply ${ply}: drop_option disagrees with legal_moves for ${mv}`);
        }
      } else {
        const opts = engine.move_options(moveFrom(mv), moveTo(mv));
        if (opts === 0) throw new Error(`game ${g} ply ${ply}: move_options disagrees with legal_moves for ${mv}`);
      }
    }

    const mv = moves[Math.floor(rng() * moves.length)];
    const kif = engine.do_move(mv);
    if (!kif || kif.length === 0) throw new Error(`do_move returned empty notation at game ${g} ply ${ply}`);
    totalPlies++;
  }
}
console.log(`ok: ${GAMES} games, ${totalPlies} plies checked`);
