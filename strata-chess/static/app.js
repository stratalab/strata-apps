// The table: the five, the board, and the tree of things you tried.
//
// The app holds no game state of its own. The position, the legal moves and
// the line all come back from `CHESS`, which owns the rules and the database;
// this decides what to draw and when to ask Stockfish what it thinks.

import { Engine } from './engine.js';
import { findPosition } from './generate.js';

const chess = window.CHESS;
const engine = new Engine();

const picker = document.querySelector('[data-picker]');
const table = document.querySelector('[data-table]');
const boardEl = document.querySelector('[data-board]');
const filesEl = document.querySelector('[data-files]');
const ranksEl = document.querySelector('[data-ranks]');
const verdictEl = document.querySelector('[data-verdict]');
const linesEl = document.querySelector('[data-lines]');
const movesEl = document.querySelector('[data-moves]');
const callEl = document.querySelector('[data-call]');

const FILES = ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h'];
// The solid glyphs for both sides; colour is a CSS matter.
const GLYPH = { k: '♚', q: '♛', r: '♜', b: '♝', n: '♞', p: '♟' };
/** How hard the engine thinks. The lite build reaches this in well under a second. */
const DEPTH = 16;
/** A drop this big, in centipawns, is a move that threw the position away. */
const SLIP = 180;

let scenario = null;
let state = null;
let selected = null;
/** How far back into the line the board is showing, or null for the end. */
let viewingPly = null;
let busy = false;

function say(html) {
  callEl.innerHTML = html;
}

function verdict(text, tone) {
  verdictEl.textContent = text;
  if (tone) verdictEl.dataset.tone = tone;
  else delete verdictEl.dataset.tone;
}

/* ---------- the five ---------- */

function drawPicker() {
  const all = JSON.parse(chess.scenarios());
  picker.replaceChildren();
  for (const s of all) {
    const card = document.createElement('button');
    card.type = 'button';
    card.className = 'card';
    card.innerHTML = `
      <p class="level">${s.level}</p>
      <h2>${s.title}</h2>
      <p class="brief">${s.brief}</p>
      <p class="goal">${s.goal}</p>`;
    card.addEventListener('click', () => start(s));
    picker.append(card);
  }
}

function start(chosen) {
  scenario = chosen;
  // A curated position is loaded by name; a generated one carries its own FEN,
  // because it exists nowhere but this tab.
  state = JSON.parse(
    chosen.generated ? chess.load_fen(chosen.id, chosen.fen) : chess.load(chosen.id),
  );
  selected = null;
  viewingPly = null;
  picker.hidden = true;
  table.hidden = false;
  document.querySelector('[data-level]').textContent = chosen.level;
  document.querySelector('[data-title]').textContent = chosen.title;
  document.querySelector('[data-goal]').textContent = chosen.goal;
  document.querySelector('[data-brief]').textContent = chosen.brief;
  verdict('Your move.');
  say(`<b>opened</b> a database for ${chosen.id} — every move from here is a commit`);
  draw();
}

/* ---------- the board ---------- */

/** The placement field of a FEN, as 64 squares from a8 to h1. */
function unpack(fen) {
  const rows = fen.split(' ')[0].split('/');
  const out = [];
  for (const row of rows) {
    for (const ch of row) {
      if (/\d/.test(ch)) out.push(...Array(Number(ch)).fill(null));
      else out.push(ch);
    }
  }
  return out;
}

function draw() {
  const squares = unpack(state.fen);
  const legal = viewingPly === null ? state.legal : [];
  const targets = selected ? legal.filter((m) => m.from === selected) : [];
  const last = lastMove();

  boardEl.replaceChildren();
  for (let i = 0; i < 64; i++) {
    const file = FILES[i % 8];
    const rank = 8 - Math.floor(i / 8);
    const name = `${file}${rank}`;
    const piece = squares[i];

    const sq = document.createElement('button');
    sq.type = 'button';
    sq.className = `sq ${(Math.floor(i / 8) + i) % 2 === 0 ? 'light' : 'dark'}`;
    sq.dataset.square = name;
    sq.setAttribute('aria-label', piece ? `${name}, ${piece}` : name);

    if (piece) {
      const span = document.createElement('span');
      span.className = `piece ${piece === piece.toUpperCase() ? 'w' : 'b'}`;
      span.textContent = GLYPH[piece.toLowerCase()];
      sq.append(span);
    }
    if (selected === name) sq.dataset.selected = '';
    if (targets.some((m) => m.to === name)) {
      sq.dataset.target = '';
      if (piece) sq.dataset.occupied = '';
    }
    if (last && (last.from === name || last.to === name)) sq.dataset.last = '';
    if (state.status === 'check' || state.status === 'checkmate') {
      const king = state.turn === 'w' ? 'K' : 'k';
      if (piece === king) sq.dataset.check = '';
    }
    boardEl.append(sq);
  }

  filesEl.replaceChildren(...FILES.map((f) => el('span', f)));
  ranksEl.replaceChildren(...[8, 7, 6, 5, 4, 3, 2, 1].map((r) => el('span', String(r))));
  drawLines();
  drawMoves();
}

function el(tag, text) {
  const node = document.createElement(tag);
  node.textContent = text;
  return node;
}

function lastMove() {
  const line = state.line;
  const at = viewingPly === null ? line.length : viewingPly;
  const ply = line[at - 1];
  if (!ply) return null;
  return { from: ply.uci.slice(0, 2), to: ply.uci.slice(2, 4) };
}

function drawLines() {
  linesEl.replaceChildren();
  for (const name of state.lines) {
    const li = document.createElement('li');
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'line-btn';
    button.textContent = name === 'default' ? 'main line' : name.replace('line-', 'from move ');
    button.setAttribute('aria-current', String(name === state.on));
    button.addEventListener('click', () => {
      if (name === state.on) return;
      state = JSON.parse(chess.use_line(name));
      selected = null;
      viewingPly = null;
      verdict(name === 'default' ? 'Back on the main line.' : 'On a variation.');
      say(`<b>branch</b> ${name} — the line it came from still holds everything it had`);
      draw();
    });
    li.append(button);
    linesEl.append(li);
  }
}

function drawMoves() {
  movesEl.replaceChildren();
  const line = state.line;

  // The position you were handed. Without it there is no way back to before
  // your first move, which is exactly where a first try usually goes wrong.
  movesEl.append(el('span', ''));
  const startBtn = document.createElement('button');
  startBtn.type = 'button';
  startBtn.className = 'ply';
  startBtn.textContent = 'start';
  startBtn.dataset.at = String(viewingPly === 0);
  startBtn.addEventListener('click', () => rewindTo(0));
  movesEl.append(startBtn);
  movesEl.append(el('span', ''));

  for (let i = 0; i < line.length; i += 2) {
    movesEl.append(el('span', `${Math.floor(i / 2) + 1}.`));
    movesEl.firstChild?.classList?.add?.('move-no');
    for (const j of [i, i + 1]) {
      if (!line[j]) {
        movesEl.append(el('span', ''));
        continue;
      }
      const button = document.createElement('button');
      button.type = 'button';
      button.className = 'ply';
      button.textContent = line[j].san;
      const at = viewingPly === null ? line.length : viewingPly;
      button.dataset.at = String(j + 1 === at);
      button.addEventListener('click', () => rewindTo(j + 1));
      movesEl.append(button);
    }
  }
  for (const node of movesEl.querySelectorAll('span')) {
    if (/^\d+\.$/.test(node.textContent)) node.className = 'move-no';
  }
}

/* ---------- playing ---------- */

boardEl.addEventListener('click', (event) => {
  if (busy) return;
  const sq = event.target.closest('[data-square]');
  if (!sq) return;
  const name = sq.dataset.square;

  if (viewingPly !== null) {
    // The board is showing an earlier position. Playing from here is a
    // variation, so take the branch first and then make the move.
    void playFromPast(name);
    return;
  }

  const move = selected && state.legal.find((m) => m.from === selected && m.to === name);
  if (move) {
    void makeMove(selected, name);
    return;
  }
  selected = state.legal.some((m) => m.from === name) ? name : null;
  draw();
});

async function playFromPast(name) {
  const move = selected && legalAtView().find((m) => m.from === selected && m.to === name);
  if (!move) {
    selected = legalAtView().some((m) => m.from === name) ? name : null;
    draw();
    return;
  }
  const at = viewingPly;
  busy = true;
  try {
    state = JSON.parse(chess.branch(at));
    say(`<b>branch fork</b> ${state.on} at move ${at} — the line you left keeps its moves`);
    viewingPly = null;
    await makeMove(selected, name, true);
  } catch (error) {
    verdict(String(error?.message ?? error), 'bad');
  } finally {
    busy = false;
  }
}

/** Legal moves for the position currently on the board, even in the past. */
function legalAtView() {
  return state.legal;
}

function rewindTo(ply) {
  const line = state.line;
  viewingPly = ply >= line.length && ply !== 0 ? null : ply;
  selected = null;
  state = JSON.parse(chess.rewind(viewingPly === null ? line.length : viewingPly));
  verdict(
    viewingPly === null
      ? 'Back at the end of the line.'
      : 'Looking at an earlier position. Play a different move to branch here.',
  );
  say(`<b>read</b> the line at move ${viewingPly ?? line.length}`);
  draw();
}

async function makeMove(from, to, alreadyBusy = false) {
  if (!alreadyBusy) busy = true;
  selected = null;
  const before = await evaluate(state.fen);
  try {
    state = JSON.parse(chess.play(from, to, 'q'));
  } catch (error) {
    verdict(String(error?.message ?? error), 'bad');
    busy = false;
    draw();
    return;
  }
  const played = state.line[state.line.length - 1];
  say(`<b>kv put</b> move/${String(state.line.length).padStart(4, '0')} — ${played.san}`);
  draw();

  if (await settled()) {
    busy = false;
    return;
  }

  const after = await evaluate(state.fen);
  // UCI scores are from the side to move. After your move it is the opponent's
  // turn, so their advantage negated is yours.
  const mine = -toCp(after);
  const was = toCp(before);
  if (mine < was - SLIP) {
    verdict(`${played.san} lets it slip. Click an earlier move and try another.`, 'bad');
  } else {
    verdict(`${played.san}. ${describe(mine)}`);
  }

  await reply();
  busy = false;
}

async function reply() {
  if (state.status === 'checkmate' || state.status === 'draw') return;
  const found = await engine.analyse(state.fen, DEPTH);
  if (!found.best || found.best === '(none)') return;
  const from = found.best.slice(0, 2);
  const to = found.best.slice(2, 4);
  const promo = found.best.slice(4, 5) || 'q';
  try {
    state = JSON.parse(chess.play(from, to, promo));
  } catch {
    return;
  }
  const played = state.line[state.line.length - 1];
  say(`<b>kv put</b> move/${String(state.line.length).padStart(4, '0')} — ${played.san} (Stockfish)`);
  draw();
  await settled();
}

/* Material actually on the board, in centipawns, from the visitor's side.
 *
 * The evaluation cannot answer "have you won the rook yet": a position with a
 * fork available is already +8, so checking the score marked the scenario
 * solved before the fork had been played. Counting pieces asks the question
 * that was actually set. */
const VALUE = { p: 100, n: 300, b: 300, r: 500, q: 900, k: 0 };

function balance(fen) {
  let total = 0;
  for (const ch of fen.split(' ')[0]) {
    const value = VALUE[ch.toLowerCase()];
    if (value === undefined) continue;
    const mine = (ch === ch.toUpperCase()) === (scenario.you === 'w');
    total += mine ? value : -value;
  }
  return total;
}

function materialGained() {
  return balance(state.fen) - balance(scenario.fen);
}

function toCp(found) {
  if (found.kind === 'mate') return found.value > 0 ? 10000 : -10000;
  return found.value;
}

function describe(cp) {
  if (cp > 600) return 'Winning.';
  if (cp > 150) return 'Better for you.';
  if (cp > -150) return 'Level.';
  return 'Worse for you.';
}

async function evaluate(fen) {
  try {
    return await engine.analyse(fen, DEPTH);
  } catch {
    return { kind: 'cp', value: 0, best: null };
  }
}

/** Has the scenario been solved, or lost? */
async function settled() {
  const youMoved = state.turn !== scenario.you;
  if (state.status === 'checkmate') {
    if (youMoved) {
      verdict('Checkmate. Solved.', 'good');
      say('<b>solved</b> — and every line you tried is still in the database');
    } else {
      verdict('Checkmate against you. Rewind and try another move.', 'bad');
    }
    return true;
  }
  if (state.status === 'draw' && scenario.win.kind !== 'hold') {
    verdict('Drawn. Not what the position was worth.', 'bad');
    return true;
  }

  const win = scenario.win;
  if (win.kind === 'promote') {
    const promoted = state.line.some((p) => p.by === scenario.you && p.uci.length === 5);
    if (promoted) {
      verdict('Promoted. Solved.', 'good');
      return true;
    }
  }
  if (win.kind === 'material' && materialGained() >= win.cp) {
    verdict('The material is yours. Solved.', 'good');
    say('<b>solved</b> — and the lines you rejected are still on the tree');
    return true;
  }
  if (win.kind === 'hold' && state.line.length >= win.plies) {
    const found = await evaluate(state.fen);
    const mine = youMoved ? -toCp(found) : toCp(found);
    if (Math.abs(mine) <= win.band) {
      verdict('Held. Solved.', 'good');
      return true;
    }
  }
  return false;
}

/* ---------- rail ---------- */

document.querySelector('[data-hint]').addEventListener('click', () => {
  verdict(scenario.hint);
});

/* Another position at this level.
 *
 * Made here and now: the Rust side lays out a legal position to the level's
 * recipe and the engine says whether it is worth solving, which is a question
 * neither of them can answer alone. Most seeds are thrown away, so this says
 * what it is doing rather than appearing to have stopped. */
let nextSeed = Math.floor(Math.random() * 100000) + 1;

document.querySelector('[data-another]').addEventListener('click', async () => {
  if (busy) return;
  busy = true;
  const level = scenario.level;
  verdict('Looking for a position…');
  say('<b>generating</b> — legal positions here, the engine grading them');
  try {
    const found = await findPosition({
      chess,
      engine,
      level,
      from: nextSeed,
      depth: 12,
      tries: 260,
      onTry: (n) => {
        if (n % 10 === 0) verdict(`Looking for a position… ${n} tried`);
      },
    });
    if (!found) {
      verdict('Could not find one this time. Try again.', 'bad');
      return;
    }
    nextSeed = found.seed + 1;
    busy = false;
    start({ ...found, generated: true });
    say(`<b>generated</b> ${found.level} — ${found.goal.toLowerCase()}, seed ${found.seed}`);
  } catch (error) {
    verdict(String(error?.message ?? error), 'bad');
  } finally {
    busy = false;
  }
});

document.querySelector('[data-back]').addEventListener('click', () => {
  table.hidden = true;
  picker.hidden = false;
  verdict('');
  say('&nbsp;');
});

drawPicker();
