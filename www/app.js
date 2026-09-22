import { bootMainThread, simd128Supported, PIECES, PNAMES, HAND_ORDER, sqOf, rowOf, colOf, moveTo, moveFrom, moveIsPromotion, moveIsDrop, moveMovedType } from './engine.js';

const $ = (id) => document.getElementById(id);
const depthSelect = $('depth');
const sideSelect = $('side');
const evaluatorSelect = $('evaluator');
const restartBtn = $('restart-btn');
const statusEl = $('status');
const thinkingEl = $('thinking');
const evalLineEl = $('eval-line');
const pvLineEl = $('pv-line');
const loadingEl = $('loading');
const loadingTextEl = $('loading-text');
const promoDialog = $('promo-dialog');
const promoYesBtn = $('promo-yes');
const promoNoBtn = $('promo-no');
const toastEl = $('toast');

const state = {
  playerSide: 0,
  selected: null,
  selectedKind: null, // 'board' | 'hand'
  pendingPromo: null, // { from, to }
  gameOver: false,
  history: [],
  rootSfen: null,
  lastMoveSq: null,
  searchId: 0,
  searching: false,
};

let mainEngine = null;
let wasmModule = null;
let worker = null;
let workerReadyResolve = null;
let workerReadyPromise = null;

function spawnWorker(module) {
  workerReadyPromise = new Promise((resolve) => { workerReadyResolve = resolve; });
  const w = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
  w.onmessage = ({ data }) => onWorkerMessage(data);
  w.onerror = (e) => {
    console.error('worker error', e);
    thinkingEl.textContent = '';
    statusEl.textContent = 'AIでエラーが発生しました。最初からやり直してください。';
    state.gameOver = true;
  };
  w.onmessageerror = (e) => console.error('worker message error', e);
  w.postMessage({ t: 'init', module });
  return w;
}

function onWorkerMessage(data) {
  if (data.t === 'ready') {
    if (workerReadyResolve) workerReadyResolve();
    return;
  }
  if (data.t === 'error') {
    console.error('worker reported error', data.message);
    thinkingEl.textContent = '';
    if (data.id === state.searchId) {
      statusEl.textContent = '内部エラーが発生しました。最初からやり直してください。';
      state.gameOver = true;
      render();
    }
    return;
  }
  if (data.id !== state.searchId) return; // stale result from a superseded search
  if (data.t === 'info') {
    renderEngineInfo(data.scoreCp, data.mateIn, data.depth, data.nodes, data.elapsedMs, null);
    return;
  }
  if (data.t === 'bestmove') {
    handleAiResult(data);
  }
}

async function abortSearch() {
  state.searchId++;
  if (state.searching) {
    state.searching = false;
    thinkingEl.textContent = '';
    worker.terminate();
    worker = spawnWorker(wasmModule);
    await workerReadyPromise;
  }
}

function parseLimit() {
  const v = depthSelect.value;
  if (v[0] === 'd') return { depth: parseInt(v.slice(1), 10) };
  return { movetimeMs: parseInt(v.slice(1), 10) };
}

async function scheduleAiMove() {
  if (state.gameOver) return;
  const id = ++state.searchId;
  state.searching = true;
  thinkingEl.textContent = 'AIが考え中…';
  const historyBuf = Uint32Array.from(state.history);
  worker.postMessage({ t: 'search', id, rootSfen: state.rootSfen, history: historyBuf, limit: parseLimit(), evaluator: evaluatorSelect.value }, [historyBuf.buffer]);
}

function handleAiResult(data) {
  state.searching = false;
  thinkingEl.textContent = '';
  if (state.gameOver) return;
  if (mainEngine.side_to_move() === state.playerSide) return; // not the AI's turn any more
  const stmBeforeMove = mainEngine.side_to_move(); // the side the search score is relative to
  if (data.move && data.move !== 0) {
    try {
      mainEngine.do_move(data.move);
      state.history.push(data.move);
      state.lastMoveSq = moveTo(data.move);
    } catch (e) {
      console.error('AI returned an illegal move', e, data);
      statusEl.textContent = '内部エラーが発生しました。最初からやり直してください。';
      state.gameOver = true;
      render();
      return;
    }
  }
  renderEngineInfo(data.scoreCp, data.mateIn, data.depth, data.nodes, data.elapsedMs, data.pv, stmBeforeMove);
  clearSelection();
  render();
  finishTurn();
}

function renderEngineInfo(scoreCp, mateIn, depth, nodes, elapsedMs, pv, stmOverride) {
  // Score is always reported from the side-to-move's POV by the engine; convert to
  // sente's POV for display, matching every Japanese shogi GUI's convention. Callers
  // that have already applied the resulting move must pass the PRE-move side to move
  // explicitly, since mainEngine.side_to_move() has flipped by then.
  const stm = stmOverride !== undefined ? stmOverride : mainEngine.side_to_move();
  const senteScore = stm === 0 ? scoreCp : -scoreCp;
  if (mateIn && mateIn !== 0) {
    const winner = (mateIn > 0) === (stm === 0) ? '先手勝ち' : '後手勝ち';
    evalLineEl.textContent = `詰み ${Math.abs(mateIn)}手（${winner}）`;
  } else {
    const sign = senteScore > 0 ? '+' : '';
    const adv = senteScore > 30 ? '（先手有利）' : senteScore < -30 ? '（後手有利）' : '';
    evalLineEl.textContent = `評価値 ${sign}${senteScore}${adv}　深さ ${depth}　${nodes.toLocaleString()} ノード　${(elapsedMs / 1000).toFixed(2)} 秒`;
  }
  if (pv && pv.length) {
    // Rendered as USI text, not full Japanese notation: a faithful kanji PV would need
    // to replay each move on a scratch position to know disambiguation/成 context,
    // which isn't worth the complexity for an advisory line under the eval score.
    const text = pv.slice(0, 8).map((m, i) => {
      const side = (stm === 0) === (i % 2 === 0) ? '▲' : '△';
      return side + mainEngine.move_to_usi_str(m);
    });
    pvLineEl.textContent = '読み筋 ' + text.join(' ') + (pv.length > 8 ? ' …' : '');
  } else {
    pvLineEl.textContent = '';
  }
}

function isOwnPiece(cellByte, side) {
  if (cellByte === 0) return false;
  const owner = (cellByte & 0x80) ? 1 : 0;
  return owner === side;
}

function clearSelection() {
  state.selected = null;
  state.selectedKind = null;
  hidePromoDialog();
}

function hidePromoDialog() {
  state.pendingPromo = null;
  promoDialog.hidden = true;
}

let toastTimer = null;
function showToast(text) {
  toastEl.textContent = text;
  toastEl.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => toastEl.classList.remove('show'), 1800);
}

function reasonText(reason) {
  switch (reason) {
    case 1: return '二歩です';
    case 2: return '打ち歩詰めです';
    case 3: return 'その位置には打てません';
    default: return '指せません';
  }
}

function findBoardMove(moves, from, to, promote) {
  for (const mv of moves) {
    if (!moveIsDrop(mv) && moveFrom(mv) === from && moveTo(mv) === to && moveIsPromotion(mv) === promote) return mv;
  }
  return undefined;
}

function playMove(mv) {
  try {
    mainEngine.do_move(mv);
  } catch (e) {
    console.error('failed to apply move', e);
    return;
  }
  state.history.push(mv);
  state.lastMoveSq = moveTo(mv);
  clearSelection();
  render();
  finishTurn();
}

function finishTurn() {
  const status = mainEngine.status();
  const inCheck = (status & 1) !== 0;
  const noMoves = (status & 2) !== 0;
  const draw = (status & 4) !== 0;
  const perpetualWinForStm = (status & 8) !== 0;
  const perpetualLoseForStm = (status & 16) !== 0;
  const stm = mainEngine.side_to_move();
  const youMove = stm === state.playerSide;
  const stmName = stm === 0 ? '先手' : '後手';

  if (perpetualWinForStm || perpetualLoseForStm) {
    state.gameOver = true;
    // perpetualWinForStm: the side to move now wins (the OTHER side checked forever).
    const youWin = perpetualWinForStm ? youMove : !youMove;
    statusEl.textContent = (youWin ? 'あなた' : 'AI') + 'の勝ち！（連続王手の千日手）';
    render();
    return;
  }
  if (draw) {
    state.gameOver = true;
    statusEl.textContent = '千日手です。引き分け！';
    render();
    return;
  }
  if (noMoves) {
    state.gameOver = true;
    const winnerIsYou = !youMove; // the side with no moves loses; if it's not you, you win
    statusEl.textContent = (winnerIsYou ? 'あなたの勝ち！' : 'AIの勝ち！') + (inCheck ? '（詰み）' : '（手詰まり）');
    render();
    return;
  }
  const who = youMove ? `あなた（${stmName}）` : `AI（${stmName}）`;
  statusEl.textContent = who + 'の番です' + (inCheck ? ' — 王手！' : '');
  render();
  if (!youMove) scheduleAiMove();
}

function handleCellClick(sq) {
  if (state.gameOver || state.pendingPromo) return;
  if (mainEngine.side_to_move() !== state.playerSide) return;

  if (state.selectedKind === 'hand') {
    const pt = state.selected;
    const drops = mainEngine.drops_of(pt);
    const mv = drops.find((m) => moveTo(m) === sq);
    if (mv !== undefined) {
      playMove(mv);
    } else {
      const synth = sq | (1 << 15) | (pt << 16);
      showToast(reasonText(mainEngine.illegal_reason(synth)));
      clearSelection();
      render();
    }
    return;
  }

  const board = mainEngine.board();
  if (state.selectedKind === 'board') {
    if (state.selected === sq) {
      clearSelection();
      render();
      return;
    }
    const opts = mainEngine.move_options(state.selected, sq);
    if (opts === 1 || opts === 2) {
      const moves = mainEngine.moves_from(state.selected);
      const mv = findBoardMove(moves, state.selected, sq, opts === 2);
      if (mv !== undefined) playMove(mv);
      return;
    }
    if (opts === 3) {
      openPromoDialog(state.selected, sq);
      return;
    }
    if (isOwnPiece(board[sq], state.playerSide)) {
      state.selected = sq;
      render();
      return;
    }
    clearSelection();
    render();
    return;
  }

  if (isOwnPiece(board[sq], state.playerSide)) {
    state.selected = sq;
    state.selectedKind = 'board';
    render();
  }
}

function handleHandClick(pieceType, side) {
  if (state.gameOver || state.pendingPromo) return;
  if (mainEngine.side_to_move() !== state.playerSide || side !== state.playerSide) return;
  const hands = mainEngine.hands();
  const idx = HAND_ORDER.indexOf(pieceType);
  if (hands[(side === 0 ? 0 : 7) + idx] <= 0) return;
  state.selected = pieceType;
  state.selectedKind = 'hand';
  render();
}

function openPromoDialog(from, to) {
  state.pendingPromo = { from, to };
  render();
  const cells = document.querySelectorAll('#board td');
  const isFlipped = state.playerSide === 1;
  const dr = isFlipped ? 8 - rowOf(to) : rowOf(to);
  const dc = isFlipped ? 8 - colOf(to) : colOf(to);
  const idx = dr * 9 + dc;
  const td = cells[idx];
  const wrapRect = $('board-wrap').getBoundingClientRect();
  const rect = td.getBoundingClientRect();
  promoDialog.hidden = false;
  promoDialog.style.left = `${rect.left - wrapRect.left + rect.width / 2 - 40}px`;
  promoDialog.style.top = `${rect.top - wrapRect.top + rect.height + 4}px`;
  promoYesBtn.focus();
}

promoYesBtn.addEventListener('click', () => {
  const { from, to } = state.pendingPromo;
  hidePromoDialog();
  const moves = mainEngine.moves_from(from);
  const mv = findBoardMove(moves, from, to, true);
  if (mv !== undefined) playMove(mv);
});
promoNoBtn.addEventListener('click', () => {
  const { from, to } = state.pendingPromo;
  hidePromoDialog();
  const moves = mainEngine.moves_from(from);
  const mv = findBoardMove(moves, from, to, false);
  if (mv !== undefined) playMove(mv);
});
document.addEventListener('keydown', (e) => {
  if (!state.pendingPromo) return;
  if (e.key === 'Escape') {
    hidePromoDialog();
    clearSelection();
    render();
  } else if (e.key === 'Enter') {
    promoYesBtn.click();
  }
});
document.addEventListener('click', (e) => {
  if (!state.pendingPromo) return;
  if (promoDialog.contains(e.target)) return;
  if (e.target.closest('td')) return; // a board click is handled by its own listener
  hidePromoDialog();
  clearSelection();
  render();
});

function getHighlights() {
  if (state.pendingPromo) return [];
  if (state.selectedKind === 'board') return mainEngine.moves_from(state.selected).map(moveTo);
  if (state.selectedKind === 'hand') return mainEngine.drops_of(state.selected).map(moveTo);
  return [];
}

function render() {
  $('game-area').classList.toggle('gote-view', state.playerSide === 1);
  const board = mainEngine.board();
  const hands = mainEngine.hands();
  const highlights = new Set(getHighlights());
  const isFlipped = state.playerSide === 1;

  const tbl = $('board');
  tbl.innerHTML = '';
  for (let r = 0; r < 9; r++) {
    const tr = document.createElement('tr');
    for (let c = 0; c < 9; c++) {
      const dr = isFlipped ? 8 - r : r;
      const dc = isFlipped ? 8 - c : c;
      const sq = sqOf(dr, dc);
      const td = document.createElement('td');
      const cell = board[sq];
      if (cell) {
        const pt = cell & 0x7f;
        const isGote = (cell & 0x80) !== 0;
        const span = document.createElement('span');
        span.textContent = PNAMES[pt] || '?';
        span.className = (isGote ? 'piece-gote' : 'piece-sente') + (pt >= PIECES.TO ? ' piece-promoted' : '') + ((isGote ? 1 : 0) === state.playerSide ? '' : ' piece-reverse');
        td.appendChild(span);
      }
      const isSelected = state.selectedKind === 'board' && state.selected === sq;
      const isHL = highlights.has(sq);
      const isLast = state.lastMoveSq === sq;
      if (isSelected) td.classList.add('selected');
      else if (isHL) td.classList.add('highlight');
      else if (isLast) td.classList.add('last-move');
      td.addEventListener('click', () => handleCellClick(sq));
      tr.appendChild(td);
    }
    tbl.appendChild(tr);
  }

  renderHand('hand-sente', 0, hands);
  renderHand('hand-gote', 1, hands);
}

function renderHand(id, side, hands) {
  const el = $(id);
  el.innerHTML = '';
  const base = side === 0 ? 0 : 7;
  const order = [PIECES.HI, PIECES.KAKU, PIECES.KIN, PIECES.GIN, PIECES.KEI, PIECES.KYO, PIECES.FU];
  for (const pt of order) {
    const idx = HAND_ORDER.indexOf(pt);
    const cnt = hands[base + idx];
    if (cnt <= 0) continue;
    const div = document.createElement('div');
    div.className = 'hand-piece';
    if (state.selectedKind === 'hand' && state.selected === pt && side === state.playerSide) div.classList.add('selected');
    const span = document.createElement('span');
    span.textContent = PNAMES[pt];
    span.style.fontSize = '14px';
    if (side !== state.playerSide) {
      span.style.transform = 'rotate(180deg)';
      span.style.display = 'inline-block';
    }
    div.appendChild(span);
    if (cnt > 1) {
      const c = document.createElement('span');
      c.className = 'cnt';
      c.textContent = cnt;
      div.appendChild(c);
    }
    div.addEventListener('click', () => handleHandClick(pt, side));
    el.appendChild(div);
  }
}

function renderCoords() {
  const topDiv = $('coord-top');
  const sideDiv = $('coord-side');
  topDiv.innerHTML = '';
  sideDiv.innerHTML = '';
  for (let i = 9; i >= 1; i--) {
    const s = document.createElement('span');
    s.style.cssText = 'font-size:10px;color:var(--text-muted);width:44px;text-align:center;display:inline-block';
    s.textContent = i;
    topDiv.appendChild(s);
  }
  for (let i = 1; i <= 9; i++) {
    const d = document.createElement('div');
    d.style.cssText = 'font-size:10px;color:var(--text-muted);height:44px;display:flex;align-items:center;padding-left:4px';
    d.textContent = '一二三四五六七八九'.charAt(i - 1);
    sideDiv.appendChild(d);
  }
}

async function initGame() {
  await abortSearch();
  mainEngine.reset();
  state.rootSfen = mainEngine.sfen();
  state.history = [];
  state.playerSide = Number(sideSelect.value);
  state.selected = null;
  state.selectedKind = null;
  state.pendingPromo = null;
  state.gameOver = false;
  state.lastMoveSq = null;
  evalLineEl.textContent = '';
  pvLineEl.textContent = '';
  render();
  const stm = mainEngine.side_to_move();
  const you = state.playerSide === stm;
  statusEl.textContent = (you ? 'あなた' : 'AI') + `（${stm === 0 ? '先手' : '後手'}）の番です`;
  if (!you) scheduleAiMove();
}

restartBtn.addEventListener('click', () => { initGame(); });
sideSelect.addEventListener('change', () => { initGame(); });

async function boot() {
  if (!simd128Supported()) {
    loadingTextEl.textContent = 'お使いのブラウザは対応していません（WebAssembly SIMD が必要です）。';
    return;
  }
  try {
    const { module, engine } = await bootMainThread();
    wasmModule = module;
    mainEngine = engine;
    worker = spawnWorker(module);
    await workerReadyPromise;
  } catch (e) {
    console.error(e);
    const detail = e && e.message ? e.message : String(e);
    loadingTextEl.innerHTML = 'エンジンの読み込みに失敗しました。ページを再読み込みしてください。<br>'
      + `<span style="font-size:11px;opacity:0.7">${detail.replace(/[<>&]/g, (c) => ({ '<': '&lt;', '>': '&gt;', '&': '&amp;' }[c]))}</span>`;
    return;
  }
  renderCoords();
  loadingEl.hidden = true;
  restartBtn.disabled = false;
  await initGame();
}

boot();
