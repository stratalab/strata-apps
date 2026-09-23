/* Strata KSP — instrument panel.
 *
 * The app has one subject: a history becoming two. The plot tells it in three
 * acts, with two colours and one line style.
 *
 *   cyan alone         the parent flew this, and nothing had forked off it
 *   amber dashes       a branch exists here, and still agrees with its parent
 *   amber solid        the branch was written to, and the gap is the change
 *
 * That middle act is the one worth having. A fork costs nothing and changes
 * nothing until you write to it, and the distance between the BRANCH rule and
 * the DIVERGED mark is how long the copy stayed free.
 *
 * Cyan is the timeline that kept flying, amber is the one you forked off it.
 * Nothing else in the interface is coloured, so a glance at any gauge, lane or
 * trace tells you which history you are reading.
 */

const R = 200;
const PARENT = "#5bc8d6";
const FORK = "#ffb03a";
const INK = "#ffffff";
const INK2 = "#9aa3ad";
const INK3 = "#5d646d";
const GRID = "#16181c";

const canvas = document.getElementById("plot");
const ctx = canvas.getContext("2d");
const arcEl = document.getElementById("arc");
const arcCtx = arcEl.getContext("2d");

let lastState = null;
let scrubbing = false;
let throttleDragging = false;
let verdictSource = null;

/* ── motion ───────────────────────────────────────────────────────
 *
 * One animation, and it answers one action. When a fork appears the new
 * branch is drawn arriving along the history it inherited, because that
 * inheritance is the only thing on screen a still frame cannot tell you:
 * the amber dashes run back to the branch rule, over ground the cyan line
 * already covered. Everything else on this page moves because the rocket
 * moved.
 */
const REDUCED = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
const FORK_MS = 1150;
let forkAnim = null;
let known = null;
let pump = 0;

const easeOut = (k) => 1 - Math.pow(1 - k, 3);

function forkProgress() {
  if (!forkAnim) return 1;
  const k = (performance.now() - forkAnim.t0) / FORK_MS;
  if (k >= 1) {
    forkAnim = null;
    return 1;
  }
  return k;
}

/* The socket already pushes a frame per tick, but it stops when the world is
 * paused - and you can fork a paused flight. This keeps frames coming for as
 * long as an animation is running, and not one frame longer. */
function pumpFrames() {
  pump = 0;
  if (lastState) draw(lastState);
  if (forkAnim) pump = requestAnimationFrame(pumpFrames);
}

function watchForForks(state) {
  const names = new Set((state.launches || []).map((l) => l.name));
  if (known) {
    for (const l of state.launches || []) {
      if (known.has(l.name) || !l.fork_seq || REDUCED) continue;
      forkAnim = { name: l.name, t0: performance.now() };
      if (!pump) pump = requestAnimationFrame(pumpFrames);
    }
  }
  known = names;
}

function fmtT(t) {
  const s = Math.max(0, t);
  const m = Math.floor(s / 60);
  return `${String(m).padStart(2, "0")}:${(s - m * 60).toFixed(1).padStart(4, "0")}`;
}

function num(v, digits = 1) {
  return Number.isFinite(v) ? v.toFixed(digits) : "—";
}

function fitDpr(el, c) {
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  const w = el.clientWidth;
  const h = el.clientHeight;
  if (el.width !== Math.round(w * dpr) || el.height !== Math.round(h * dpr)) {
    el.width = Math.round(w * dpr);
    el.height = Math.round(h * dpr);
  }
  c.setTransform(dpr, 0, 0, dpr, 0, 0);
  return [w, h];
}

/* Which launch is the parent line, and which is the fork being compared.
 * `parent` on a LaunchView is the branch it was forked from, so the tree is
 * already in the data; this just picks the pair worth showing side by side. */
function pair(state) {
  const all = state.launches || [];
  if (!all.length) return { parent: null, fork: null };
  const focused = all.find((l) => l.name === state.focused) || all[0];
  const forked = all.filter((l) => all.some((p) => p.name === l.parent));
  if (focused.parent && all.some((p) => p.name === focused.parent)) {
    return { parent: all.find((p) => p.name === focused.parent), fork: focused };
  }
  const child = forked.find((l) => l.parent === focused.name);
  return { parent: focused, fork: child || null };
}

const alt = (s) => Math.hypot(s.x, s.y) - R;

/* Read a trail at an arbitrary time, so two histories are compared at equal
 * times rather than at equal indices. */
function altAt(trail, t) {
  const pts = trail || [];
  if (!pts.length) return 0;
  let lo = 0;
  let hi = pts.length - 1;
  if (t <= pts[0].t) return alt(pts[0]);
  if (t >= pts[hi].t) return alt(pts[hi]);
  while (hi - lo > 1) {
    const mid = (lo + hi) >> 1;
    if (pts[mid].t <= t) lo = mid;
    else hi = mid;
  }
  const span = pts[hi].t - pts[lo].t || 1;
  const k = (t - pts[lo].t) / span;
  return alt(pts[lo]) * (1 - k) + alt(pts[hi]) * k;
}

/* A run of {t, a} between two times, with the ends interpolated so adjacent
 * acts meet instead of leaving a notch at the seam. */
function slice(trail, t0, t1) {
  const pts = trail || [];
  const out = [];
  for (let i = 0; i < pts.length; i += 1) {
    const s = pts[i];
    if (s.t < t0 || s.t > t1) continue;
    if (!out.length && i > 0) out.push({ t: t0, a: altAt(pts, t0) });
    out.push({ t: s.t, a: alt(s) });
  }
  const last = pts[pts.length - 1];
  if (out.length && last && last.t > t1) out.push({ t: t1, a: altAt(pts, t1) });
  return out;
}

/* Where the branch was cut. A database fact: the engine recorded the version,
 * the snapshot carries it as fork_seq, this maps it onto the time axis. */
function branchPoint(parent, fork) {
  if (!parent || !fork || !fork.fork_seq) return null;
  const at = (parent.trail || []).find((s) => s.seq >= fork.fork_seq);
  return at ? { seq: fork.fork_seq, t: at.t, a: alt(at) } : null;
}

/* How much of a difference this reading could be inventing.
 *
 * The parent is read at the fork's timestamps by drawing a straight line
 * between its two nearest samples, and on a curve that line misses the truth
 * by about |a[i-1] - 2a[i] + a[i+1]| / 8. Two sample grids offset by a
 * fraction of a step therefore disagree by that much even when the two
 * trajectories are the same - and they often are the same, because a coasting
 * vehicle follows a path that does not depend on its mass. Adding a tank to
 * something already in orbit changes nothing about where it goes.
 *
 * Without this the panel draws that arithmetic as if it were the branch. */
function interpError(trail, t) {
  const pts = trail || [];
  if (pts.length < 3) return 0;
  let lo = 0;
  let hi = pts.length - 1;
  if (t <= pts[0].t || t >= pts[hi].t) return 0;
  while (hi - lo > 1) {
    const mid = (lo + hi) >> 1;
    if (pts[mid].t <= t) lo = mid;
    else hi = mid;
  }
  const i = Math.min(Math.max(lo, 1), pts.length - 2);
  return Math.abs(alt(pts[i - 1]) - 2 * alt(pts[i]) + alt(pts[i + 1])) / 8;
}

/* Where the two histories stopped agreeing - which is not the same moment.
 * A fresh branch reads identically to its parent until something writes to
 * it, so this walks the child forward from the branch and returns the first
 * sample that no longer matches the parent at the same instant, by more than
 * the reading itself could account for and for more than one sample. */
function divergePoint(parent, fork, branch) {
  if (!parent || !fork) return null;
  const pt = parent.trail || [];
  const ft = fork.trail || [];
  if (pt.length < 2 || ft.length < 2) return null;
  const end = pt[pt.length - 1].t;
  const from = branch ? branch.t : pt[0].t;
  let first = null;
  for (const s of ft) {
    if (s.t < from || s.t > end) continue;
    const floor = Math.max(0.01, interpError(pt, s.t) * 1.5);
    if (Math.abs(alt(s) - altAt(pt, s.t)) > floor) {
      if (first) return first;
      first = { t: s.t, seq: s.seq, a: alt(s) };
    } else {
      first = null;
    }
  }
  return null;
}

function seqRange(state) {
  let lo = Infinity;
  let hi = 0;
  for (const l of state.launches || []) {
    for (const s of l.trail || []) {
      if (s.seq < lo) lo = s.seq;
      if (s.seq > hi) hi = s.seq;
    }
    if (l.seq > hi) hi = l.seq;
  }
  return Number.isFinite(lo) ? [lo, Math.max(hi, lo + 1)] : [0, 1];
}

/* ── the plot ─────────────────────────────────────────────────────── */

function draw(state) {
  if (!state || !state.launches) return;
  lastState = state;
  watchForForks(state);
  const k = forkProgress();

  const [w, h] = fitDpr(canvas, ctx);
  ctx.clearRect(0, 0, w, h);

  const { parent, fork } = pair(state);
  const branch = branchPoint(parent, fork);
  const diverge = divergePoint(parent, fork, branch);

  // Both panels are read together - is that bump in the difference the same
  // bump in the climb? - so they are given one time axis rather than each
  // fitting its own data. The branch rule, the diverge mark and the point the
  // difference opens all land on the same vertical.
  const time = timeAxis(w, parent, fork);

  // Height against time, not a shared world-space view. Two timelines that
  // differ by a tank of fuel end up ~0.3 m apart above a 200 m body, so in
  // world space one trace is simply drawn on top of the other and the
  // divergence - the entire subject - is invisible.
  const splitY = Math.round(h * 0.7);
  profile(w, splitY, parent, fork, branch, diverge, k, time);
  divergence(w, h, splitY, parent, fork, branch, diverge, k, time);

  // The orbital view survives as an inset, because it is what makes the thing
  // legible as a flight rather than a chart.
  orbitInset(w, h, parent, fork);

  renderReadout(state, parent, fork, diverge, equalTimeDelta(parent, fork, branch));
  renderHangar(state);
  renderVerdict(state);
  drawArc(state, parent, fork, branch, k);

  const empty = document.getElementById("empty");
  if (empty) empty.hidden = (state.launches || []).length > 0;
}

const PAD_L = 64;
const PAD_R = 34;

/* The time window both panels are drawn against, and the mapping onto it. */
function timeAxis(w, parent, fork) {
  let tMin = Infinity;
  let tMax = -Infinity;
  for (const l of [parent, fork])
    for (const s of (l && l.trail) || []) {
      if (s.t < tMin) tMin = s.t;
      if (s.t > tMax) tMax = s.t;
    }
  if (!Number.isFinite(tMin)) {
    tMin = 0;
    tMax = 1;
  }
  if (tMax - tMin < 1e-6) tMax = tMin + 1;
  const X = (t) => PAD_L + ((t - tMin) / (tMax - tMin)) * (w - PAD_L - PAD_R);
  return { tMin, tMax, X };
}

/* The difference between the histories right now, taken at the fork's own
 * clock. Reading the two gauge columns against each other compares different
 * mission times - a fork taken at T+30 has thirty seconds less flight behind
 * it - so the number quoted anywhere is this one. */
function equalTimeDelta(parent, fork, branch) {
  if (!parent || !fork || !branch) return null;
  const ft = fork.trail || [];
  const pt = parent.trail || [];
  if (!ft.length || pt.length < 2) return null;
  const last = ft[ft.length - 1];
  return { t: last.t, d: alt(last) - altAt(pt, last.t) };
}

const lastT = (l) => {
  const t = (l && l.trail) || [];
  return t.length ? t[t.length - 1].t : -Infinity;
};

function profile(w, h, parent, fork, branch, diverge, k, time) {
  const pad = { l: PAD_L, r: PAD_R, t: 34, b: 34 };
  const lanes = [parent, fork].filter(Boolean);
  const samples = [];
  for (const l of lanes) for (const s of l.trail || []) samples.push(s);
  if (samples.length < 2) return;

  const { tMin, tMax, X } = time;
  let aMin = Infinity;
  let aMax = -Infinity;
  for (const s of samples) {
    const a = alt(s);
    if (a < aMin) aMin = a;
    if (a > aMax) aMax = a;
  }
  const span = Math.max(aMax - aMin, 1e-6);
  aMin -= span * 0.12;
  aMax += span * 0.12;

  const Y = (a) => h - pad.b - ((a - aMin) / (aMax - aMin)) * (h - pad.t - pad.b);

  const step = niceStep((aMax - aMin) / 5);
  ctx.font = '400 10px "IBM Plex Mono", monospace';
  ctx.textAlign = "right";
  for (let a = Math.ceil(aMin / step) * step; a < aMax; a += step) {
    const y = Math.round(Y(a)) + 0.5;
    ctx.strokeStyle = GRID;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(pad.l, y);
    ctx.lineTo(w - pad.r, y);
    ctx.stroke();
    ctx.fillStyle = INK3;
    ctx.fillText(`${a.toFixed(step < 1 ? 1 : 0)}`, pad.l - 10, y + 3);
  }
  // Axis captions share the baseline: the top-right corner belongs to the
  // orbit inset and the top-left to the hangar.
  ctx.fillStyle = INK3;
  ctx.textAlign = "left";
  ctx.fillText("ALTITUDE / m", pad.l, h - 12);
  ctx.textAlign = "right";
  ctx.fillText("TIME / s", w - pad.r, h - 12);

  // Weight carries the same meaning the colours do. A fork that differs by a
  // fraction of a metre is drawn straight over its parent, so the parent is
  // laid down heavy and the fork runs thin inside it: touching lines read as
  // one cyan line cored with amber, and separating lines read as two.
  const run = (pts, colour, dash, width) => {
    if (pts.length < 2) return;
    ctx.beginPath();
    pts.forEach((p, i) => (i ? ctx.lineTo(X(p.t), Y(p.a)) : ctx.moveTo(X(p.t), Y(p.a))));
    ctx.strokeStyle = colour;
    ctx.lineWidth = width;
    ctx.lineJoin = "round";
    ctx.lineCap = "round";
    ctx.setLineDash(dash || []);
    ctx.stroke();
    ctx.setLineDash([]);
  };

  const bT = branch ? branch.t : tMax;
  const dT = diverge ? diverge.t : tMax;
  // While the fork animation runs, the branch is only drawn as far as it has
  // arrived. Once it finishes, reach is simply the end of the data.
  const reach = forkAnim ? bT + easeOut(k) * (tMax - bT) : tMax;

  // The gap between the histories is the subject, so it is laid down first
  // and both lines are drawn over it.
  // Only where both trails actually have samples: a fork that has flown less
  // than its parent would otherwise close its polygon across the whole plot.
  const gapEnd = Math.min(reach, lastT(parent), lastT(fork));
  if (branch && diverge && gapEnd > dT) {
    const pg = slice(parent.trail, dT, gapEnd);
    const fg = slice(fork.trail, dT, gapEnd);
    if (pg.length > 1 && fg.length > 1) {
      ctx.beginPath();
      pg.forEach((q, i) => (i ? ctx.lineTo(X(q.t), Y(q.a)) : ctx.moveTo(X(q.t), Y(q.a))));
      for (let i = fg.length - 1; i >= 0; i -= 1) ctx.lineTo(X(fg[i].t), Y(fg[i].a));
      ctx.closePath();
      ctx.fillStyle = FORK;
      ctx.globalAlpha = 0.1;
      ctx.fill();
      ctx.globalAlpha = 1;
    }
  }

  // The parent flew all of it, so its line runs the full width. The fork only
  // exists from the branch rule onward: dashed while it still agrees, solid
  // from the point it stopped agreeing.
  run(slice(parent && parent.trail, tMin, tMax), PARENT, null, 3.5);
  if (branch) {
    run(slice(fork.trail, bT, Math.min(dT, reach)), FORK, [2, 5], 1.75);
    if (diverge && reach > dT) run(slice(fork.trail, dT, reach), FORK, null, 1.75);

    // The branch arrives by claiming what it inherited: amber runs backwards
    // from the rule along ground the cyan line already covered, then clears.
    // It cannot stay - the parent flew that stretch alone - but a fork taken
    // at the head has no forward history yet, and this is the only moment
    // that shows where the copy came from.
    if (forkAnim) {
      const sweep = easeOut(Math.min(k / 0.6, 1));
      ctx.globalAlpha = k > 0.72 ? Math.max(0, 1 - (k - 0.72) / 0.28) : 1;
      run(slice(parent.trail, bT - sweep * (bT - tMin), bT), FORK, [2, 5], 1.75);
      ctx.globalAlpha = 1;
    }
  }

  if (branch) branchRule(w, X(bT), pad.t, h - pad.b, branch.seq, k);
  if (diverge && reach >= dT) divergeMark(X(dT), Y(diverge.a), pad.t, h - pad.b);

  for (const [l, c] of [
    [parent, branch ? PARENT : PARENT],
    [fork, FORK],
  ]) {
    if (!l || !(l.trail || []).length) continue;
    const s = l.trail[l.trail.length - 1];
    if (l === fork && s.t > reach) continue;
    ctx.fillStyle = c;
    ctx.beginPath();
    ctx.arc(X(s.t), Y(alt(s)), 3.5, 0, Math.PI * 2);
    ctx.fill();
  }
}

/* The branch is a moment in the log, not a point on a curve, so it reads as a
 * cursor across the whole plot rather than a dot on one line. */
function branchRule(w, x, top, bottom, seq, k) {
  const grow = forkAnim ? easeOut(Math.min(k / 0.25, 1)) : 1;
  ctx.strokeStyle = "#343a42";
  ctx.lineWidth = 1;
  ctx.setLineDash([2, 5]);
  ctx.beginPath();
  ctx.moveTo(Math.round(x) + 0.5, top + (bottom - top) * (1 - grow));
  ctx.lineTo(Math.round(x) + 0.5, bottom);
  ctx.stroke();
  ctx.setLineDash([]);
  if (grow < 1) return;
  // Low, so it clears the orbit inset, and on whichever side of the rule has
  // room - forking at the head puts this hard against the right edge.
  const flip = x > w - 96;
  ctx.textAlign = flip ? "right" : "left";
  const tx = flip ? x - 7 : x + 7;
  ctx.fillStyle = INK2;
  ctx.font = '500 9px "IBM Plex Mono", monospace';
  ctx.fillText("BRANCH", tx, bottom - 34);
  ctx.fillStyle = INK3;
  ctx.fillText(`seq ${seq}`, tx, bottom - 22);
}

/* Where the copy stopped being free. */
function divergeMark(x, y, top, bottom) {
  ctx.strokeStyle = FORK;
  ctx.lineWidth = 1.5;
  ctx.beginPath();
  ctx.arc(x, y, 5, 0, Math.PI * 2);
  ctx.stroke();
  ctx.fillStyle = FORK;
  ctx.font = '500 9px "IBM Plex Mono", monospace';
  ctx.textAlign = "center";
  // Below the mark when the trace is running along the top, where a caption
  // above it would sit on the line it is pointing at.
  const above = y - top > (bottom - top) * 0.25;
  ctx.fillText("DIVERGED", x, above ? y - 13 : y + 19);
}

/* The divergence strip: fork minus parent, over time.
 *
 * This is the panel the app is actually for. Adding a tank moves the
 * trajectory by about 0.3 m out of 45 - under one percent, so on any shared
 * axis the two timelines are the same line and the difference is invisible.
 * Plotted as a difference against its own scale, that same 0.3 m fills the
 * panel: the moment the histories separate, by how much, and in which
 * direction. A diff does not show you two files and hope you spot it.
 */
function divergence(w, h, top, parent, fork, branch, diverge, k, time) {
  const pad = { l: PAD_L, r: PAD_R, t: 26, b: 30 };
  const y0 = top + pad.t;
  const y1 = h - pad.b;

  ctx.strokeStyle = "#1b1e23";
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(0, top + 0.5);
  ctx.lineTo(w, top + 0.5);
  ctx.stroke();

  ctx.fillStyle = INK3;
  ctx.font = '400 10px "IBM Plex Mono", monospace';
  ctx.textAlign = "left";
  ctx.fillText("DIVERGENCE  fork − parent / m", pad.l, top + 16);

  if (!parent || !fork || !branch) {
    ctx.fillStyle = INK3;
    ctx.fillText("Fork the launch to compare two timelines.", pad.l, (y0 + y1) / 2);
    return;
  }

  const pt = (parent.trail || []).filter((s) => s.t >= branch.t);
  const ft = (fork.trail || []).filter((s) => s.t >= branch.t);

  // A branch taken at the head has almost no history of its own yet. That is
  // a reading, not a reason to draw nothing: the zero line and the caption go
  // down first, and only the trace waits for samples.
  const { X } = time;
  const end = ft.length ? ft[ft.length - 1].t : branch.t;
  const reach = forkAnim ? branch.t + easeOut(k) * (end - branch.t) : Infinity;
  const pts =
    pt.length > 1 && ft.length > 1
      ? ft.filter((s) => s.t <= reach).map((s) => ({ t: s.t, d: alt(s) - altAt(pt, s.t) }))
      : [];

  let mag = 0;
  for (const q of pts) if (Math.abs(q.d) > mag) mag = Math.abs(q.d);
  const flat = !diverge || mag < 1e-9;
  const scale = (flat ? 1 : mag) * 1.25;
  const Y = (d) => (y0 + y1) / 2 - (d / scale) * ((y1 - y0) / 2);

  // Zero runs only from the branch: before it there is no second history to
  // take a difference against, and a line there would invent one.
  ctx.strokeStyle = "#2a2f36";
  ctx.setLineDash([3, 4]);
  ctx.beginPath();
  ctx.moveTo(X(branch.t), Y(0));
  ctx.lineTo(X(Math.max(end, branch.t)), Y(0));
  ctx.stroke();
  ctx.setLineDash([]);

  ctx.fillStyle = INK3;
  ctx.font = '400 10px "IBM Plex Mono", monospace';
  ctx.textAlign = "right";
  if (flat) {
    ctx.fillStyle = INK2;
    ctx.font = '400 11px "IBM Plex Mono", monospace';
    ctx.fillText("identical — a branch costs nothing until you write to it", w - pad.r, top + 16);
  }

  // A branch cut this instant has no span of its own to draw across, and an
  // axis label or a marker on a line of no length is furniture, not a reading.
  if (end <= branch.t || pts.length < 2) return;
  ctx.fillStyle = INK3;
  ctx.font = '400 10px "IBM Plex Mono", monospace';
  ctx.textAlign = "right";
  ctx.fillText("0", pad.l - 10, Y(0) + 3);

  // Nothing above the noise floor means the histories agree, so the panel
  // draws them agreeing rather than plotting the arithmetic of the reading.
  if (flat) {
    ctx.fillStyle = FORK;
    ctx.beginPath();
    ctx.arc(X(pts[pts.length - 1].t), Y(0), 3.5, 0, Math.PI * 2);
    ctx.fill();
    return;
  }

  if (!flat) {
    ctx.beginPath();
    ctx.moveTo(X(pts[0].t), Y(0));
    pts.forEach((q) => ctx.lineTo(X(q.t), Y(q.d)));
    ctx.lineTo(X(pts[pts.length - 1].t), Y(0));
    ctx.closePath();
    ctx.fillStyle = FORK;
    ctx.globalAlpha = 0.16;
    ctx.fill();
    ctx.globalAlpha = 1;
  }

  ctx.beginPath();
  pts.forEach((q, i) => (i ? ctx.lineTo(X(q.t), Y(q.d)) : ctx.moveTo(X(q.t), Y(q.d))));
  ctx.strokeStyle = FORK;
  ctx.lineWidth = 2;
  ctx.lineCap = "round";
  ctx.setLineDash(flat ? [2, 5] : []);
  ctx.stroke();
  ctx.setLineDash([]);

  const last = pts[pts.length - 1];
  ctx.fillStyle = FORK;
  ctx.beginPath();
  ctx.arc(X(last.t), Y(last.d), 3.5, 0, Math.PI * 2);
  ctx.fill();

  if (!flat) {
    ctx.fillStyle = INK3;
    ctx.font = '400 10px "IBM Plex Mono", monospace';
    ctx.textAlign = "right";
    ctx.fillText(`${mag.toFixed(mag < 10 ? 2 : 1)}`, pad.l - 10, Y(mag) + 3);
    ctx.fillText(`-${mag.toFixed(mag < 10 ? 2 : 1)}`, pad.l - 10, Y(-mag) + 3);
    ctx.fillStyle = INK;
    ctx.font = '500 13px "IBM Plex Mono", monospace';
    ctx.fillText(`${last.d >= 0 ? "+" : ""}${last.d.toFixed(2)} m`, w - pad.r, top + 16);
  }
}

/* Small orbital view, top right. Context, not subject. */
function orbitInset(w, h, parent, fork) {
  // Sized against both axes: on a short viewport a width-only size made the
  // inset nearly as tall as the plot and swallowed the branch caption.
  const size = Math.min(210, w * 0.19, h * 0.3);
  const x0 = w - size - 26;
  const y0 = 26;
  const cx = x0 + size / 2;
  const cy = y0 + size / 2;

  let maxR = R * 1.08;
  for (const l of [parent, fork])
    for (const s of (l && l.trail) || []) {
      const r = Math.hypot(s.x, s.y);
      if (r > maxR) maxR = r;
    }
  const scale = (size / 2 - 10) / maxR;

  ctx.strokeStyle = "#20242a";
  ctx.lineWidth = 1;
  ctx.strokeRect(x0 + 0.5, y0 + 0.5, size, size);
  // An eccentric orbit reaches further on one side than the other, so the
  // trails are clipped to the frame rather than trusted to fit inside it.
  ctx.save();
  ctx.beginPath();
  ctx.rect(x0 + 1, y0 + 1, size - 1, size - 1);
  ctx.clip();
  ctx.beginPath();
  ctx.arc(cx, cy, R * scale, 0, Math.PI * 2);
  ctx.fillStyle = "#0b0d10";
  ctx.fill();
  ctx.strokeStyle = "#2a2f36";
  ctx.stroke();

  for (const [l, c] of [
    [parent, PARENT],
    [fork, FORK],
  ]) {
    const t = (l && l.trail) || [];
    if (t.length < 2) continue;
    ctx.beginPath();
    t.forEach((s, i) => {
      const x = cx + s.x * scale;
      const y = cy - s.y * scale;
      i ? ctx.lineTo(x, y) : ctx.moveTo(x, y);
    });
    ctx.strokeStyle = c;
    ctx.lineWidth = 1.25;
    ctx.stroke();
  }
  ctx.restore();
  ctx.fillStyle = INK3;
  ctx.font = '400 9px "IBM Plex Mono", monospace';
  ctx.textAlign = "left";
  ctx.fillText("ORBIT", x0, y0 - 7);
}

function niceStep(raw) {
  const pow = Math.pow(10, Math.floor(Math.log10(Math.max(raw, 1e-6))));
  const n = raw / pow;
  return (n > 5 ? 10 : n > 2 ? 5 : n > 1 ? 2 : 1) * pow;
}

/* ── the arc: the event log, drawn ────────────────────────────────── */

function drawArc(state, parent, fork, branch, k) {
  const [w, h] = fitDpr(arcEl, arcCtx);
  arcCtx.clearRect(0, 0, w, h);
  const [lo, hi] = seqRange(state);
  const left = 120;
  const right = w - 120;
  const at = (seq) => left + ((seq - lo) / (hi - lo)) * (right - left);

  const lanes = [];
  if (parent) lanes.push({ l: parent, y: 26, colour: PARENT });
  if (fork) lanes.push({ l: fork, y: 48, colour: FORK });

  for (const lane of lanes) {
    const t = lane.l.trail || [];
    if (!t.length) continue;
    // A fork's lane begins where the branch was cut, not where its inherited
    // events begin, or every lane would start at the same place and the tree
    // would look flat.
    const isFork = lane.l === fork && branch;
    const head = lane.l.seq || t[t.length - 1].seq;
    const a = at(isFork ? branch.seq : t[0].seq);
    const full = at(head);
    const b = isFork && forkAnim ? a + easeOut(k) * (full - a) : full;
    arcCtx.strokeStyle = lane.colour;
    arcCtx.globalAlpha = 0.35;
    arcCtx.lineWidth = 1.5;
    arcCtx.beginPath();
    arcCtx.moveTo(a, lane.y);
    arcCtx.lineTo(b, lane.y);
    arcCtx.stroke();
    arcCtx.globalAlpha = 1;
    arcCtx.fillStyle = lane.colour;
    arcCtx.beginPath();
    arcCtx.arc(b, lane.y, 3, 0, Math.PI * 2);
    arcCtx.fill();
    arcCtx.font = '500 9px "IBM Plex Mono", monospace';
    arcCtx.textAlign = "right";
    arcCtx.fillText(lane.l.name, at(t[0].seq) - 10, lane.y + 3);
  }

  // the split: one history became two, here
  if (branch && lanes.length === 2) {
    const x = at(branch.seq);
    arcCtx.strokeStyle = INK;
    arcCtx.lineWidth = 1;
    arcCtx.beginPath();
    arcCtx.moveTo(x, lanes[0].y);
    arcCtx.lineTo(x, lanes[1].y);
    arcCtx.stroke();
    arcCtx.fillStyle = INK;
    arcCtx.beginPath();
    arcCtx.arc(x, lanes[0].y, 3.5, 0, Math.PI * 2);
    arcCtx.fill();
  }

  // playhead
  const scrub = document.getElementById("scrub");
  const head = scrubbing ? Number(scrub.value) : hi;
  const hx = at(head);
  arcCtx.strokeStyle = INK;
  arcCtx.globalAlpha = 0.5;
  arcCtx.lineWidth = 1;
  arcCtx.beginPath();
  arcCtx.moveTo(hx, 12);
  arcCtx.lineTo(hx, h - 12);
  arcCtx.stroke();
  arcCtx.globalAlpha = 1;
  arcCtx.fillStyle = INK3;
  arcCtx.font = '400 9px "IBM Plex Mono", monospace';
  arcCtx.textAlign = "center";
  arcCtx.fillText(`seq ${head}`, hx, h - 3);

  if (!scrubbing) {
    scrub.min = String(lo);
    scrub.max = String(hi);
    scrub.value = String(hi);
  }
}

/* ── readout ──────────────────────────────────────────────────────── */

function renderReadout(state, parent, fork, diverge, now) {
  const set = (id, v) => {
    const el = document.getElementById(id);
    if (el) el.textContent = v;
  };
  const vel = (l) => Math.hypot(l.vx, l.vy);

  if (parent) {
    set("p-alt", num(alt(parent)));
    set("p-vel", num(vel(parent)));
    set("p-mass", num(parent.mass, 2));
    set("p-name", `${parent.name} · ${parent.status}`);
    set("t", fmtT(parent.t));
    set("mission", parent.design || parent.name);
  }
  const col = document.getElementById("col-fork");
  if (fork) {
    col.classList.remove("idle");
    set("f-alt", num(alt(fork)));
    set("f-vel", num(vel(fork)));
    set("f-mass", num(fork.mass, 2));
    set("f-name", `${fork.name} · ${fork.status}`);
  } else {
    col.classList.add("idle");
    set("f-alt", "—");
    set("f-vel", "—");
    set("f-mass", "—");
    set("f-name", "no fork yet");
  }

  // The delta belongs next to the numbers it is a delta of.
  const delta = document.getElementById("delta");
  if (delta) {
    if (now) {
      delta.hidden = false;
      delta.dataset.state = diverge ? "apart" : "same";
      delta.textContent = diverge
        ? `T+${fmtT(now.t)} · ${now.d >= 0 ? "+" : ""}${now.d.toFixed(2)} m apart`
        : `T+${fmtT(now.t)} · identical`;
    } else {
      delta.hidden = true;
    }
  }

  set("seq", String((parent && parent.seq) || 0));
  set("branches", String(state.branch_count || 1));
  set("persist", `${num(state.persist_ms, 1)} ms`);
  const db = document.getElementById("db-line");
  if (db) db.textContent = state.db_path || "";

  const findings = state.findings || [];
  set("findings-n", String(findings.length));
  const fl = document.getElementById("findings");
  if (fl) fl.textContent = findings.map((f) => f.note || f.code || String(f)).join(" · ");

  document.getElementById("btn-auto").classList.toggle("active", !!(parent && parent.autopilot));
  document.querySelectorAll(".warp").forEach((b) => {
    b.classList.toggle("active", parent && Number(b.dataset.w) === parent.warp);
  });
  const thr = document.getElementById("throttle");
  if (parent && !throttleDragging) thr.value = String(parent.throttle);
}

/* What the database did with the last branch operation you asked for.
 *
 * Promote under Strict is the whole argument of the demo - the engine refuses
 * a conflicting merge and leaves the hangar exactly as it was - and until now
 * it reported that into a panel that is collapsed by default. A refusal that
 * nobody sees is indistinguishable from nothing happening. */
function renderVerdict(state) {
  const el = document.getElementById("verdict");
  if (!el) return;
  // Both results live on the snapshot forever, so the last button you pressed
  // decides which one the strip is reporting.
  const p = verdictSource === "compare" ? null : state.last_promote;
  const c = verdictSource === "promote" ? null : state.last_compare;
  let stamp = "";
  let kind = "";
  let head = "";
  let body = "";

  if (p) {
    kind = p.ok ? "applied" : "refused";
    head = p.strategy === "source_wins" ? "SOURCE WINS" : "STRICT";
    if (p.ok) {
      const n = (p.applied || []).length;
      body = `applied · ${n} ${n === 1 ? "change" : "changes"} from ${p.source_launch || "the fork"} rebuilt the hangar`;
    } else {
      body = `refused · ${p.code || "conflict"} · hangar unchanged`;
    }
    stamp = `p|${head}|${body}`;
  } else if (c && !c.empty) {
    kind = "compare";
    head = "COMPARE";
    body = `${c.added} added · ${c.modified} modified · ${c.removed} removed across ${(c.capabilities || []).join(", ") || "no capabilities"}`;
    stamp = `c|${body}`;
  } else {
    el.hidden = true;
    el.dataset.stamp = "";
    return;
  }

  el.hidden = false;
  el.dataset.state = kind;
  if (el.dataset.stamp !== stamp) {
    el.dataset.stamp = stamp;
    el.classList.remove("in");
    void el.offsetWidth;
    el.classList.add("in");
  }
  document.getElementById("verdict-head").textContent = head;
  document.getElementById("verdict-body").textContent = body;
}

function renderHangar(state) {
  if (!state.vab) return;
  document.getElementById("vab-name").textContent = state.vab.name;
  document.getElementById("wet").textContent = num(state.vab.wet_mass, 2);
  document.getElementById("dv").textContent = num(state.vab.dv_budget_mps, 0);
  // Collapsed, the panel still says what is on the pad, so it reads as a
  // summary you can open rather than an empty box.
  const chip = document.getElementById("hangar-chip");
  if (chip) chip.textContent = `${state.vab.name} · ${num(state.vab.wet_mass, 2)} kg`;

  const stack = document.getElementById("stack");
  stack.innerHTML = "";
  const heaviest = Math.max(...state.vab.parts.map((p) => p.dry_kg + p.fuel_kg), 1);
  state.vab.parts.forEach((p) => {
    const li = document.createElement("li");
    const ord = document.createElement("span");
    ord.className = "ord";
    ord.textContent = p.ordinal;
    const name = document.createElement("b");
    name.textContent = p.kind;
    const bar = document.createElement("span");
    bar.className = "bar";
    bar.style.width = `${((p.dry_kg + p.fuel_kg) / heaviest) * 46}px`;
    const rm = document.createElement("button");
    rm.textContent = "×";
    rm.title = `Remove ${p.kind}`;
    rm.onclick = () => post("/api/vab/remove", { index: p.ordinal }).then(draw);
    li.append(ord, name, bar, rm);
    stack.append(li);
  });

  const catalog = document.getElementById("catalog");
  if (!catalog.dataset.ready && state.vab.catalog) {
    state.vab.catalog.forEach((c) => {
      const b = document.createElement("button");
      b.textContent = c.kind;
      b.title = `Add ${c.kind}`;
      b.onclick = () => post("/api/vab/add", { part_id: c.part_id }).then(draw);
      catalog.append(b);
    });
    catalog.dataset.ready = "1";
  }
}

const hangarToggle = document.getElementById("hangar-toggle");
hangarToggle.onclick = () => {
  const el = document.getElementById("hangar");
  const open = el.hasAttribute("data-collapsed");
  if (open) el.removeAttribute("data-collapsed");
  else el.setAttribute("data-collapsed", "");
  hangarToggle.setAttribute("aria-expanded", String(open));
};

window.addEventListener("resize", () => lastState && draw(lastState));

function post(path, body) {
  return fetch(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: body ? JSON.stringify(body) : "{}",
  }).then((r) => r.json());
}

document.getElementById("btn-run").onclick = () => post("/api/run").then(draw);
document.getElementById("btn-pause").onclick = () => post("/api/pause").then(draw);
document.getElementById("btn-reset").onclick = () => post("/api/launch").then(draw);
document.getElementById("btn-stage").onclick = () => post("/api/stage").then(draw);
document.getElementById("btn-auto").onclick = () => {
  const on = !document.getElementById("btn-auto").classList.contains("active");
  post("/api/autopilot", { on }).then(draw);
};
document.getElementById("btn-save").onclick = () => post("/api/vab/save").then(draw);
document.getElementById("btn-stick").onclick = () => post("/api/vab/reset").then(draw);
document.getElementById("btn-audit").onclick = () =>
  post("/api/audit").then((body) => {
    const line = document.getElementById("audit-line");
    line.textContent = body.ok ? "chain ok" : "chain fail";
  });
document.getElementById("btn-compare").onclick = () => {
  // Default the comparison to the fork against the branch it came from, which
  // is the pair the rest of the screen is already about.
  const { parent, fork } = lastState ? pair(lastState) : { parent: null, fork: null };
  const body = parent && fork ? { a: fork.name, b: parent.name } : {};
  verdictSource = "compare";
  // The answer comes back as the comparison itself, not a snapshot; the next
  // tick carries it on last_compare, so redraw what we already have.
  post("/api/compare", body).then(() => lastState && draw(lastState));
};
document.getElementById("btn-archive").onclick = () => {
  const launch = lastState && lastState.focused;
  if (!launch) return;
  post("/api/archive", { launch }).then(draw);
};
document.getElementById("btn-fork").onclick = () => {
  const from = lastState && lastState.focused;
  if (!from) return;
  const at_seq = Number(document.getElementById("scrub").value);
  post("/api/fork", { from, at_seq }).then(draw);
};
document.getElementById("btn-rewind").onclick = () => {
  const launch = lastState && lastState.focused;
  if (!launch) return;
  const seq = Number(document.getElementById("scrub").value);
  post("/api/rewind", { launch, seq }).then(draw);
};
document.getElementById("btn-tank").onclick = () => post("/api/add-tank", {}).then(draw);
document.getElementById("btn-strategy").onclick = () => {
  const btn = document.getElementById("btn-strategy");
  const next = btn.dataset.strategy === "strict" ? "source_wins" : "strict";
  btn.dataset.strategy = next;
  btn.textContent = next === "strict" ? "Strict" : "SourceWins";
};
document.getElementById("btn-promote").onclick = () => {
  const launch = lastState && lastState.focused;
  if (!launch) return;
  const strategy = document.getElementById("btn-strategy").dataset.strategy || "strict";
  verdictSource = "promote";
  post("/api/promote", { launch, strategy }).then((body) => {
    // A refusal answers with the error rather than a snapshot. The next tick
    // carries last_promote either way; this just saves waiting for one.
    if (body && body.vab) draw(body);
    else if (body && body.error && lastState) {
      lastState.last_promote = {
        ok: false,
        strategy,
        source_launch: launch,
        code: body.error.code,
      };
      draw(lastState);
    }
  });
};
const scrubEl = document.getElementById("scrub");
scrubEl.onpointerdown = () => {
  scrubbing = true;
};
scrubEl.onpointerup = () => {
  scrubbing = false;
};
scrubEl.oninput = () => {
  if (lastState) draw(lastState);
};
const throttleEl = document.getElementById("throttle");
throttleEl.onpointerdown = () => {
  throttleDragging = true;
};
throttleEl.onpointerup = () => {
  throttleDragging = false;
};
throttleEl.oninput = () => {
  post("/api/throttle", { value: Number(throttleEl.value) }).then(draw);
};
document.querySelectorAll(".warp").forEach((btn) => {
  btn.onclick = () => post("/api/warp", { mult: Number(btn.dataset.w) }).then(draw);
});

function connect() {
  const proto = location.protocol === "https:" ? "wss" : "ws";
  const ws = new WebSocket(`${proto}://${location.host}/ws`);
  ws.onmessage = (ev) => {
    try {
      draw(JSON.parse(ev.data));
    } catch {
      /* ignore */
    }
  };
  ws.onclose = () => setTimeout(connect, 800);
}

fetch("/api/state")
  .then((r) => r.json())
  .then(draw)
  .finally(connect);

