// The canvas, the tools, and the scrubber.
//
// The app holds no history of its own. Everything the timeline shows came back
// from `PAINT.timeline()`, and every frame of a scrub came back from
// `PAINT.canvas_at()`. That is the point of the demo, and it is also why the
// code is short: there is no undo stack to keep in step with anything.

const paint = window.PAINT;

const board = document.querySelector('[data-board]');
const ctx = board.getContext('2d');
const app = document.querySelector('.app');
const scrub = document.querySelector('[data-scrub]');
const ticks = document.querySelector('[data-ticks]');
const fill = document.querySelector('[data-fill]');
const atLabel = document.querySelector('[data-at]');
const counts = document.querySelector('[data-counts]');
const callOut = document.querySelector('[data-call]');
const playButton = document.querySelector('[data-play]');
const pastFlag = document.querySelector('[data-past-flag]');
const sizeInput = document.querySelector('[data-size]');
const sizeDot = document.querySelector('[data-size-dot]');
const eraseButton = document.querySelector('[data-erase]');
const branchBar = document.querySelector('[data-branches]');

const PALETTE = ['#f2f4f7', '#ff7a52', '#5bc8d6', '#ffc857', '#6fcf7f', '#a78bfa', '#f472b6'];

let colour = PALETTE[0];
let width = Number(sizeInput.value);
let erasing = false;

/** The commit list, exactly as the database gave it. */
let timeline = [];
/** null means "now"; otherwise an index into `timeline`. */
let viewing = null;
/** What is on screen, so a resize can repaint without asking again. */
let shown = [];

let drawing = null;
let playTimer = null;
/** True while a finger is down on the scrubber. */
let scrubbing = false;
/** The commit index last handed to the database, so a drag does not re-ask. */
let showing = null;

/* ---------- the database ---------- */

function say(html) {
  callOut.innerHTML = html;
}

/* The timelines, and which one the brush is on.
 *
 * The chips stay out of the way until there is a second timeline: an app
 * nobody has forked should not be explaining branches to anyone. */
function refreshBranches() {
  const names = JSON.parse(paint.branches());
  const on = paint.branch();
  branchBar.hidden = names.length < 2;
  branchBar.replaceChildren();
  if (names.length < 2) return;
  for (const name of names) {
    const chip = document.createElement('button');
    chip.type = 'button';
    chip.className = 'branch';
    chip.textContent = name;
    chip.setAttribute('aria-current', String(name === on));
    chip.addEventListener('click', () => {
      if (name === on) return;
      stopPlaying();
      paint.use_branch(name);
      openBranch();
      say(`<b>now painting on</b> ${name}`);
    });
    branchBar.append(chip);
  }
}

/** Loads whichever timeline the engine says we are on, at its head. */
function openBranch() {
  viewing = null;
  showing = null;
  loadTimeline();
  render(JSON.parse(paint.canvas()));
  refreshBranches();
}

function refreshCounts() {
  const n = timeline.length;
  counts.textContent = n === 0 ? 'no strokes yet' : `${n} stroke${n === 1 ? '' : 's'} · ${n} commit${n === 1 ? '' : 's'}`;
}

function refreshTicks() {
  ticks.replaceChildren();
  if (timeline.length < 2) return;
  const frag = document.createDocumentFragment();
  for (let i = 0; i < timeline.length; i++) {
    const mark = document.createElement('i');
    mark.style.left = `${(i / (timeline.length - 1)) * 100}%`;
    frag.append(mark);
  }
  ticks.append(frag);
}

/* The slider runs 0..1 continuously and the commit is the nearest one to
 * where it is. It used to run 0..n-1 in whole steps, which meant the handle
 * itself snapped: on a ten-stroke painting there were ten stops across the
 * whole track and the drag felt like a ratchet. The timeline really is
 * discrete - there is no state between two commits to show - but the handle
 * should still follow the cursor, the way a video scrubber does.
 */
function scrubIndex() {
  const span = timeline.length - 1;
  if (span < 1) return 0;
  return Math.round(Number(scrub.value) * span);
}

function scrubFraction(index) {
  const span = timeline.length - 1;
  return span < 1 ? 1 : Math.max(0, index) / span;
}

function refreshScrub() {
  const at = viewing === null ? timeline.length - 1 : viewing;
  // Not while a finger is on it: writing the value mid-drag fights the drag.
  if (!scrubbing) scrub.value = String(scrubFraction(at));
  paintFill();
  atLabel.textContent = viewing === null ? 'now' : `${viewing + 1} / ${timeline.length}`;
  scrub.setAttribute(
    'aria-valuetext',
    timeline.length === 0
      ? 'no commits yet'
      : `commit ${Math.max(0, at) + 1} of ${timeline.length}`,
  );
  app.toggleAttribute('data-past', viewing !== null);
  pastFlag.hidden = viewing === null;
  // Forking from the head would only copy the present, so the offer is only
  // made once you have scrubbed to a commit that is actually in the past.
  playButton.disabled = timeline.length < 2;
}

/* The fill follows the handle, not the commit, so the bar stays under the
   cursor through a drag instead of lurching between tick marks. */
function paintFill() {
  fill.style.width = `${Number(scrub.value) * 100}%`;
}

function loadTimeline() {
  timeline = JSON.parse(paint.timeline());
  refreshCounts();
  refreshTicks();
  refreshScrub();
}

/* ---------- drawing ---------- */

// Committed strokes are baked once into an offscreen canvas and blitted; only
// the stroke under the pointer is drawn live. Redrawing all of them on every
// pointermove meant a drawing got heavier the more you put in it - by a
// hundred strokes the line lagged behind the cursor, which reads as the app
// being broken rather than slow.
const base = document.createElement('canvas');
const baseCtx = base.getContext('2d');
/** The exact array currently baked in, compared by identity. */
let baked = null;
let dpr = 1;

function fit() {
  const rect = board.getBoundingClientRect();
  dpr = Math.min(window.devicePixelRatio || 1, 2);
  board.width = Math.max(1, Math.round(rect.width * dpr));
  board.height = Math.max(1, Math.round(rect.height * dpr));
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  baked = null; // the buffer is the old size
  render(shown);
}

function strokePath(stroke, w, h, g = ctx) {
  g.beginPath();
  const pts = stroke.points;
  g.moveTo(pts[0][0] * w, pts[0][1] * h);
  if (pts.length === 1) {
    // A tap is a dot, not a zero-length line, which would draw nothing.
    g.lineTo(pts[0][0] * w + 0.01, pts[0][1] * h);
  }
  for (let i = 1; i < pts.length; i++) g.lineTo(pts[i][0] * w, pts[i][1] * h);
  g.lineWidth = stroke.width;
  g.lineCap = 'round';
  g.lineJoin = 'round';
  // The eraser cuts back to the panel rather than painting over it, so a
  // stroke removed at commit 4 is still gone when you scrub to commit 40.
  g.globalCompositeOperation = stroke.erase ? 'destination-out' : 'source-over';
  g.strokeStyle = stroke.erase ? 'rgba(0,0,0,1)' : stroke.colour;
  g.stroke();
  g.globalCompositeOperation = 'source-over';
}

function bake(strokes, w, h) {
  base.width = board.width;
  base.height = board.height;
  baseCtx.setTransform(dpr, 0, 0, dpr, 0, 0);
  baseCtx.clearRect(0, 0, w, h);
  for (const stroke of strokes) {
    if (stroke?.points?.length) strokePath(stroke, w, h, baseCtx);
  }
  baked = strokes;
}

function render(strokes) {
  shown = strokes;
  const rect = board.getBoundingClientRect();
  if (strokes !== baked) bake(strokes, rect.width, rect.height);

  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.clearRect(0, 0, board.width, board.height);
  ctx.drawImage(base, 0, 0);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

  // The live stroke composites onto the blit, so an eraser under the pointer
  // cuts into the baked paint exactly as it will once committed.
  if (drawing?.points.length) strokePath(drawing, rect.width, rect.height);
}

function point(event) {
  const rect = board.getBoundingClientRect();
  return [
    Math.min(1, Math.max(0, (event.clientX - rect.left) / rect.width)),
    Math.min(1, Math.max(0, (event.clientY - rect.top) / rect.height)),
  ];
}

/* Starts a new timeline from the commit being viewed, and moves the brush to
 * it. Called from pointerdown rather than from a button: having scrubbed back
 * and picked up the brush, the person has already said what they want, and a
 * "Branch here" button in the way only asks them to say it twice.
 *
 * The painting forked from keeps everything it had - this is
 * `fork_at_timestamp`, not a copy of the canvas. */
function branchFromHere() {
  const commit = timeline[viewing];
  if (!commit) return false;
  try {
    const forked = JSON.parse(paint.fork_at(commit.timestamp));
    openBranch();
    say(
      `<b>branch fork</b> ${forked.branch} from ${forked.from} at commit ${commit.version} — painting on ${forked.branch}`,
    );
    return true;
  } catch (error) {
    console.error(error);
    say(`<b>could not branch</b> ${error?.message ?? error}`);
    return false;
  }
}

board.addEventListener('pointerdown', (event) => {
  stopPlaying();
  // Drawing in the past is the fork. No mode, no button: the brush works
  // wherever you are and the timeline does what it has to underneath.
  if (viewing !== null && !branchFromHere()) return;
  board.setPointerCapture(event.pointerId);
  drawing = { colour, width, erase: erasing, points: [point(event)] };
  render(shown);
});

board.addEventListener('pointermove', (event) => {
  if (!drawing) return;
  const [x, y] = point(event);
  const last = drawing.points[drawing.points.length - 1];
  // Thin the trail: a 120Hz drag sends thousands of points a second and the
  // ones a pixel apart cost a commit's size without changing the line.
  if (Math.hypot(x - last[0], y - last[1]) < 0.0015) return;
  drawing.points.push([x, y]);
  render(shown);
});

function finish() {
  if (!drawing) return;
  const stroke = drawing;
  drawing = null;
  if (!stroke.points.length) return;

  try {
    const commit = JSON.parse(paint.stroke(JSON.stringify(stroke)));
    loadTimeline();
    render([...shown, stroke]);
    say(
      `<b>kv put</b> stroke/${String(commit.strokes).padStart(8, '0')} + head → commit ${commit.version} at ${commit.timestamp}`,
    );
  } catch (error) {
    console.error(error);
    say(`<b>refused</b> ${error?.message ?? error}`);
    render(shown);
  }
}

// pointerup and pointercancel only. `pointerleave` was in here too, which
// committed the stroke the moment the cursor crossed the edge of the canvas -
// so a line drawn out to the border stopped short and became two strokes.
// Pointer capture already delivers the release wherever it happens.
board.addEventListener('pointerup', finish);
board.addEventListener('pointercancel', finish);

/* ---------- scrubbing ---------- */

function show(index) {
  const commit = timeline[index];
  if (!commit) return;
  const strokes = JSON.parse(paint.canvas_at(commit.timestamp));
  render(strokes);
  say(
    `<b>kv list_at</b>("stroke/", ${commit.timestamp}) → ${strokes.length} stroke${strokes.length === 1 ? '' : 's'} · commit ${commit.version}`,
  );
}

function goTo(index) {
  const last = timeline.length - 1;
  showing = index;
  viewing = index >= last ? null : index;
  refreshScrub();
  if (viewing === null) {
    render(JSON.parse(paint.canvas()));
    say(`<b>kv list</b>("stroke/") → the painting as it is now`);
  } else {
    show(index);
  }
}

scrub.addEventListener('pointerdown', () => {
  scrubbing = true;
});
addEventListener('pointerup', () => {
  if (!scrubbing) return;
  scrubbing = false;
  refreshScrub();
});

scrub.addEventListener('input', () => {
  stopPlaying();
  paintFill();
  const index = scrubIndex();
  // The handle moves continuously; the database is only asked when the drag
  // actually crosses into another commit. Without this a single drag fires a
  // hundred identical as-of reads and the repaints make it feel like it is
  // lagging behind the cursor.
  if (index === showing) return;
  showing = index;
  goTo(index);
});

// Arrow keys step one commit. `step="any"` would otherwise move them by a
// hundredth of the track, which on a short history lands between commits.
scrub.addEventListener('keydown', (event) => {
  const step =
    event.key === 'ArrowRight' || event.key === 'ArrowUp'
      ? 1
      : event.key === 'ArrowLeft' || event.key === 'ArrowDown'
        ? -1
        : 0;
  if (!step || timeline.length < 2) return;
  event.preventDefault();
  stopPlaying();
  const at = viewing === null ? timeline.length - 1 : viewing;
  const next = Math.min(timeline.length - 1, Math.max(0, at + step));
  showing = next;
  goTo(next);
});

document.querySelector('[data-now]').addEventListener('click', () => {
  stopPlaying();
  goTo(timeline.length - 1);
});

/* ---------- timelapse ---------- */

// Swapped here rather than in CSS: the `d` property is a path override only
// Chrome implements, so everywhere else the pause button stayed a triangle.
const PLAY_GLYPH = 'M4 2.5v11l9-5.5z';
const PAUSE_GLYPH = 'M4 3h3v10H4zM9 3h3v10H9z';

function setPlayGlyph(playing) {
  playButton.querySelector('path').setAttribute('d', playing ? PAUSE_GLYPH : PLAY_GLYPH);
}

/* How fast the painting replays.
 *
 * A fixed interval per commit is wrong at both ends: six strokes went by in
 * half a second with nothing to watch, and three hundred would have taken
 * half a minute. The replay is paced to a target length instead, with a floor
 * and a ceiling on the time any one commit gets, and a long history advances
 * several commits per frame rather than dragging the whole thing out. */
const REPLAY_MS = 7000;
const FRAME_MIN = 110;
const FRAME_MAX = 420;

function pace(commits) {
  const frame = Math.min(FRAME_MAX, Math.max(FRAME_MIN, REPLAY_MS / Math.max(1, commits)));
  const step = Math.max(1, Math.ceil(commits / (REPLAY_MS / frame)));
  return { frame, step };
}

function stopPlaying() {
  if (playTimer === null) return;
  clearTimeout(playTimer);
  playTimer = null;
  playButton.removeAttribute('data-playing');
  playButton.setAttribute('aria-label', 'Play the painting back');
  setPlayGlyph(false);
}

function play() {
  if (timeline.length < 2) return;
  stopPlaying();
  playButton.setAttribute('data-playing', '');
  playButton.setAttribute('aria-label', 'Stop');
  setPlayGlyph(true);
  const { frame, step } = pace(timeline.length);
  let i = 0;
  goTo(0);

  // Chained timeouts, not setInterval. A frame here does a real as-of read and
  // repaints every stroke it returns, so on a long painting it can outrun the
  // interval - and setInterval answers that by firing the backlog back to
  // back, which is exactly the stutter-then-sprint that made the replay look
  // like it was running away. This way the gap is always a real gap.
  const tick = () => {
    i += step;
    if (i >= timeline.length - 1) {
      stopPlaying();
      goTo(timeline.length - 1);
      return;
    }
    goTo(i);
    playTimer = setTimeout(tick, frame);
  };
  playTimer = setTimeout(tick, frame);
}

playButton.addEventListener('click', () => {
  if (playTimer !== null) stopPlaying();
  else play();
});

/* ---------- tools ---------- */

for (const value of PALETTE) {
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'swatch';
  button.style.setProperty('--swatch', value);
  button.setAttribute('role', 'radio');
  button.setAttribute('aria-checked', String(value === colour));
  button.setAttribute('aria-label', value);
  button.addEventListener('click', () => {
    colour = value;
    erasing = false;
    eraseButton.setAttribute('aria-pressed', 'false');
    for (const other of document.querySelectorAll('.swatch')) {
      other.setAttribute('aria-checked', String(other === button));
    }
  });
  document.querySelector('[data-swatches]').append(button);
}

function sizeChanged() {
  width = Number(sizeInput.value);
  sizeDot.style.setProperty('--dot', String(0.2 + (width / 48) * 0.8));
}
sizeInput.addEventListener('input', sizeChanged);
sizeChanged();

eraseButton.addEventListener('click', () => {
  erasing = !erasing;
  eraseButton.setAttribute('aria-pressed', String(erasing));
});

// Clear is a stroke like any other: a wide erase across the canvas, so it
// lands on the timeline and can itself be scrubbed past. Nothing in this app
// deletes, which is what makes the history trustworthy.
document.querySelector('[data-clear]').addEventListener('click', () => {
  stopPlaying();
  goTo(timeline.length - 1);
  const rows = 26;
  const points = [];
  for (let r = 0; r < rows; r++) {
    const y = (r + 0.5) / rows;
    points.push([r % 2 ? 1.02 : -0.02, y], [r % 2 ? -0.02 : 1.02, y]);
  }
  try {
    paint.stroke(JSON.stringify({ colour: '#000', width: 44, erase: true, points }));
    loadTimeline();
    render(JSON.parse(paint.canvas()));
    say('<b>kv put</b> a full-width erase — cleared, and still on the timeline');
  } catch (error) {
    console.error(error);
    say(`<b>refused</b> ${error?.message ?? error}`);
  }
});

/* ---------- go ---------- */

addEventListener('resize', fit);
fit();
loadTimeline();
refreshBranches();
say('the database is empty — draw something');
