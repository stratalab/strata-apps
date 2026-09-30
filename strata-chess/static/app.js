// The table: the five, the board, and the tree of things you tried.
//
// The app holds no game state of its own. The position, the legal moves and
// the line all come back from `CHESS`, which owns the rules and the database;
// this decides what to draw and when to ask Stockfish what it thinks.

import { Engine } from './engine.js';
import { findPosition } from './generate.js';

const chess = window.CHESS;
const engine = new Engine();

const levelsEl = document.querySelector('[data-levels]');
const boardEl = document.querySelector('[data-board]');
const filesEl = document.querySelector('[data-files]');
const ranksEl = document.querySelector('[data-ranks]');
const statusEl = document.querySelector('[data-status]');
const goalEl = document.querySelector('[data-goal]');
const scoreEl = document.querySelector('[data-score]');

const FILES = ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h'];
// The solid glyphs for both sides; colour is a CSS matter.
const GLYPH = { k: '♚', q: '♛', r: '♜', b: '♝', n: '♞', p: '♟' };
const LEVELS = ['beginner', 'easy', 'medium', 'hard', 'grandmaster'];
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
let curated = [];

/** Says what is happening, in chess's words. */
function status(text, tone) {
  statusEl.textContent = text;
  if (tone) statusEl.dataset.tone = tone;
  else delete statusEl.dataset.tone;
}

function drawLevels() {
  levelsEl.replaceChildren();
  for (const level of LEVELS) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'level';
    button.textContent = level;
    button.setAttribute('aria-current', String(scenario?.level === level));
    button.addEventListener('click', () => {
      if (busy) return;
      const first = curated.find((s) => s.level === level);
      if (first) start(first);
    });
    levelsEl.append(button);
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
  goalEl.textContent = chosen.goal;
  drawLevels();
  status(state.turn === 'w' ? 'White to play' : 'Black to play');
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
  drawScore();
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

/* The score sheet.
 *
 * Ruled rows, White and Black in their own columns, and variations written
 * indented under the move they leave. A bulletin always prints the main line
 * as the score and the alternatives beneath it, which is also what keeps a
 * way back visible: the first cut rendered whichever line you were on, so
 * stepping into a variation made the game itself disappear.
 */
function drawScore() {
  const lines = JSON.parse(chess.lines_detail());
  const main = lines.find((l) => l.name === 'default') ?? { name: 'default', moves: [] };
  const others = lines.filter((l) => l.name !== 'default');
  const at = viewingPly === null ? state.line.length : viewingPly;

  scoreEl.replaceChildren();

  const variationsFrom = (ply) => {
    for (const other of others) {
      if (other.from !== ply) continue;
      scoreEl.append(variationRow(other, at));
    }
  };

  variationsFrom(0);

  for (let i = 0; i < main.moves.length; i += 2) {
    const li = document.createElement('li');
    const no = document.createElement('span');
    no.className = 'no';
    no.textContent = `${Math.floor(i / 2) + 1}.`;
    li.append(no);

    for (const j of [i, i + 1]) {
      if (main.moves[j] === undefined) {
        li.append(document.createElement('span'));
        continue;
      }
      const button = document.createElement('button');
      button.type = 'button';
      button.className = 'move';
      button.textContent = main.moves[j];
      button.dataset.here = String(state.on === 'default' && j + 1 === at);
      button.addEventListener('click', () => goToMainPly(j + 1));
      li.append(button);
    }
    scoreEl.append(li);
    variationsFrom(i + 1);
    variationsFrom(i + 2);
  }

  if (main.moves.length === 0 && others.length === 0) {
    const li = document.createElement('li');
    const no = document.createElement('span');
    no.className = 'no';
    no.textContent = '1.';
    li.append(no, document.createElement('span'), document.createElement('span'));
    scoreEl.append(li);
  }
}

/** A variation, indented. The one being played is marked and spelled out. */
function variationRow(other, at) {
  const li = document.createElement('li');
  li.className = 'variation';
  li.append(document.createElement('span'));

  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'line';
  const live = other.name === state.on;
  const shown = live ? other.moves : other.moves.slice(0, 4);
  const text = shown.join(' ');
  button.textContent = other.moves.length
    ? `(${text}${!live && other.moves.length > 4 ? '…' : ''})`
    : '(…)';
  button.setAttribute('aria-current', String(live));
  button.addEventListener('click', () => {
    if (busy) return;
    state = JSON.parse(chess.use_line(other.name));
    selected = null;
    viewingPly = null;
    status(state.turn === 'w' ? 'White to play' : 'Black to play');
    draw();
  });
  li.append(button);
  void at;
  return li;
}

/** Steps to a point on the main line, from wherever you are. */
function goToMainPly(ply) {
  if (busy) return;
  if (state.on !== 'default') {
    state = JSON.parse(chess.use_line('default'));
    selected = null;
  }
  rewindTo(ply);
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
    viewingPly = null;
    await makeMove(selected, name, true);
  } catch (error) {
    status(String(error?.message ?? error), 'bad');
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
  status(
    viewingPly === null
      ? 'At the end of the line'
      : 'Earlier in the game. Play a different move to take it another way.',
  );
  draw();
}

async function makeMove(from, to, alreadyBusy = false) {
  if (!alreadyBusy) busy = true;
  selected = null;
  const before = await evaluate(state.fen);
  try {
    state = JSON.parse(chess.play(from, to, 'q'));
  } catch (error) {
    status(String(error?.message ?? error), 'bad');
    busy = false;
    draw();
    return;
  }
  const played = state.line[state.line.length - 1];
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
    status(`${played.san} lets it go. Go back and try another.`, 'bad');
  } else {
    status(`${played.san}. ${describe(mine)}`);
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
      status('Checkmate. That is the one.', 'good');
    } else {
      status('Checkmate against you. Go back and try another.', 'bad');
    }
    return true;
  }
  if (state.status === 'draw' && scenario.win.kind !== 'hold') {
    status('A draw. The position was worth more than that.', 'bad');
    return true;
  }

  const win = scenario.win;
  if (win.kind === 'promote') {
    const promoted = state.line.some((p) => p.by === scenario.you && p.uci.length === 5);
    if (promoted) {
      status('A new queen. That is the one.', 'good');
      return true;
    }
  }
  if (win.kind === 'material' && materialGained() >= win.cp) {
    status('Won. That is the one.', 'good');
    return true;
  }
  if (win.kind === 'hold' && state.line.length >= win.plies) {
    const found = await evaluate(state.fen);
    const mine = youMoved ? -toCp(found) : toCp(found);
    if (Math.abs(mine) <= win.band) {
      status('Held. That is the one.', 'good');
      return true;
    }
  }
  return false;
}

/* ---------- rail ---------- */

/* Back to the position as it was handed over.
 *
 * The move buttons reach every ply but the first, and ply 0 is the one people
 * actually want: it is where a first try went wrong. Playing a different move
 * from here takes the game another way and leaves the old one written down. */
document.querySelector('[data-restart]').addEventListener('click', () => {
  if (busy) return;
  if (state.on !== 'default') {
    state = JSON.parse(chess.use_line('default'));
  }
  selected = null;
  rewindTo(0);
});

document.querySelector('[data-hint]').addEventListener('click', () => {
  status(scenario.hint);
});

// The generator writes its own hint from what the engine found, so a made
// position is never left without one.


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
  status('Finding a position');
  try {
    const found = await findPosition({
      chess,
      engine,
      level,
      from: nextSeed,
      depth: 12,
      tries: 260,
      onTry: (n) => {
        if (n % 10 === 0) status(`Finding a position, ${n} tried`);
      },
    });
    if (!found) {
      status('Nothing turned up this time. Try again.', 'bad');
      return;
    }
    nextSeed = found.seed + 1;
    busy = false;
    start({ ...found, generated: true });
  } catch (error) {
    status(String(error?.message ?? error), 'bad');
  } finally {
    busy = false;
  }
});

/* The five curated positions open each level; anything after that is made.
   The board is never empty: the page opens on one. */
curated = JSON.parse(chess.scenarios());
drawLevels();
start(curated[0]);
