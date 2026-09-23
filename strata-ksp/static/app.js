/* Strata KSP.
 *
 * One loop: build a rocket, launch it, see how it went, change one thing, go
 * again. That is the whole game, and the reason it is worth playing is that
 * failing costs nothing.
 *
 * Underneath, every attempt is a branch of the database and Revert is an
 * as-of read of it. The interface never says so, because it does not have to:
 * "try again" is the same idea in words somebody already knows.
 *
 * Colour means one thing each. Cyan is the flight you are watching. Grey is
 * an attempt you already made. Amber is something wanting your attention, and
 * red is a flight that ended badly.
 */

/* The world in play, as the engine reports it. Nothing here assumes a radius
 * or a gravity: pick another planet and every reading, the map scale and the
 * thrust-to-weight all follow. */
let world = { id: "", name: "", radius: 200, g0: 9.81, rho0: 0, scale_height: 1 };

const LIVE = "#5bc8d6";
const GHOST = "#49515b";
const WARN = "#ffb03a";
const FAIL = "#ff6b6b";
const INK = "#ffffff";
const INK2 = "#9aa3ad";
const INK3 = "#5d646d";

const map = document.getElementById("map");
const mapCtx = map.getContext("2d");

let lastState = null;
let throttleDragging = false;
/* Attempts the viewer has asked to see behind the live one. */
const ghosts = new Set();
let screen = "hangar";
/* Where the next part goes, as an index into the stack counted from the base.
 * Zero is the pad end, which is where you usually want an engine. */
let slot = 0;

const $ = (id) => document.getElementById(id);
const alt = (p) => Math.hypot(p.x, p.y) - world.radius;

function fmtT(t) {
  const s = Math.max(0, t);
  const m = Math.floor(s / 60);
  return `${String(m).padStart(2, "0")}:${(s - m * 60).toFixed(1).padStart(4, "0")}`;
}

function num(v, digits = 1) {
  return Number.isFinite(v) ? v.toFixed(digits) : "—";
}

/* Altitudes run from metres on the pad to hundreds up, and a fixed number of
 * decimals is wrong at one end or the other. */
function metres(v) {
  if (!Number.isFinite(v)) return "—";
  const a = Math.abs(v);
  return a >= 100 ? v.toFixed(0) : a >= 10 ? v.toFixed(1) : v.toFixed(2);
}

function fitDpr(el, c) {
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  const w = el.clientWidth;
  const h = el.clientHeight;
  if (!w || !h) return [0, 0];
  if (el.width !== Math.round(w * dpr) || el.height !== Math.round(h * dpr)) {
    el.width = Math.round(w * dpr);
    el.height = Math.round(h * dpr);
  }
  c.setTransform(dpr, 0, 0, dpr, 0, 0);
  return [w, h];
}

function focused(state) {
  const all = state.launches || [];
  return all.find((l) => l.name === state.focused) || all[all.length - 1] || null;
}

/* Attempt 3, not launch-0003. The database's name for it is not the player's. */
const attemptNo = (name) => Number(String(name).replace(/\D/g, "")) || 0;

function peak(launch) {
  let best = 0;
  for (const s of launch.trail || []) best = Math.max(best, alt(s));
  return Math.max(best, alt(launch));
}

/* What happened, in the fewest words that are still true. */
function outcomeOf(launch) {
  if (!launch) return { key: "none", text: "" };
  switch (launch.status) {
    case "orbit":
      return { key: "win", text: `In orbit at ${metres(alt(launch))} m` };
    case "crashed":
      return { key: "fail", text: `Crashed after reaching ${metres(peak(launch))} m` };
    case "escaped":
      return { key: "win", text: "Escaped the planet entirely" };
    default:
      return launch.fuel <= 1e-6
        ? { key: "spent", text: `Out of fuel at ${metres(alt(launch))} m` }
        : { key: "flying", text: "" };
  }
}

/* ── the map ──────────────────────────────────────────────────────
 *
 * World space, the way the game shows it: the planet, where you are, and
 * where you are going. An earlier version plotted altitude against time
 * because it had to separate two branches that differed by centimetres.
 * Nothing here needs separating, and this is the view that reads as flight.
 */

function drawMap(state) {
  const [w, h] = fitDpr(map, mapCtx);
  if (!w) return;
  mapCtx.clearRect(0, 0, w, h);

  const live = focused(state);
  const shown = (state.launches || []).filter((l) => ghosts.has(l.name) && l !== live);

  // Frame the flight, not the body.
  //
  // Fitting the whole planet is the obvious thing and it is wrong here: fifty
  // metres up from a two-hundred-metre world is a sliver against a disc. So
  // the view fits what is actually being flown - the trails, the vehicle, and
  // the ground directly beneath it so the horizon is always in shot - and the
  // planet is drawn at whatever size that implies, mostly off-screen on the
  // way up and whole once you are in orbit.
  //
  // The predicted conic is drawn but never frames the shot: a sub-orbital arc
  // predicts an ellipse through the middle of the planet, and letting that
  // set the bounds would zoom the camera out to nothing.
  const pts = [];
  for (const l of [live, ...shown]) for (const s of (l && l.trail) || []) pts.push(s);
  if (live) {
    pts.push(live);
    const r = Math.hypot(live.x, live.y) || 1;
    pts.push({ x: (live.x / r) * world.radius, y: (live.y / r) * world.radius });
  }

  let minX = -world.radius;
  let maxX = world.radius;
  let minY = -world.radius;
  let maxY = world.radius;
  if (pts.length) {
    minX = Math.min(...pts.map((s) => s.x));
    maxX = Math.max(...pts.map((s) => s.x));
    minY = Math.min(...pts.map((s) => s.y));
    maxY = Math.max(...pts.map((s) => s.y));
  }
  const padW = Math.max((maxX - minX) * 0.14, 12);
  const padH = Math.max((maxY - minY) * 0.14, 12);
  minX -= padW;
  maxX += padW;
  minY -= padH;
  maxY += padH;

  const scale = Math.min((w - 48) / (maxX - minX), (h - 48) / (maxY - minY));
  const midX = (minX + maxX) / 2;
  const midY = (minY + maxY) / 2;
  const cx = w / 2 - midX * scale;
  const cy = h / 2 + midY * scale;
  const X = (x) => cx + x * scale;
  const Y = (y) => cy - y * scale;

  // the planet
  mapCtx.beginPath();
  mapCtx.arc(cx, cy, world.radius * scale, 0, Math.PI * 2);
  mapCtx.fillStyle = "#0e1319";
  mapCtx.fill();
  mapCtx.strokeStyle = "#232a33";
  mapCtx.lineWidth = 1;
  mapCtx.stroke();

  const trail = (pts, colour, width, dash) => {
    if (!pts || pts.length < 2) return;
    mapCtx.beginPath();
    pts.forEach((s, i) => (i ? mapCtx.lineTo(X(s.x), Y(s.y)) : mapCtx.moveTo(X(s.x), Y(s.y))));
    mapCtx.strokeStyle = colour;
    mapCtx.lineWidth = width;
    mapCtx.lineJoin = "round";
    mapCtx.setLineDash(dash || []);
    mapCtx.stroke();
    mapCtx.setLineDash([]);
  };

  for (const l of shown) {
    trail(l.trail, GHOST, 1.25);
    const end = (l.trail || [])[(l.trail || []).length - 1];
    if (end) {
      mapCtx.fillStyle = GHOST;
      mapCtx.font = '400 9px "IBM Plex Mono", monospace';
      mapCtx.textAlign = "left";
      mapCtx.fillText(`#${attemptNo(l.name)}`, X(end.x) + 6, Y(end.y) + 3);
    }
  }

  if (live) {
    // Where it is going, if nothing changes. The engine predicts the conic;
    // this just draws it.
    trail(live.predicted, "#2f6f78", 1, [3, 4]);
    trail(live.trail, live.status === "crashed" ? FAIL : LIVE, 2);
    rocket(X(live.x), Y(live.y), live.theta, live.status);
  }

  // A scale bar, because "how high is that" is the question the map raises.
  const barWorld = niceStep((maxX - minX) / 3);
  const barPx = barWorld * scale;
  if (barPx > 24) {
    const x0 = 22;
    const y0 = h - 22;
    mapCtx.strokeStyle = "#333b45";
    mapCtx.lineWidth = 1;
    mapCtx.beginPath();
    mapCtx.moveTo(x0, y0);
    mapCtx.lineTo(x0 + barPx, y0);
    mapCtx.stroke();
    mapCtx.fillStyle = INK3;
    mapCtx.font = '400 9px "IBM Plex Mono", monospace';
    mapCtx.textAlign = "left";
    mapCtx.fillText(`${metres(barWorld)} m`, x0, y0 - 6);
  }
}

function rocket(x, y, theta, status) {
  mapCtx.save();
  mapCtx.translate(x, y);
  mapCtx.rotate(-theta);
  mapCtx.beginPath();
  mapCtx.moveTo(7, 0);
  mapCtx.lineTo(-4, 3.6);
  mapCtx.lineTo(-4, -3.6);
  mapCtx.closePath();
  mapCtx.fillStyle = status === "crashed" ? FAIL : INK;
  mapCtx.fill();
  mapCtx.restore();
}

function niceStep(raw) {
  const pow = Math.pow(10, Math.floor(Math.log10(Math.max(raw, 1e-6))));
  const n = raw / pow;
  return (n > 5 ? 10 : n > 2 ? 5 : n > 1 ? 2 : 1) * pow;
}

/* ── hangar ───────────────────────────────────────────────────── */

/* The worlds you can launch from.
 *
 * Switching clears the pad, because everything already flown was flown under
 * a different gravity through different air and cannot share a map with what
 * comes next. The button says so before you press it.
 */
function renderWorlds(state) {
  const el = $("worlds");
  if (!el || !state.planets) return;
  const key = `${state.planets.map((p) => p.id).join("|")}#${state.planet.id}`;
  if (el.dataset.key === key) return;
  el.dataset.key = key;
  el.replaceChildren();
  for (const p of state.planets) {
    const b = document.createElement("button");
    b.className = "world";
    b.type = "button";
    b.setAttribute("role", "radio");
    b.setAttribute("aria-checked", String(p.id === state.planet.id));
    if (p.id === state.planet.id) b.dataset.active = "1";

    const n = document.createElement("b");
    n.textContent = p.name;
    const facts = document.createElement("span");
    facts.className = "world-facts";
    facts.textContent = `${p.g0.toFixed(1)} m/s² · ${p.rho0 > 0 ? "air" : "airless"}`;
    const blurb = document.createElement("span");
    blurb.className = "world-blurb";
    blurb.textContent = p.blurb;
    b.append(n, facts, blurb);

    b.onclick = () => {
      if (p.id === state.planet.id) return;
      const flown = (lastState?.launches ?? []).length;
      if (
        flown &&
        !confirm(
          `Launching from ${p.name} clears the pad: ${flown} attempt${flown === 1 ? "" : "s"} flown here will be archived. Their records stay in the database.`,
        )
      ) {
        return;
      }
      post("/api/planet", { id: p.id }).then((next) => {
        ghosts.clear();
        draw(next);
        show("hangar");
      });
    };
    el.append(b);
  }
}

/* What this part can be set to.
 *
 * A tank holds less than full if you say so, and an engine can be held below
 * its rating. Both are real: draining a tank costs delta-v and buys
 * thrust-to-weight, and limiting an engine burns proportionally less fuel, so
 * it trades climb rate for burn time. Parts with neither get nothing, rather
 * than a disabled control nobody can use.
 */
function tune(p) {
  const wrap = document.createElement("span");
  wrap.className = "part-tune";
  const knob = (label, value, max, unit, send) => {
    const l = document.createElement("label");
    l.className = "knob";
    const t = document.createElement("span");
    t.className = "knob-label";
    t.textContent = label;
    const r = document.createElement("input");
    r.type = "range";
    r.min = "0";
    r.max = String(max);
    r.step = String(max / 100);
    r.value = String(value);
    r.setAttribute("aria-label", `${label} of ${p.kind}`);
    const out = document.createElement("b");
    out.textContent = unit(value);
    // Redraw on release, not on every pixel: each change is a write, and the
    // whole stack re-renders behind it.
    r.oninput = () => {
      out.textContent = unit(Number(r.value));
    };
    r.onchange = () => send(Number(r.value)).then(draw);
    l.append(t, r, out);
    wrap.append(l);
  };

  if (p.fuel_cap_kg > 0) {
    knob(
      "Fuel",
      p.fuel_kg,
      p.fuel_cap_kg,
      (v) => `${Math.round((v / p.fuel_cap_kg) * 100)}%`,
      (v) => post("/api/vab/tune", { index: p.ordinal, fuel: v }),
    );
  }
  if (p.thrust_n > 0) {
    knob(
      "Thrust",
      p.thrust_limit,
      1,
      (v) => `${Math.round(v * 100)}%`,
      (v) => post("/api/vab/tune", { index: p.ordinal, thrust_limit: v }),
    );
  }
  return wrap;
}

/* The parts, drawn as the shapes they are. A list of names reads like a form;
 * a stack of parts reads like a rocket, which is what you are building. */
function partShape(kind) {
  const el = document.createElement("span");
  el.className = `part-shape part-${kind}`;
  return el;
}

function renderHangar(state) {
  const vab = state.vab;
  if (!vab) return;

  const stack = $("stack");
  const n = vab.parts.length;
  if (slot > n) slot = n;
  const key = `${vab.parts
    .map((p) => `${p.ordinal}:${p.kind}:${p.fuel_kg.toFixed(3)}:${p.thrust_limit.toFixed(3)}`)
    .join("|")}#${slot}`;
  if (stack.dataset.key !== key) {
    stack.dataset.key = key;

    // A slot is a place a part can go, drawn as the gap it would fill. The
    // one that is chosen is where the next part from the bin lands, so
    // "another engine" is two clicks and goes where you put it.
    const addSlot = (index, label) => {
      const li = document.createElement("li");
      li.className = "slot";
      const b = document.createElement("button");
      b.className = "slot-hit";
      b.type = "button";
      b.setAttribute("aria-label", label);
      b.setAttribute("aria-pressed", String(index === slot));
      if (index === slot) li.dataset.active = "1";
      b.onclick = () => {
        slot = index;
        if (lastState) draw(lastState);
      };
      li.append(b);
      stack.append(li);
    };

    stack.replaceChildren();
    // Ordinal 0 is the part that fires first, which is the one at the bottom.
    // A rocket is read nose first, so the display runs the other way; removal
    // still goes by ordinal, which does not move.
    [...vab.parts].reverse().forEach((p, k) => {
      addSlot(n - k, k === 0 ? "Put the next part on the nose" : `Put the next part above the ${p.kind}`);
      const li = document.createElement("li");
      li.className = "part";
      const shape = partShape(p.kind);
      const name = document.createElement("b");
      name.textContent = p.kind;
      const mass = document.createElement("span");
      mass.className = "part-mass";
      mass.textContent = `${num(p.dry_kg + p.fuel_kg, 2)} kg`;
      const rm = document.createElement("button");
      rm.className = "part-remove";
      rm.textContent = "Remove";
      rm.setAttribute("aria-label", `Remove ${p.kind}`);
      rm.onclick = () => post("/api/vab/remove", { index: p.ordinal }).then(draw);
      li.append(shape, name, tune(p), mass, rm);
      stack.append(li);
    });
    addSlot(0, "Put the next part at the base");
  }
  $("pad-empty").hidden = vab.parts.length > 0;

  const bin = $("bin");
  if (!bin.dataset.ready && vab.catalog) {
    vab.catalog.forEach((c) => {
      const b = document.createElement("button");
      b.className = "bin-part";
      b.append(partShape(c.kind));
      const name = document.createElement("b");
      name.textContent = c.kind;
      const meta = document.createElement("span");
      meta.textContent = c.thrust_n > 0 ? `${c.thrust_n} N` : `${num(c.dry_kg, 2)} kg`;
      b.append(name, meta);
      b.onclick = () => post("/api/vab/add", { part_id: c.part_id, index: slot }).then(draw);
      bin.append(b);
    });
    bin.dataset.ready = "1";
  }

  const twr = vab.wet_mass > 0 ? vab.thrust_n / (vab.wet_mass * world.g0) : 0;
  const stages = vab.parts.filter((p) => p.kind === "decoupler").length + 1;
  $("h-mass").textContent = num(vab.wet_mass, 2);
  $("h-dv").textContent = num(vab.dv_budget_mps, 0);
  $("h-twr").textContent = num(twr, 2);
  $("h-stab").textContent = `${vab.stability >= 0 ? "+" : ""}${num(vab.stability, 2)}`;
  $("h-stages").textContent = String(stages);
  $("spec-twr").dataset.state = twr < 1 ? "bad" : "ok";
  // Only worth flagging where there is air to be unstable in.
  $("spec-stab").dataset.state = world.rho0 > 0 && vab.stability < 0 ? "bad" : "ok";

  // The one number that decides whether anything happens when you press the
  // button, said before you press it.
  const warn = $("h-warn");
  if (!vab.parts.length) {
    warn.hidden = false;
    warn.textContent = "Add a capsule, a tank and an engine to get off the ground.";
  } else if (twr < 1) {
    warn.hidden = false;
    warn.textContent = `Thrust-to-weight is ${num(twr, 2)}. Under 1 the engine cannot lift its own stack: add thrust or drop mass.`;
  } else if (world.rho0 > 0 && vab.stability < 0) {
    warn.hidden = false;
    warn.textContent = `Stability is ${num(vab.stability, 2)}. The air pushes ahead of the centre of mass, so ${world.name} will swing this stack around as it picks up speed. Fins at the base fix it.`;
  } else {
    warn.hidden = true;
  }
  $("btn-launch").disabled = !vab.parts.length;
}

/* ── flight ───────────────────────────────────────────────────── */

function renderFlight(state) {
  const l = focused(state);
  if (!l) return;
  $("f-t").textContent = fmtT(l.t);
  $("f-alt").textContent = metres(alt(l));
  $("f-spd").textContent = num(Math.hypot(l.vx, l.vy), 1);
  $("f-ap").textContent = metres(l.ap);
  $("f-pe").textContent = metres(l.pe);

  const cap = Math.max(l.fuel, 1e-9);
  const pct = state.vab && state.vab.fuel > 0 ? Math.min(1, l.fuel / state.vab.fuel) : cap > 0 ? 1 : 0;
  $("f-fuel-fill").style.width = `${Math.round(pct * 100)}%`;
  $("f-fuel-fill").dataset.state = pct <= 0.001 ? "empty" : pct < 0.2 ? "low" : "ok";
  $("f-fuel").textContent = `${Math.round(pct * 100)}%`;

  const aoa = Math.abs((l.aoa ?? 0) * (180 / Math.PI));
  const out = aoa > 45 && l.status === "flying" ? { key: "spent", text: `Tumbling — ${aoa.toFixed(0)}° off course` } : outcomeOf(l);
  const banner = $("outcome");
  banner.hidden = !out.text;
  banner.textContent = out.text;
  banner.dataset.state = out.key;

  $("btn-auto").classList.toggle("active", !!l.autopilot);
  document.querySelectorAll(".warp").forEach((b) => {
    b.classList.toggle("active", Number(b.dataset.w) === l.warp);
  });
  if (!throttleDragging) $("throttle").value = String(l.throttle);
  $("btn-stage").disabled = l.status === "crashed";
}

/* ── attempts ─────────────────────────────────────────────────── */

const GLYPH = { win: "◉", fail: "✕", spent: "◌", flying: "▶", none: "·" };

function renderAttempts(state) {
  const list = $("attempts");
  const all = state.launches || [];
  $("attempts-empty").hidden = all.length > 0;

  const key = all
    .map((l) => `${l.name}:${l.status}:${Math.round(peak(l))}:${ghosts.has(l.name)}`)
    .join("|");
  if (list.dataset.key === key && list.dataset.focus === state.focused) return;
  list.dataset.key = key;
  list.dataset.focus = state.focused;

  list.replaceChildren();
  for (const l of all) {
    const out = outcomeOf(l);
    const b = document.createElement("button");
    b.className = "attempt";
    b.dataset.state = out.key;
    if (l.name === state.focused) b.dataset.live = "1";
    if (ghosts.has(l.name)) b.dataset.shown = "1";

    const g = document.createElement("span");
    g.className = "attempt-glyph";
    g.textContent = GLYPH[out.key] ?? GLYPH.none;
    const n = document.createElement("b");
    n.textContent = `#${attemptNo(l.name)}`;
    const s = document.createElement("span");
    s.className = "attempt-peak";
    s.textContent = `${metres(peak(l))} m`;
    b.append(g, n, s);

    b.title =
      l.name === state.focused
        ? "The flight you are watching"
        : ghosts.has(l.name)
          ? "Hide this attempt"
          : "Draw this attempt behind the live one";
    b.onclick = () => {
      if (l.name === state.focused) return;
      if (ghosts.has(l.name)) ghosts.delete(l.name);
      else ghosts.add(l.name);
      list.dataset.key = "";
      if (lastState) draw(lastState);
    };
    list.append(b);
  }
}

/* ── frame ────────────────────────────────────────────────────── */

function draw(state) {
  if (!state || !state.launches) return;
  lastState = state;
  if (state.planet) world = state.planet;
  renderWorlds(state);
  renderHangar(state);
  renderAttempts(state);
  if (screen === "flight") {
    renderFlight(state);
    drawMap(state);
  }
}

function show(next) {
  screen = next;
  document.body.dataset.screen = next;
  if (lastState) draw(lastState);
}

Object.defineProperty(globalThis, "kspState", { get: () => lastState });

/* ── wiring ───────────────────────────────────────────────────── */

/* One protocol, two transports. Served by the native binary this is an HTTP
 * call; compiled to wasm the same route runs in-process and KSP_LOCAL is how
 * bridge.js says so. Everything above this line is identical either way. */
function post(path, body) {
  if (globalThis.KSP_LOCAL) return globalThis.KSP_LOCAL.post(path, body);
  return fetch(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: body ? JSON.stringify(body) : "{}",
  }).then((r) => r.json());
}

$("btn-launch").onclick = () =>
  post("/api/launch").then((s) => {
    ghosts.clear();
    draw(s);
    show("flight");
  });

$("btn-hangar").onclick = () => {
  // Stop the clock on the way out: coming back to a rocket that kept flying
  // while you were choosing parts is not what "back to the hangar" means.
  post("/api/pause").then((s) => {
    draw(s);
    show("hangar");
  });
};

/* Revert is the whole reason the loop is worth repeating. The same attempt
 * goes back to the pad - underneath, an as-of read of this branch at the
 * event it launched on. */
$("btn-revert").onclick = () => {
  const l = lastState && focused(lastState);
  if (!l) return;
  const first = (l.trail || [])[0];
  if (!first) return;
  post("/api/rewind", { launch: l.name, seq: first.seq }).then(draw);
};

$("btn-stage").onclick = () => post("/api/stage").then(draw);
$("btn-auto").onclick = () => {
  const on = !$("btn-auto").classList.contains("active");
  post("/api/autopilot", { on }).then(draw);
};
document.querySelectorAll(".warp").forEach((b) => {
  b.onclick = () => post("/api/warp", { mult: Number(b.dataset.w) }).then(draw);
});

const throttleEl = $("throttle");
throttleEl.onpointerdown = () => {
  throttleDragging = true;
};
throttleEl.onpointerup = () => {
  throttleDragging = false;
};
throttleEl.oninput = () => post("/api/throttle", { value: Number(throttleEl.value) }).then(draw);

window.addEventListener("resize", () => lastState && draw(lastState));

function connect() {
  const proto = location.protocol === "https:" ? "wss" : "ws";
  const ws = new WebSocket(`${proto}://${location.host}/ws`);
  ws.onmessage = (ev) => {
    try {
      draw(JSON.parse(ev.data));
    } catch (error) {
      // Never silently: a throw in here leaves the frame half drawn and the
      // console empty, which is how a missing variable survived a screenshot
      // review. Report it and keep the socket alive.
      console.error("draw failed", error);
    }
  };
  ws.onclose = () => setTimeout(connect, 800);
}

if (globalThis.KSP_LOCAL) {
  globalThis.KSP_LOCAL.post("/api/state").then(draw);
  globalThis.KSP_LOCAL.subscribe(draw);
} else {
  fetch("/api/state")
    .then((r) => r.json())
    .then(draw)
    .finally(connect);
}
