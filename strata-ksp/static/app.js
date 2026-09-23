const R = 200;
const WIN_R = 224;
const canvas = document.getElementById("plot");
const ctx = canvas.getContext("2d");
let lastState = null;
let scrubbing = false;
let throttleDragging = false;

function fmtT(t) {
  const s = Math.max(0, t);
  const m = Math.floor(s / 60);
  const rem = s - m * 60;
  return `${String(m).padStart(2, "0")}:${rem.toFixed(1).padStart(4, "0")}`;
}

function fmtAlt(h) {
  if (!Number.isFinite(h)) return "∞";
  return h.toFixed(1);
}

function worldToCanvas(x, y, scale, cx, cy) {
  return [cx + x * scale, cy - y * scale];
}

function fiducial(x, y, dx, dy) {
  ctx.beginPath();
  ctx.moveTo(x, y + dy * 10);
  ctx.lineTo(x, y);
  ctx.lineTo(x + dx * 10, y);
  ctx.stroke();
}

function drawSpark(cv, L, focused) {
  const c = cv.getContext("2d");
  const w = cv.width;
  const h = cv.height;
  c.fillStyle = "#070b12";
  c.fillRect(0, 0, w, h);
  const trail = L.trail || [];
  let maxR = R * 1.35;
  for (const p of trail) {
    const r = Math.hypot(p.x, p.y);
    if (r > maxR) maxR = r;
  }
  const scale = Math.min(w, h) / (Math.max(maxR, WIN_R) * 2.55);
  const cx = w / 2;
  const cy = h / 2;
  c.beginPath();
  c.arc(cx, cy, R * scale, 0, Math.PI * 2);
  c.fillStyle = "#1a140e";
  c.fill();
  c.strokeStyle = "#e7dcc8";
  c.globalAlpha = 0.25;
  c.lineWidth = 0.6;
  c.stroke();
  c.globalAlpha = 1;
  if (trail.length < 2) return;
  c.beginPath();
  trail.forEach((p, i) => {
    const x = cx + p.x * scale;
    const y = cy - p.y * scale;
    if (i === 0) c.moveTo(x, y);
    else c.lineTo(x, y);
  });
  c.strokeStyle = focused ? "#d9e6f2" : "#6a7a88";
  c.globalAlpha = focused ? 0.9 : 0.5;
  c.lineWidth = 1;
  c.stroke();
  c.globalAlpha = 1;
}

function renderFilm(state) {
  const list = document.getElementById("film-list");
  const launches = (state.launches || []).filter((L) =>
    String(L.name || "").startsWith("launch-"),
  );
  const keep = new Set();
  const existing = new Map();
  list.querySelectorAll(".item").forEach((el) => existing.set(el.dataset.name, el));
  for (const L of launches) {
    keep.add(L.name);
    let el = existing.get(L.name);
    if (!el) {
      el = document.createElement("button");
      el.type = "button";
      el.className = "item";
      el.dataset.name = L.name;
      const cv = document.createElement("canvas");
      cv.width = 64;
      cv.height = 48;
      const copy = document.createElement("span");
      copy.className = "item-copy";
      copy.innerHTML = `<span class="item-name"></span><span class="item-stat"></span>`;
      el.append(cv, copy);
      el.onclick = () => post("/api/focus", { launch: L.name }).then(draw);
      list.append(el);
    }
    const focused = L.name === state.focused;
    el.classList.toggle("active", focused);
    el.querySelector(".item-name").textContent = L.name;
    const design = (L.design || "").replace("design-", "d-");
    el.querySelector(".item-stat").textContent = `${L.status}  ${design}`;
    drawSpark(el.querySelector("canvas"), L, focused);
  }
  for (const [name, el] of existing) {
    if (!keep.has(name)) el.remove();
  }
}

function draw(state) {
  lastState = state;
  const launch =
    (state.launches || []).find((l) => l.name === state.focused) ||
    (state.launches || [])[0];

  document.getElementById("vab-name").textContent = state.vab.name;
  document.getElementById("wet").textContent = state.vab.wet_mass.toFixed(2);
  document.getElementById("dv").textContent = `${state.vab.dv_budget_mps.toFixed(0)} m/s`;
  document.getElementById("db-line").textContent = state.db_path;
  document.getElementById("branches").textContent = state.branch_count;
  document.getElementById("persist").textContent =
    state.persist_ms > 0 ? `${state.persist_ms.toFixed(1)} ms` : "—";
  document.getElementById("btn-run").classList.toggle("active", state.running);
  document.getElementById("btn-pause").classList.toggle("active", !state.running);

  const autoBtn = document.getElementById("btn-auto");
  const throttleEl = document.getElementById("throttle");
  if (!launch) {
    document.getElementById("t").textContent = "00:00.0";
    document.getElementById("seq").textContent = "0";
    autoBtn.classList.remove("active");
    autoBtn.setAttribute("aria-pressed", "false");
    throttleEl.disabled = true;
  } else {
    document.getElementById("t").textContent = fmtT(launch.t);
    document.getElementById("seq").textContent = launch.seq;
    autoBtn.classList.toggle("active", launch.autopilot);
    autoBtn.setAttribute("aria-pressed", launch.autopilot ? "true" : "false");
    throttleEl.disabled = !!launch.autopilot;
    if (!throttleDragging) throttleEl.value = String(launch.throttle ?? 0);
  }

  const note = document.getElementById("promote-note");
  if (state.last_promote) {
    const p = state.last_promote;
    note.hidden = false;
    const un = (p.unsupported || []).join(", ") || "none";
    note.textContent = p.ok
      ? `${p.note || "Promoted JSON spec."} unsupported: ${un}`
      : `${p.note || "promote refused"} (${p.strategy})`;
  } else {
    note.hidden = true;
  }

  const cmp = document.getElementById("compare-note");
  if (state.last_compare) {
    const c = state.last_compare;
    cmp.hidden = false;
    cmp.textContent = `compare +${c.added} −${c.removed} ~${c.modified}  json ${c.json_entities}  kv ${c.kv_entities}  event ${c.event_entities}  graph ${c.graph_entities}`;
  } else if (state.last_archive) {
    const a = state.last_archive;
    cmp.hidden = false;
    cmp.textContent = a.design_deleted
      ? `archived ${a.launch}; dropped ${a.design}`
      : `archived ${a.launch}; ${a.design || "design"} refcount ${a.remaining_refcount}`;
  } else {
    cmp.hidden = true;
  }

  const findings = state.findings || [];
  document.getElementById("findings-n").textContent = findings.length;
  const list = document.getElementById("findings");
  list.innerHTML = "";
  findings.forEach((f) => {
    const li = document.createElement("li");
    const issue = f.issue
      ? ` <a class="issue" href="${f.issue}">#${String(f.issue).split("/").pop()}</a>`
      : "";
    li.innerHTML = `<span class="kind">${f.kind}</span> <strong>${f.title}</strong>${issue} — ${f.detail}`;
    list.append(li);
  });

  const stack = document.getElementById("stack");
  stack.innerHTML = "";
  (state.vab.parts || []).forEach((p) => {
    const li = document.createElement("li");
    if (p.kind === "decoupler") li.className = "sep";
    li.innerHTML = `<span>${p.ordinal} ${p.part_id}</span>`;
    const rm = document.createElement("button");
    rm.type = "button";
    rm.textContent = "×";
    rm.onclick = () => post("/api/vab/remove", { index: p.ordinal }).then(draw);
    li.append(rm);
    stack.append(li);
  });

  const catalog = document.getElementById("catalog");
  if (!catalog.dataset.ready && state.vab.catalog) {
    state.vab.catalog.forEach((c) => {
      const b = document.createElement("button");
      b.type = "button";
      b.textContent = `+ ${c.part_id}`;
      b.onclick = () => post("/api/vab/add", { part_id: c.part_id }).then(draw);
      catalog.append(b);
    });
    catalog.dataset.ready = "1";
  }

  renderFilm(state);

  const warp = launch ? launch.warp : 1;
  document.querySelectorAll(".warp").forEach((btn) => {
    btn.classList.toggle("active", Number(btn.dataset.w) === warp);
  });

  const w = canvas.width;
  const h = canvas.height;
  ctx.fillStyle = "#070b12";
  ctx.fillRect(0, 0, w, h);

  const scale = Math.min(w, h) / (R * 4.2);
  const cx = w / 2;
  const cy = h / 2;

  ctx.strokeStyle = "#1c2430";
  ctx.lineWidth = 1;
  ctx.globalAlpha = 1;
  fiducial(18, 18, 1, 1);
  fiducial(w - 18, 18, -1, 1);
  fiducial(18, h - 18, 1, -1);
  fiducial(w - 18, h - 18, -1, -1);

  ctx.beginPath();
  ctx.arc(cx, cy, WIN_R * scale, 0, Math.PI * 2);
  ctx.setLineDash([2, 5]);
  ctx.strokeStyle = "#c4a574";
  ctx.globalAlpha = 0.28;
  ctx.lineWidth = 1;
  ctx.stroke();
  ctx.setLineDash([]);
  ctx.globalAlpha = 1;

  ctx.beginPath();
  ctx.arc(cx, cy, R * scale, 0, Math.PI * 2);
  ctx.fillStyle = "#1a140e";
  ctx.fill();
  ctx.strokeStyle = "#e7dcc8";
  ctx.lineWidth = 1;
  ctx.globalAlpha = 0.35;
  ctx.stroke();
  ctx.globalAlpha = 1;

  ctx.beginPath();
  ctx.arc(cx, cy, R * scale, -0.55, 0.85);
  ctx.strokeStyle = "#e7dcc8";
  ctx.globalAlpha = 0.55;
  ctx.lineWidth = 1.25;
  ctx.stroke();
  ctx.globalAlpha = 1;

  ctx.fillStyle = "#8a8174";
  ctx.font = '11px "IBM Plex Sans", system-ui, sans-serif';
  ctx.fillText("kerb", cx - 14, cy + R * scale + 16);

  const bar = 100 * scale;
  ctx.strokeStyle = "#c4a574";
  ctx.globalAlpha = 0.7;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(28, h - 36);
  ctx.lineTo(28 + bar, h - 36);
  ctx.stroke();
  ctx.globalAlpha = 1;
  ctx.fillStyle = "#c4a574";
  ctx.font = '10px "IBM Plex Mono", ui-monospace, monospace';
  ctx.fillText("100 m", 28, h - 22);

  if (!launch) return;

  const scrub = document.getElementById("scrub");
  const maxSeq = launch.seq || 0;
  if (!scrubbing) {
    scrub.max = String(maxSeq);
    if (Number(scrub.value) > maxSeq) scrub.value = String(maxSeq);
    if (Number(scrub.value) === 0 && maxSeq > 0) scrub.value = String(maxSeq);
  }
  const scrubSeq = Number(scrub.value);

  (state.launches || []).forEach((L) => {
    const focused = L.name === state.focused;
    const pts = (L.trail || []).filter((p) => !focused || (p.seq || 0) <= scrubSeq);
    if (pts.length < 2) return;
    ctx.beginPath();
    pts.forEach((p, i) => {
      const [px, py] = worldToCanvas(p.x, p.y, scale, cx, cy);
      if (i === 0) ctx.moveTo(px, py);
      else ctx.lineTo(px, py);
    });
    ctx.strokeStyle = focused ? "#d9e6f2" : "#6a7a88";
    ctx.globalAlpha = focused ? 0.9 : 0.35;
    ctx.lineWidth = focused ? 1.5 : 1;
    ctx.stroke();
    ctx.globalAlpha = 1;
  });

  if (launch.predicted && launch.predicted.length > 1) {
    ctx.beginPath();
    launch.predicted.forEach((p, i) => {
      const [px, py] = worldToCanvas(p.x, p.y, scale, cx, cy);
      if (i === 0) ctx.moveTo(px, py);
      else ctx.lineTo(px, py);
    });
    ctx.closePath();
    ctx.setLineDash([5, 6]);
    ctx.strokeStyle = "#3d6b5a";
    ctx.globalAlpha = 0.4;
    ctx.lineWidth = 1;
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.globalAlpha = 1;
  }

  const ghost = (launch.trail || []).find((p) => p.seq === scrubSeq);
  if (ghost && ghost.seq !== launch.seq) {
    const [gx, gy] = worldToCanvas(ghost.x, ghost.y, scale, cx, cy);
    ctx.beginPath();
    ctx.arc(gx, gy, 4, 0, Math.PI * 2);
    ctx.strokeStyle = "#c4a574";
    ctx.globalAlpha = 0.8;
    ctx.stroke();
    ctx.globalAlpha = 1;
  }

  const [sx, sy] = worldToCanvas(launch.x, launch.y, scale, cx, cy);
  ctx.save();
  ctx.translate(sx, sy);
  ctx.rotate(-launch.theta);
  ctx.beginPath();
  ctx.moveTo(8, 0);
  ctx.lineTo(-6, 5);
  ctx.lineTo(-6, -5);
  ctx.closePath();
  ctx.fillStyle = "#e7dcc8";
  ctx.fill();
  if (launch.throttle > 0.05) {
    ctx.beginPath();
    ctx.moveTo(-6, 0);
    ctx.lineTo(-14, 3);
    ctx.lineTo(-14, -3);
    ctx.closePath();
    ctx.fillStyle = "#ff6a3d";
    ctx.fill();
  }
  ctx.restore();

  ctx.font = '11px "IBM Plex Mono", ui-monospace, monospace';
  ctx.fillStyle = "#e7dcc8";
  ctx.fillText(`${launch.name}  ${launch.design}`, 28, 36);
  ctx.fillStyle = "#c4a574";
  ctx.fillText(String(launch.status), 28, 52);
  ctx.fillStyle = "#e7dcc8";
  ctx.fillText(`pe ${fmtAlt(launch.pe)} m`, 28, 72);
  ctx.fillText(`ap ${fmtAlt(launch.ap)} m`, 28, 88);
  ctx.fillText(`e  ${launch.e.toFixed(3)}`, 28, 104);
  ctx.fillStyle = "#8a8174";
  ctx.fillText(`${launch.mass.toFixed(2)} kg`, 28, 120);
}

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

