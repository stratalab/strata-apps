const GFP = [215, 238, 192];
const POLLEN = [227, 176, 58];
const STAIN = [196, 122, 255];
const VOID = [7, 6, 10];

let state = null;
let focused = "control";
const plate = document.getElementById("plate");
const focusCanvas = document.getElementById("focus");
const wells = new Map();

function decodeBoard(b64, width, height) {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return { bytes, width, height };
}

function liveAt(board, x, y) {
  const i = y * board.width + x;
  return (board.bytes[i >> 3] & (1 << (i & 7))) !== 0;
}

function paint(canvas, colony, control, { showMissing, cell }) {
  const ctx = canvas.getContext("2d");
  const w = state.width;
  const h = state.height;
  const cw = Math.max(1, Math.floor(canvas.width / w));
  const ch = Math.max(1, Math.floor(canvas.height / h));
  canvas.width = w * cw;
  canvas.height = h * ch;
  const img = ctx.createImageData(canvas.width, canvas.height);
  const data = img.data;
  const board = decodeBoard(colony.board, w, h);
  const ctrl = control ? decodeBoard(control.board, w, h) : null;

  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const mine = liveAt(board, x, y);
      const theirs = ctrl ? liveAt(ctrl, x, y) : mine;
      let rgb = VOID;
      if (mine && theirs) rgb = GFP;
      else if (mine && !theirs) rgb = POLLEN;
      else if (!mine && theirs && showMissing) rgb = STAIN;
      const gap = cell > 2 ? 1 : 0;
      for (let py = 0; py < ch - gap; py++) {
        for (let px = 0; px < cw - gap; px++) {
          const idx = ((y * ch + py) * canvas.width + (x * cw + px)) * 4;
          data[idx] = rgb[0];
          data[idx + 1] = rgb[1];
          data[idx + 2] = rgb[2];
          data[idx + 3] = 255;
        }
      }
    }
  }
  ctx.putImageData(img, 0, 0);
}

function ensureWells(colonies) {
  if (plate.childElementCount === colonies.length) return;
  plate.innerHTML = "";
  wells.clear();
  for (const colony of colonies) {
    const el = document.createElement("button");
    el.type = "button";
    el.className = "well";
    el.dataset.name = colony.name;
    const canvas = document.createElement("canvas");
    canvas.width = 128;
    canvas.height = 96;
    const tag = document.createElement("span");
    tag.className = "tag";
    tag.textContent = colony.name;
    el.append(canvas, tag);
    el.addEventListener("click", () => {
      focused = colony.name;
      render(state);
    });
    plate.append(el);
    wells.set(colony.name, { el, canvas });
  }
}

function renderSmear(colonies) {
  const svg = document.getElementById("smear");
  const maxH = Math.max(
    1,
    ...colonies.flatMap((c) => c.history),
    ...colonies.map((c) => c.divergence)
  );
  const w = 1000;
  const h = 120;
  svg.innerHTML = "";
  for (const colony of colonies) {
    const hist = colony.history.length ? colony.history : [colony.divergence];
    const pts = hist
      .map((v, i) => {
        const x = hist.length === 1 ? 0 : (i / (hist.length - 1)) * w;
        const y = h - 6 - (v / maxH) * (h - 12);
        return `${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(" ");
    const line = document.createElementNS("http://www.w3.org/2000/svg", "polyline");
    line.setAttribute("points", pts);
    if (colony.name === "control") line.classList.add("control");
    if (colony.name === focused) line.style.opacity = "1";
    svg.append(line);
  }
}

function renderFindings(findings) {
  const ol = document.getElementById("findings");
  ol.innerHTML = "";
  for (const finding of findings) {
    const li = document.createElement("li");
    const kind = document.createElement("span");
    kind.className = "kind";
    kind.textContent = `${finding.kind} · ${finding.surface}`;
    const title = document.createElement("h3");
    title.textContent = finding.title;
    const detail = document.createElement("p");
    detail.textContent = finding.detail;
    li.append(kind, title, detail);
    ol.append(li);
  }
}

function render(snapshot) {
  if (!snapshot) return;
  state = snapshot;
  const control = snapshot.colonies[0];
  const current =
    snapshot.colonies.find((c) => c.name === focused) || control;

  document.getElementById("gen").textContent = snapshot.generation;
  document.getElementById("persist").textContent = snapshot.persist_ms.toFixed(1);
  document.getElementById("commits").textContent = snapshot.total_commits;
  document.getElementById("focus-name").textContent = current.name;
  document.getElementById("focus-kicker").textContent =
    current.name === "control" ? "unflipped seed" : "one-cell lie";
  document.getElementById("the-lie").textContent = current.perturbed
    ? `the lie is cell (${current.perturbed[0]}, ${current.perturbed[1]})`
    : "no cell was flipped";
  document.getElementById("div").textContent = current.divergence;
  document.getElementById("live").textContent = current.live;
  document.getElementById("hash").textContent = current.fingerprint.slice(0, 8);
  document.getElementById("db-line").textContent =
    `${snapshot.durable ? "durable" : "cache"} · ${snapshot.db_path} · ${snapshot.persist_mode} persist · ${snapshot.width}×${snapshot.height} · avg ${snapshot.avg_persist_ms.toFixed(1)}ms`;
  document.getElementById("btn-run").textContent = snapshot.running ? "Running" : "Run";
  if (document.activeElement !== hz) {
    hz.value = snapshot.tick_hz;
    document.getElementById("hz-val").textContent = Number(snapshot.tick_hz.toFixed(1));
  }

  ensureWells(snapshot.colonies);
  for (const colony of snapshot.colonies) {
    const well = wells.get(colony.name);
    if (!well) continue;
    well.el.classList.toggle("focused", colony.name === focused);
    paint(well.canvas, colony, control, { showMissing: false, cell: 2 });
  }
  paint(focusCanvas, current, control, { showMissing: true, cell: 8 });
  renderSmear(snapshot.colonies);
  renderFindings(snapshot.findings || []);
}

function showError(error) {
  const el = document.getElementById("error");
  el.textContent = error.message;
  el.hidden = false;
}

async function post(path, body) {
  try {
    const response = await fetch(path, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body ?? {}),
    });
    const snapshot = await response.json();
    if (!response.ok) throw new Error(snapshot.error || `Request failed (${response.status})`);
    document.getElementById("error").hidden = true;
    return snapshot;
  } catch (error) {
    showError(error);
    return null;
  }
}

document.getElementById("btn-run").onclick = () => post("/api/run").then(render);
document.getElementById("btn-pause").onclick = () => post("/api/pause").then(render);
document.getElementById("btn-step").onclick = () => post("/api/step").then(render);
document.getElementById("btn-reset").onclick = () => post("/api/reset").then(render);
document.getElementById("btn-audit").onclick = () => post("/api/audit").then(render);
document.getElementById("btn-compare").onclick = () =>
  post("/api/compare", { colony: focused }).then(render);
document.getElementById("btn-rewind").onclick = () => {
  const generation = Number(document.getElementById("rewind-gen").value);
  post("/api/rewind", { generation }).then(render);
};

const hz = document.getElementById("hz");
hz.oninput = () => {
  document.getElementById("hz-val").textContent = hz.value;
};
hz.onchange = () => post("/api/speed", { hz: Number(hz.value) }).then(render);

focusCanvas.addEventListener("click", (event) => {
  if (!state) return;
  const rect = focusCanvas.getBoundingClientRect();
  const x = Math.floor(((event.clientX - rect.left) / rect.width) * state.width);
  const y = Math.floor(((event.clientY - rect.top) / rect.height) * state.height);
  post("/api/perturb", { colony: focused, x, y }).then(render);
});

function connect() {
  const proto = location.protocol === "https:" ? "wss" : "ws";
  const ws = new WebSocket(`${proto}://${location.host}/ws`);
  ws.onmessage = (ev) => {
    try {
      render(JSON.parse(ev.data));
    } catch {
      /* ignore */
    }
  };
  ws.onclose = () => setTimeout(connect, 800);
}

fetch("/api/state")
  .then(async (r) => {
    const snapshot = await r.json();
    if (!r.ok) throw new Error(snapshot.error || `Request failed (${r.status})`);
    return snapshot;
  })
  .then(render)
  .catch(showError)
  .finally(connect);
