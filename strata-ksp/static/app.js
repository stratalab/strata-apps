/* Strata KSP — instrument panel.
 *
 * Two rules the rest of this file follows.
 *
 * The camera frames the DIFFERENCE, not the body. The old view fitted the
 * planet, so two trajectories that differ by tens of metres over a 200-unit
 * world collapsed into the same hairline and the screen was 60% empty brown.
 * Here the view fits the envelope of the post-fork trails, which is the only
 * region where the timelines disagree, and the planet is whatever is left.
 *
 * Colour means one thing. Cyan is the timeline that kept flying, amber is the
 * one you forked off it. Nothing else in the interface is coloured, so a glance
 * at any gauge, lane or trace tells you which history you are reading.
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

/* The sequence where a child stopped agreeing with its parent. Trail samples
 * carry their own seq, so this is a read of the data rather than a guess. */
function forkSeq(parent, fork) {
  if (!parent || !fork || !fork.trail || !fork.trail.length) return null;
  return fork.trail[0].seq || null;
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
  const [w, h] = fitDpr(canvas, ctx);
  ctx.clearRect(0, 0, w, h);

  const { parent, fork } = pair(state);
  const split = forkSeq(parent, fork);

  // The profile is the main plot, not the orbital view.
  //
  // Two timelines that differ by a tank of fuel end up ~0.3 m apart above a
  // 200 m body. In a shared world-space plot one trace is simply drawn on top
  // of the other and the divergence - the entire subject - is invisible. Height
  // against time separates them: one line leaves the fork mark and becomes two,
  // and the gap between them IS the difference the fork made.
  const splitY = Math.round(h * 0.70);
  profile(w, splitY, parent, fork, split);
  divergence(w, h, splitY, parent, fork, split);

  // The orbital view survives as an inset, because it is what makes the thing
  // legible as a flight rather than a chart.
  orbitInset(w, h, parent, fork, split);

  renderReadout(state, parent, fork);
  renderHangar(state);
  drawArc(state, parent, fork, split);

  const empty = document.getElementById("empty");
  if (empty) empty.hidden = (state.launches || []).length > 0;
}

function profile(w, h, parent, fork, split) {
  const pad = { l: 64, r: 34, t: 34, b: 34 };
  const lanes = [parent, fork].filter(Boolean);
  const samples = [];
  for (const l of lanes) for (const s of l.trail || []) samples.push(s);
  if (samples.length < 2) return;

  let tMin = Infinity, tMax = -Infinity, aMin = Infinity, aMax = -Infinity;
  for (const s of samples) {
    const a = Math.hypot(s.x, s.y) - R;
    if (s.t < tMin) tMin = s.t;
    if (s.t > tMax) tMax = s.t;
    if (a < aMin) aMin = a;
    if (a > aMax) aMax = a;
  }
  if (tMax - tMin < 1e-6) tMax = tMin + 1;
  const span = Math.max(aMax - aMin, 1e-6);
  aMin -= span * 0.12;
  aMax += span * 0.12;

  const X = (t) => pad.l + ((t - tMin) / (tMax - tMin)) * (w - pad.l - pad.r);
  const Y = (a) => h - pad.b - ((a - aMin) / (aMax - aMin)) * (h - pad.t - pad.b);

  // graticule + altitude labels
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

  const line = (l, colour, from) => {
    const t = (l.trail || []).filter((s) => from === null || s.seq >= from);
    if (t.length < 2) return;
    ctx.beginPath();
    t.forEach((s, i) => {
      const x = X(s.t);
      const y = Y(Math.hypot(s.x, s.y) - R);
      i ? ctx.lineTo(x, y) : ctx.moveTo(x, y);
    });
    ctx.strokeStyle = colour;
    ctx.lineWidth = 2;
    ctx.lineJoin = "round";
    ctx.stroke();
  };

  // Shade the gap between the two timelines: the area is the divergence.
  if (parent && fork && split !== null) {
    const pt = (parent.trail || []).filter((s) => s.seq >= split);
    const ft = fork.trail || [];
    if (pt.length > 1 && ft.length > 1) {
      ctx.beginPath();
      pt.forEach((s, i) => {
        const x = X(s.t), y = Y(Math.hypot(s.x, s.y) - R);
        i ? ctx.lineTo(x, y) : ctx.moveTo(x, y);
      });
      for (let i = ft.length - 1; i >= 0; i--) {
        const s = ft[i];
        ctx.lineTo(X(s.t), Y(Math.hypot(s.x, s.y) - R));
      }
      ctx.closePath();
      ctx.fillStyle = FORK;
      ctx.globalAlpha = 0.1;
      ctx.fill();
      ctx.globalAlpha = 1;
    }
  }

  if (parent && split !== null) line(parent, INK3, null);
  if (parent) line(parent, PARENT, split);
  if (fork) line(fork, FORK, null);

  if (parent && split !== null) {
    const at = (parent.trail || []).find((s) => s.seq >= split);
    if (at) forkMark(X(at.t), Y(Math.hypot(at.x, at.y) - R));
  }
  for (const [l, c] of [[parent, PARENT], [fork, FORK]]) {
    if (!l || !(l.trail || []).length) continue;
    const s = l.trail[l.trail.length - 1];
    ctx.fillStyle = c;
    ctx.beginPath();
    ctx.arc(X(s.t), Y(Math.hypot(s.x, s.y) - R), 3.5, 0, Math.PI * 2);
    ctx.fill();
  }
}

/* The divergence strip: fork minus parent, over time.
 *
 * This is the panel the app is actually for. Adding a tank moves the
 * trajectory by about 0.3 m out of 45 - under one percent, so on any shared
 * axis the two timelines are the same line and the difference is invisible.
 * Plotted as a difference against its own scale, that same 0.3 m fills the
 * panel: the moment the histories separate, and by how much, and in which
 * direction. A diff does not show you two files and hope you spot it.
 */
function divergence(w, h, top, parent, fork, split) {
  const pad = { l: 64, r: 34, t: 26, b: 30 };
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

  if (!parent || !fork || split === null) {
    ctx.fillText("DIVERGENCE", pad.l, top + 16);
    ctx.fillStyle = INK3;
    ctx.fillText("Fork the launch to compare two timelines.", pad.l, (y0 + y1) / 2);
    return;
  }

  const alt = (s) => Math.hypot(s.x, s.y) - R;
  const pt = (parent.trail || []).filter((s) => s.seq >= split);
  const ft = fork.trail || [];
  if (pt.length < 2 || ft.length < 2) return;

  // Sample the parent at the fork's timestamps so the difference is taken at
  // equal times rather than equal indices.
  const at = (trail, t) => {
    let lo = 0, hi = trail.length - 1;
    if (t <= trail[0].t) return alt(trail[0]);
    if (t >= trail[hi].t) return alt(trail[hi]);
    while (hi - lo > 1) {
      const mid = (lo + hi) >> 1;
      if (trail[mid].t <= t) lo = mid; else hi = mid;
    }
    const span = trail[hi].t - trail[lo].t || 1;
    const k = (t - trail[lo].t) / span;
    return alt(trail[lo]) * (1 - k) + alt(trail[hi]) * k;
  };

  const pts = ft.map((s) => ({ t: s.t, d: alt(s) - at(pt, s.t) }));
  let tMin = pts[0].t, tMax = pts[pts.length - 1].t, mag = 1e-6;
  for (const q of pts) if (Math.abs(q.d) > mag) mag = Math.abs(q.d);
  if (tMax - tMin < 1e-6) tMax = tMin + 1;
  mag *= 1.25;

  const X = (t) => pad.l + ((t - tMin) / (tMax - tMin)) * (w - pad.l - pad.r);
  const Y = (d) => (y0 + y1) / 2 - (d / mag) * ((y1 - y0) / 2);

  // zero line: where the two histories still agree
  ctx.strokeStyle = "#2a2f36";
  ctx.setLineDash([3, 4]);
  ctx.beginPath();
  ctx.moveTo(pad.l, Y(0));
  ctx.lineTo(w - pad.r, Y(0));
  ctx.stroke();
  ctx.setLineDash([]);

  ctx.beginPath();
  ctx.moveTo(X(pts[0].t), Y(0));
  pts.forEach((q) => ctx.lineTo(X(q.t), Y(q.d)));
  ctx.lineTo(X(pts[pts.length - 1].t), Y(0));
  ctx.closePath();
  ctx.fillStyle = FORK;
  ctx.globalAlpha = 0.16;
  ctx.fill();
  ctx.globalAlpha = 1;

  ctx.beginPath();
  pts.forEach((q, i) => (i ? ctx.lineTo(X(q.t), Y(q.d)) : ctx.moveTo(X(q.t), Y(q.d))));
  ctx.strokeStyle = FORK;
  ctx.lineWidth = 2;
  ctx.stroke();

  const last = pts[pts.length - 1];
  ctx.fillStyle = FORK;
  ctx.beginPath();
  ctx.arc(X(last.t), Y(last.d), 3.5, 0, Math.PI * 2);
  ctx.fill();

  ctx.fillStyle = INK3;
  ctx.textAlign = "left";
  ctx.fillText("DIVERGENCE  fork \u2212 parent / m", pad.l, top + 16);
  ctx.textAlign = "right";
  ctx.fillText(`${(mag / 1.25).toFixed(1)}`, pad.l - 10, Y(mag / 1.25) + 3);
  ctx.fillText(`-${(mag / 1.25).toFixed(1)}`, pad.l - 10, Y(-mag / 1.25) + 3);
  ctx.fillText("0", pad.l - 10, Y(0) + 3);
  ctx.fillStyle = INK;
  ctx.font = '500 13px "IBM Plex Mono", monospace';
  ctx.fillText(`${last.d >= 0 ? "+" : ""}${last.d.toFixed(2)} m`, w - pad.r, top + 16);
}

/* Small orbital view, top right. Context, not subject. */
function orbitInset(w, h, parent, fork, split) {
  const size = Math.min(210, w * 0.19);
  const x0 = w - size - 26;
  const y0 = 26;
  const cx = x0 + size / 2;
  const cy = y0 + size / 2;

  let maxR = R * 1.08;
  for (const l of [parent, fork]) for (const s of (l && l.trail) || []) {
    const r = Math.hypot(s.x, s.y);
    if (r > maxR) maxR = r;
  }
  const scale = (size / 2 - 8) / maxR;

  ctx.strokeStyle = "#20242a";
  ctx.lineWidth = 1;
  ctx.strokeRect(x0 + 0.5, y0 + 0.5, size, size);
  ctx.beginPath();
  ctx.arc(cx, cy, R * scale, 0, Math.PI * 2);
  ctx.fillStyle = "#0b0d10";
  ctx.fill();
  ctx.strokeStyle = "#2a2f36";
  ctx.stroke();

  for (const [l, c] of [[parent, PARENT], [fork, FORK]]) {
    const t = (l && l.trail) || [];
    if (t.length < 2) continue;
    ctx.beginPath();
    t.forEach((s, i) => {
      const x = cx + s.x * scale, y = cy - s.y * scale;
      i ? ctx.lineTo(x, y) : ctx.moveTo(x, y);
    });
    ctx.strokeStyle = c;
    ctx.lineWidth = 1.25;
    ctx.stroke();
  }
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


/* The fork point gets a mark because it is the one moment the whole demo is
 * about: one history became two here. */
function forkMark(x, y) {
  ctx.strokeStyle = INK;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.arc(x, y, 5, 0, Math.PI * 2);
  ctx.stroke();
  ctx.beginPath();
  ctx.moveTo(x, y - 11);
  ctx.lineTo(x, y - 6);
  ctx.stroke();
  ctx.fillStyle = INK2;
  ctx.font = '500 9px "IBM Plex Mono", monospace';
  ctx.textAlign = "center";
  ctx.fillText("FORK", x, y - 16);
}



/* ── the arc: the event log, drawn ────────────────────────────────── */

function drawArc(state, parent, fork, split) {
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
    const a = at(t[0].seq);
    const b = at(lane.l.seq || t[t.length - 1].seq);
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
    arcCtx.fillText(lane.l.name, a - 10, lane.y + 3);
  }

  // the split: one history became two
  if (split !== null && lanes.length === 2) {
    const x = at(split);
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

function renderReadout(state, parent, fork) {
  const set = (id, v) => {
    const el = document.getElementById(id);
    if (el) el.textContent = v;
  };
  const alt = (l) => Math.hypot(l.x, l.y) - R;
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

  if (state.last_promote && state.last_promote.error) {
    const note = document.getElementById("promote-note");
    note.hidden = false;
    note.textContent = `${state.last_promote.error.code || "conflict"} — hangar unchanged`;
  }
}

function renderHangar(state) {
  if (!state.vab) return;
  document.getElementById("vab-name").textContent = state.vab.name;
  document.getElementById("wet").textContent = num(state.vab.wet_mass, 2);
  document.getElementById("dv").textContent = num(state.vab.dv_budget_mps, 0);

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
document.getElementById("btn-compare").onclick = () => post("/api/compare", {}).then(draw);
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
  post("/api/promote", { launch, strategy }).then((body) => {
    const err = body && body.error;
    if (body && body.vab) draw(body);
    else if (lastState) draw(lastState);
    if (err) {
      const note = document.getElementById("promote-note");
      note.hidden = false;
      note.textContent = `${err.code || "conflict"} — hangar unchanged`;
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

