import { ColoniesClient } from "./client.mjs";
import { decode, encode, live, divergence } from "./life.mjs";
import { EditDraft } from "./edit-draft.mjs";
import { BranchExplorer } from "./branch-explorer.mjs";

const $ = (id) => document.getElementById(id);
const label = (name) =>
  name === "control"
    ? "Original"
    : name.startsWith("experiment-")
      ? `Colony ${name.slice(11).padStart(2, "0")}`
      : `Your world ${name.slice(5).padStart(2, "0")}`;
const number = (value) => value.toLocaleString("en-US");
const css = getComputedStyle(document.documentElement);
const color = (name) => css.getPropertyValue(name).trim();
const palette = {
  background: color("--color-inset"),
  grid: color("--cell-grid"),
  shared: color("--cell-shared"),
  added: color("--cell-added"),
  missing: color("--cell-missing"),
  cursor: color("--cell-cursor"),
};
const cards = new Map(),
  histories = new Map(),
  historicalBoards = new Map(),
  pendingReads = new Set(),
  failedReads = new Map();
let client,
  state,
  selected = "experiment-1",
  viewing = null,
  running = false,
  busy = false,
  forkId = 1,
  hover = null,
  showDifferences = false,
  frame = 0,
  generation = 0;
let draft = null,
  tool = "auto",
  beforePreview = false,
  pointer = null;
let replaying = false,
  replayTimer,
  replayEpoch = 0;
let scrubbing = false,
  scrubTarget = null;
let compareTarget = "control",
  comparisonView = "overlay";
const explorer = new BranchExplorer({
  label,
  request,
  navigate: (name, checkpointId) => navigateBranch(name, checkpointId, true),
  redraw: scheduleRender,
});

function announce(message) {
  $("announcement").textContent = message;
}
function error(message) {
  $("error").textContent = message;
  $("error-banner").hidden = !message;
  $("retry").hidden = !!state && !client?.closed;
}
function connect() {
  const connection = new ColoniesClient((event) => {
    if (connection !== client) return;
    if (event.event === "tick") {
      accept(event.result);
      scheduleRender();
    }
    if (event.error) {
      running = false;
      error(event.error);
      renderControls();
    }
  });
  client = connection;
  window.coloniesClient = client;
}
function accept(snapshot) {
  state = snapshot;
  for (const branch of state.branches) {
    const history = (histories.get(branch.name) ?? []).filter(
      (checkpoint) => checkpoint.ordinal >= branch.oldestOrdinal,
    );
    if (history.at(-1)?.id !== branch.head.id) history.push(branch.head);
    histories.set(branch.name, history);
  }
}
async function request(type, args) {
  return (await client.request(type, args)).result;
}
async function syncHistories() {
  for (const branch of state.branches)
    histories.set(branch.name, await request("history", { name: branch.name }));
}
async function pause() {
  stopReplay();
  accept(await request("pause"));
  running = false;
}

async function action(fn) {
  if (busy) return;
  busy = true;
  renderControls();
  try {
    error("");
    await fn();
  } catch (failure) {
    error(failure.message);
  } finally {
    busy = false;
    render();
  }
}
async function initialize() {
  stopReplay();
  draft = null;
  pointer = null;
  beforePreview = false;
  generation++;
  running = false;
  viewing = null;
  hover = null;
  histories.clear();
  historicalBoards.clear();
  pendingReads.clear();
  failedReads.clear();
  compareTarget = "control";
  comparisonView = "overlay";
  if (!client || client.closed) connect();
  $("loading").hidden = false;
  $("loading").querySelector("span").textContent = "Preparing your colonies";
  $("loading").querySelector("small").textContent =
    "Loading the Strata engine in this tab…";
  $("workspace").setAttribute("aria-busy", "true");
  try {
    accept(await request("init"));
    selected = "experiment-1";
    forkId = 1;
    await syncHistories();
    $("loading").hidden = true;
    announce(
      "Your worlds are ready. Press Play, or tap a cell to make a change.",
    );
  } catch (failure) {
    state = undefined;
    $("loading").querySelector("span").textContent =
      "Your experiment could not start";
    $("loading").querySelector("small").textContent =
      "Use Try again to reload the browser engine.";
    throw failure;
  } finally {
    $("workspace").setAttribute("aria-busy", "false");
  }
}

function scheduleRender() {
  if (!frame)
    frame = requestAnimationFrame(() => {
      frame = 0;
      render();
    });
}
function current() {
  const branch = state?.branches.find((branch) => branch.name === selected);
  return branch
    ? {
        branch,
        checkpoint: draft?.checkpoint ?? viewing?.checkpoint ?? branch.head,
        board: draft
          ? beforePreview
            ? encode(draft.original)
            : draft.encoded
          : (viewing?.board ?? branch.board),
      }
    : null;
}
function referenceName() {
  return compareTarget === "parent"
    ? (current()?.branch.parent?.name ?? null)
    : "control";
}
function comparisonAt(checkpoint) {
  const name = referenceName();
  const reference = state.branches.find((b) => b.name === name);
  if (!reference)
    return {
      name,
      board: null,
      message: "Original has no parent. Choose Original as the reference.",
    };
  const found =
    reference.head.generation === checkpoint.generation
      ? reference.head
      : histories
          .get(name)
          ?.findLast((c) => c.generation === checkpoint.generation);
  if (!found)
    return {
      name,
      board: null,
      message: `${label(name)} has no saved board at generation ${checkpoint.generation}. Its history runs from generation ${histories.get(name)?.[0]?.generation ?? reference.head.generation} to ${reference.head.generation}.`,
    };
  const board =
    found.id === reference.head.id
      ? reference.board
      : historicalBoards.get(found.id);
  if (board)
    return {
      name,
      board,
      checkpoint: found,
      message: `${label(name)} · generation ${found.generation}, revision ${found.revision}. Latest saved revision at this generation.`,
    };
  if (failedReads.has(found.id))
    return {
      name,
      board: null,
      failed: true,
      message: `Could not load ${label(name)} at generation ${found.generation}: ${failedReads.get(found.id)}`,
    };
  if (!pendingReads.has(found.id)) {
    pendingReads.add(found.id);
    const epoch = generation;
    request("read", { name, checkpoint: found.id })
      .then((result) => {
        if (epoch !== generation) return;
        historicalBoards.set(found.id, result.board);
        if (historicalBoards.size > 128)
          historicalBoards.delete(historicalBoards.keys().next().value);
      })
      .catch((failure) => {
        if (epoch === generation) failedReads.set(found.id, failure.message);
      })
      .finally(() => {
        if (epoch === generation) {
          pendingReads.delete(found.id);
          scheduleRender();
        }
      });
  }
  return {
    name,
    board: null,
    message: `Loading ${label(name)} at generation ${found.generation}…`,
  };
}
async function selectBranch(name, checkpointId) {
  if (draft) return;
  // Choosing a live preview changes the camera, not the simulation clock.
  // Explicit historical navigation still pauses so that moment stays editable.
  if (checkpointId !== undefined) await pause();
  else stopReplay();
  if (!running) {
    accept(await request("snapshot"));
    await syncHistories();
  }
  const result = checkpointId
    ? await request("read", { name, checkpoint: checkpointId })
    : null;
  selected = name;
  viewing =
    result?.checkpoint.id ===
    state.branches.find((b) => b.name === name).head.id
      ? null
      : result;
  hover = null;
  announce(
    `${label(name)} selected${result ? ` at generation ${result.checkpoint.generation}, revision ${result.checkpoint.revision}` : ""}.`,
  );
}
async function navigateBranch(name, checkpointId, reveal = false) {
  if (busy || draft) return;
  await action(() => selectBranch(name, checkpointId));
  if (reveal && selected === name) {
    document.querySelector(".stage-frame").scrollIntoView({ block: "start" });
    $("focus").focus({ preventScroll: true });
  }
}
function paint(
  canvas,
  encoded,
  baseline,
  {
    focus = false,
    changes = [],
    overlay = showDifferences,
    interactive = true,
  } = {},
) {
  const board = decode(encoded),
    control = baseline ? decode(baseline) : null;
  const ctx = canvas.getContext("2d");
  const sx = canvas.width / state.width,
    sy = canvas.height / state.height;
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  if (focus) {
    ctx.strokeStyle = palette.grid;
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (let x = 0; x <= state.width; x++) {
      ctx.moveTo(x * sx + 0.5, 0);
      ctx.lineTo(x * sx + 0.5, canvas.height);
    }
    for (let y = 0; y <= state.height; y++) {
      ctx.moveTo(0, y * sy + 0.5);
      ctx.lineTo(canvas.width, y * sy + 0.5);
    }
    ctx.stroke();
  }
  const gap = focus ? Math.max(1, sx * 0.12) : 0;
  for (let y = 0; y < state.height; y++)
    for (let x = 0; x < state.width; x++) {
      const mine = live(board, state.width, x, y),
        theirs = control ? live(control, state.width, x, y) : mine;
      if (mine) {
        ctx.fillStyle = overlay && !theirs ? palette.added : palette.shared;
        ctx.fillRect(x * sx + gap, y * sy + gap, sx - gap * 2, sy - gap * 2);
      } else if (theirs && overlay && focus) {
        ctx.strokeStyle = palette.missing;
        ctx.lineWidth = Math.max(1, sx * 0.1);
        ctx.strokeRect(
          x * sx + gap * 2,
          y * sy + gap * 2,
          sx - gap * 4,
          sy - gap * 4,
        );
      }
    }
  if (focus) {
    for (const [x, y] of changes) {
      ctx.strokeStyle = palette.added;
      ctx.lineWidth = 1;
      ctx.strokeRect(x * sx - 2, y * sy - 2, sx + 4, sy + 4);
    }
    if (hover && interactive) {
      ctx.strokeStyle = palette.cursor;
      ctx.lineWidth = 2;
      ctx.strokeRect(hover[0] * sx + 1, hover[1] * sy + 1, sx - 2, sy - 2);
    }
  }
}

function renderCards() {
  const names = new Set(state.branches.map((branch) => branch.name));
  for (const [name, card] of cards)
    if (!names.has(name)) {
      card.button.remove();
      cards.delete(name);
    }
  for (const branch of state.branches) {
    let card = cards.get(branch.name);
    if (!card) {
      const button = document.createElement("button");
      button.className = "colony-card";
      button.dataset.branch = branch.name;
      const canvas = document.createElement("canvas");
      canvas.width = 256;
      canvas.height = 192;
      canvas.setAttribute("aria-hidden", "true");
      const caption = document.createElement("span");
      caption.className = "colony-caption";
      const title = document.createElement("strong");
      title.textContent = label(branch.name);
      const meta = document.createElement("small");
      caption.append(title, meta);
      button.append(canvas, caption);
      button.onclick = () => navigateBranch(branch.name);
      $("colonies").append(button);
      card = { button, canvas, meta };
      cards.set(branch.name, card);
    }
    card.button.setAttribute("aria-pressed", String(branch.name === selected));
    card.button.setAttribute(
      "aria-label",
      `${label(branch.name)}, generation ${branch.head.generation}`,
    );
    card.button.disabled = busy || !!draft;
    card.meta.textContent = String(branch.head.generation).padStart(3, "0");
    paint(card.canvas, branch.board, null);
  }
  $("colony-count").textContent = String(state.branches.length).padStart(
    2,
    "0",
  );
  const status = `${state.branches.length} colonies ready in your browser.`;
  if ($("status").textContent !== status) $("status").textContent = status;
}

function renderControls() {
  renderSpeed();
  const unavailable = !state || busy || client?.closed;
  const historical = !!viewing;
  for (const id of [
    "play",
    "pause",
    "step",
    "fork",
    "reset",
    "speed",
    "compare-toggle",
    "latest",
    "compare-target",
    "view-overlay",
    "view-split",
    "visit-origin",
  ])
    $(id).disabled = !!unavailable;
  $("play").hidden = running || replaying;
  $("pause").hidden = !running && !replaying;
  $("play").disabled ||= !!draft && (!draft.changes.length || pointer !== null);
  $("step").disabled ||= !!draft;
  $("play").querySelector("span").textContent = draft
    ? "Play with changes"
    : historical
      ? "Replay"
      : "Play";
  $("quick-change").hidden = !draft;
  $("change-note").textContent = draft
    ? `${draft.changes.length} cell${draft.changes.length === 1 ? "" : "s"} changed`
    : "";
  $("quick-undo").disabled =
    !!unavailable || pointer !== null || !draft?.undoStack.length;
  $("quick-cancel").disabled = !!unavailable || pointer !== null;
  $("play-invitation").textContent = draft
    ? "Ready to see what happens?"
    : historical
      ? "You’re in the past. Change a cell to try a new future."
      : "One little change can change everything.";
  $("step").setAttribute(
    "aria-label",
    historical
      ? "Next recorded checkpoint"
      : "Advance all colonies one generation",
  );
  $("step").title = historical
    ? "Next recorded checkpoint"
    : "Advance all colonies one generation";
  $("history").disabled =
    (!scrubbing && !!unavailable) ||
    !!draft ||
    (histories.get(selected)?.length ?? 0) < 2;
  $("latest").disabled ||= !!draft;
  $("visit-origin").disabled ||= !!draft;
  $("view-overlay").disabled ||= pointer !== null;
  $("view-split").disabled ||= pointer !== null;
  $("compare-target").value = compareTarget;
  const parent = current()?.branch.parent;
  $("compare-target").querySelector('[value="parent"]').textContent = parent
    ? `Parent · ${label(parent.name)}`
    : "Parent (none)";
  $("visit-origin").hidden = !parent;
  $("visit-origin").disabled ||= !current()?.branch.parentAvailable;
  $("visit-origin").title = current()?.branch.parentAvailable
    ? "Visit the moment this world began"
    : "That moment has rolled out of recent history";
  $("view-overlay").setAttribute(
    "aria-pressed",
    String(comparisonView === "overlay"),
  );
  $("view-split").setAttribute(
    "aria-pressed",
    String(comparisonView === "split"),
  );
  $("compare-toggle").disabled ||= comparisonView === "split";
  $("compare-toggle").title =
    comparisonView === "split"
      ? "Difference highlighting is available in Overlay view"
      : "Highlight cells that differ from the reference";
  $("fork").disabled ||=
    !!draft || (state?.branches.length ?? 0) >= (state?.limits.branches ?? 12);
  $("running-indicator").dataset.running = String(running || replaying);
  $("running-indicator").querySelector("span").textContent = draft
    ? "Editing"
    : replaying
      ? "Replaying"
      : historical
        ? "History"
        : running
          ? "Running"
          : "Paused";
  $("state-badge").textContent = !state
    ? "Loading"
    : draft
      ? beforePreview
        ? "Before edits"
        : "Changing cells"
      : replaying
        ? "Replaying history"
        : historical
          ? "In the past"
          : running
            ? "Live"
            : "Paused";
  $("state-badge").dataset.mode = draft
    ? "editing"
    : historical
      ? "history"
      : running
        ? "live"
        : "paused";
  $("compare-toggle").setAttribute("aria-pressed", String(showDifferences));
  $("latest").hidden = !historical;
  $("fork-description").textContent =
    state?.branches.length >= state?.limits.branches
      ? "You have 12 worlds to explore. Start fresh for a new set."
      : "Keep this world. Give a new one a little nudge.";
  const count = draft?.changes.length ?? 0;
  $("draft-actions").hidden = !draft;
  $("draft-badge").textContent = draft ? `${count} changed` : "Draft edits";
  $("draft-context").textContent = draft
    ? `${label(draft.name)} · generation ${draft.checkpoint.generation}, revision ${draft.checkpoint.revision}. ${draftNeedsFork() ? "Applying creates a new branch." : "Apply these cells as one mutation."}`
    : "Click or drag on the grid. Changes stay in a draft until you apply them.";
  for (const button of document.querySelectorAll("[data-tool]")) {
    button.disabled = !!unavailable || pointer !== null;
    button.setAttribute("aria-pressed", String(button.dataset.tool === tool));
  }
  for (const id of [
    "undo-edit",
    "clear-edits",
    "preview-before",
    "apply-edits",
    "apply-run",
    "cancel-edits",
  ])
    $(id).disabled = !!unavailable || pointer !== null;
  $("undo-edit").disabled ||= !draft?.undoStack.length;
  for (const id of [
    "clear-edits",
    "preview-before",
    "apply-edits",
    "apply-run",
  ])
    $(id).disabled ||= !count;
  $("preview-before").setAttribute("aria-pressed", String(beforePreview));
  $("preview-before").textContent = beforePreview
    ? "Show draft"
    : "Show before";
  $("focus").setAttribute(
    "aria-disabled",
    String(!!unavailable || beforePreview),
  );
}

function renderTimeline(checkpoint) {
  const history = histories.get(selected) ?? [];
  $("history").max = Math.max(0, history.length - 1);
  if (!scrubbing)
    $("history").value = Math.max(
      0,
      history.findIndex((c) => c.id === checkpoint.id),
    );
  $("history").setAttribute(
    "aria-valuetext",
    `Generation ${checkpoint.generation}, revision ${checkpoint.revision}, ${checkpoint.kind}`,
  );
  $("checkpoint").textContent =
    `Generation ${checkpoint.generation} · revision ${checkpoint.revision}`;
  $("timeline-kind").textContent = draft
    ? "· unsaved draft"
    : replaying
      ? "· replaying recorded moments"
      : viewing
        ? "· viewing the past"
        : checkpoint.generation === 0
          ? "· beginning"
          : "· latest";
  $("history-start").textContent =
    history[0]?.ordinal > 0 ? `Gen ${history[0].generation}` : "Beginning";
  $("history-end").textContent = "Now";
  document.querySelector(".timeline").hidden =
    !viewing &&
    !$("details-panel").open &&
    !history.some((c) => c.generation > 0);
  $("history-note").textContent = viewing
    ? "Change a cell to try another future"
    : history[0]?.ordinal > 0
      ? "Recent history · older moments roll away"
      : "Slide back. Try something different.";
  const markers = document.createDocumentFragment();
  history.forEach((item, i) => {
    const children = state.branches.filter(
      (b) => b.parent?.name === selected && b.parent.checkpoint === item.id,
    );
    if (item.kind !== "mutation" && item.kind !== "fork" && !children.length)
      return;
    const marker = document.createElement("i");
    marker.className = `checkpoint-marker ${children.length ? "fork" : item.kind}`;
    if (children.length)
      marker.dataset.forks = children.map((b) => b.name).join(" ");
    marker.style.left = `${history.length < 2 ? 0 : (i / (history.length - 1)) * 100}%`;
    markers.append(marker);
  });
  $("history-markers").replaceChildren(markers);
}
function render() {
  renderControls();
  const shown = current();
  if (!shown) return;
  const { branch, board, checkpoint } = shown;
  const comparison = comparisonAt(checkpoint);
  const baseline = comparison.board;
  $("selected").textContent = label(selected);
  $("branch-name").textContent = label(selected);
  $("generation").textContent = String(checkpoint.generation).padStart(4, "0");
  $("parent-line").textContent = branch.parent
    ? `From ${label(branch.parent.name)} · generation ${branch.parent.generation}, revision ${branch.parent.revision}`
    : "The original · your unchanged reference";
  $("population").textContent = number(
    divergence(decode(board), new Uint8Array(decode(board).length)),
  );
  $("difference").textContent = baseline
    ? number(divergence(decode(board), decode(baseline)))
    : "—";
  $("difference-label").textContent = baseline
    ? "cells differ"
    : "comparison unavailable";
  $("fork-context").textContent =
    `${label(selected)} · gen ${checkpoint.generation}`;
  $("edit-hint-text").textContent = draft
    ? beforePreview
      ? "Before your changes."
      : "A little nudge. Press Play to see where it leads."
    : viewing
      ? "Tap a cell to grow a different future from here."
      : "Tap or drag on the grid to give this world a nudge.";
  paint($("focus"), board, baseline, {
    focus: true,
    overlay: comparisonView === "overlay" && showDifferences,
    changes: draft
      ? beforePreview
        ? []
        : draft.changes
      : checkpoint.kind === "mutation"
        ? checkpoint.changes
        : [],
  });
  renderComparison(comparison, checkpoint);
  renderTimeline(checkpoint);
  renderCards();
  if ($("details-panel").open)
    explorer.update({
      state,
      selected,
      reference: referenceName(),
      epoch: generation,
      locked: busy || !!draft || client?.closed,
    });
}

function renderComparison(comparison, checkpoint) {
  const split = comparisonView === "split";
  document.querySelector(".legend").hidden = split || !showDifferences;
  $("comparison-boards").classList.toggle("split", split);
  $("selected-board-label").hidden = !split;
  $("reference-board").hidden = !split;
  $("selected-board-label").textContent =
    `${label(selected)} · gen ${checkpoint.generation} · rev ${checkpoint.revision}${draft ? (beforePreview ? " · before draft" : " · draft") : ""}`;
  $("reference-board-label").textContent = comparison.checkpoint
    ? `${label(comparison.name)} · gen ${comparison.checkpoint.generation} · rev ${comparison.checkpoint.revision}`
    : comparison.name
      ? label(comparison.name)
      : "No parent";
  $("comparison-context").textContent = comparison.message;
  $("comparison-retry").hidden = !comparison.failed;
  $("reference").hidden = !comparison.board;
  $("reference-unavailable").hidden = !!comparison.board;
  $("reference-unavailable").textContent = comparison.message;
  if (comparison.board && split) {
    paint($("reference"), comparison.board, null, {
      focus: true,
      overlay: false,
      interactive: false,
    });
    $("reference").setAttribute("aria-label", comparison.message);
  }
}

async function fork() {
  const source = selected,
    checkpoint = current().checkpoint;
  await pause();
  accept(
    await request("fork", {
      source,
      checkpoint: checkpoint.id,
      name: `fork-${forkId++}`,
    }),
  );
  selected = state.branches.at(-1).name;
  viewing = null;
  await syncHistories();
  announce(
    `${label(selected)} forked from ${label(source)} at generation ${checkpoint.generation}.`,
  );
}
function draftNeedsFork() {
  return (
    draft &&
    (draft.name === "control" ||
      draft.checkpoint.id !==
        state.branches.find((b) => b.name === draft.name).head.id)
  );
}
function beginDraft() {
  if (draft) return true;
  const shown = current();
  if (!shown) return false;
  if (
    (viewing || selected === "control") &&
    state.branches.length >= state.limits.branches
  ) {
    error(
      "This experiment has 12 colonies. Select a colony at its latest moment to edit it, or start a new experiment.",
    );
    return false;
  }
  stopReplay();
  draft = new EditDraft({
    name: selected,
    checkpoint: shown.checkpoint,
    board: shown.board,
    width: state.width,
    height: state.height,
  });
  beforePreview = false;
  // Capture the displayed board before pausing. If a live tick was already in
  // flight, applying forks this exact moment rather than editing a newer board.
  action(async () => {
    await pause();
    await syncHistories();
  });
  return true;
}
async function applyDraft(runAfter) {
  if (!draft || !draft.changes.length) return;
  const cells = draft.changes;
  await pause();
  if (draftNeedsFork()) {
    accept(
      await request("fork", {
        source: draft.name,
        checkpoint: draft.checkpoint.id,
        name: `fork-${forkId++}`,
      }),
    );
    selected = state.branches.at(-1).name;
    // Retain the visible draft on this child if its mutation write fails.
    viewing = null;
    draft.name = selected;
    draft.checkpoint = state.branches.at(-1).head;
  }
  accept(await request("mutate", { name: selected, cells }));
  draft = null;
  beforePreview = false;
  viewing = null;
  await syncHistories();
  announce(
    `${cells.length} cells applied as one mutation in ${label(selected)}.`,
  );
  if (runAfter) {
    accept(await request("run", { hz: Number($("speed").value) }));
    running = true;
  }
}
function stopReplay() {
  replayEpoch++;
  clearTimeout(replayTimer);
  replaying = false;
}
async function advanceRecorded(epoch = replayEpoch) {
  if (!viewing) return false;
  const name = selected;
  const history = histories.get(name);
  const index = history.findIndex((c) => c.id === viewing.checkpoint.id);
  const next = history[index + 1];
  if (!next) return false;
  const result = await request("read", { name, checkpoint: next.id });
  if (epoch !== replayEpoch || selected !== name || draft) return false;
  viewing =
    next.id === state.branches.find((b) => b.name === name).head.id
      ? null
      : result;
  render();
  return !!viewing;
}
function startReplay() {
  stopReplay();
  replaying = true;
  const epoch = replayEpoch;
  const tick = async () => {
    try {
      const more = await advanceRecorded(epoch);
      if (epoch !== replayEpoch) return;
      if (!more) {
        stopReplay();
        render();
        announce(
          "Replay finished at the latest recorded moment. Run experiment to simulate further.",
        );
        return;
      }
      replayTimer = setTimeout(tick, 1000 / Number($("speed").value));
    } catch (failure) {
      if (epoch !== replayEpoch) return;
      stopReplay();
      error(failure.message);
      render();
    }
  };
  replayTimer = setTimeout(tick, 1000 / Number($("speed").value));
  announce("Replaying recorded history. No new generations are being created.");
}
function coordinates(event) {
  const rect = $("focus").getBoundingClientRect();
  return [
    Math.min(
      state.width - 1,
      Math.max(
        0,
        Math.floor(((event.clientX - rect.left) / rect.width) * state.width),
      ),
    ),
    Math.min(
      state.height - 1,
      Math.max(
        0,
        Math.floor(((event.clientY - rect.top) / rect.height) * state.height),
      ),
    ),
  ];
}

$("details-panel").ontoggle = () => {
  if (!$("details-panel").open) {
    comparisonView = "overlay";
    showDifferences = false;
  }
  render();
};
$("quick-undo").onclick = () => {
  draft.undo();
  beforePreview = false;
  render();
};
$("quick-cancel").onclick = () => $("cancel-edits").click();
$("help").onclick = () => {
  if (!state || busy) {
    $("help-dialog").showModal();
    return;
  }
  action(async () => {
    await pause();
    $("help-dialog").showModal();
  });
};
$("play").onclick = () =>
  action(async () => {
    if (draft) {
      await applyDraft(true);
      return;
    }
    if (viewing) {
      startReplay();
      return;
    }
    accept(await request("run", { hz: Number($("speed").value) }));
    running = true;
    announce("All colonies are running.");
  });
$("pause").onclick = () =>
  action(async () => {
    await pause();
    await syncHistories();
    announce("Simulation paused.");
  });
$("step").onclick = () =>
  action(async () => {
    if (draft) return;
    if (viewing) {
      stopReplay();
      await advanceRecorded();
      return;
    }
    running = false;
    accept(await request("step"));
    await syncHistories();
  });
$("fork").onclick = () => action(fork);
$("latest").onclick = () => {
  if (busy || draft) return;
  stopReplay();
  viewing = null;
  render();
  announce("Returned to the latest generation.");
};
function renderSpeed() {
  $("speed-value").replaceChildren(
    document.createTextNode(`${$("speed").value} `),
  );
  const unit = document.createElement("span");
  unit.textContent = viewing && !draft ? "moments / sec" : "gen / sec";
  $("speed").setAttribute(
    "aria-label",
    viewing && !draft
      ? "Recorded moments per second"
      : "Generations per second",
  );
  $("speed-value").append(unit);
}
$("speed").oninput = renderSpeed;
$("speed").onchange = () =>
  action(async () => {
    if (running) await request("run", { hz: Number($("speed").value) });
  });
$("compare-target").onchange = () => {
  compareTarget = $("compare-target").value;
  render();
};
$("view-overlay").onclick = () => {
  comparisonView = "overlay";
  render();
};
$("view-split").onclick = () => {
  comparisonView = "split";
  render();
};
$("visit-origin").onclick = () => {
  const parent = current()?.branch.parent;
  if (parent) navigateBranch(parent.name, parent.checkpoint, true);
};
$("comparison-retry").onclick = () => {
  failedReads.clear();
  render();
};
$("compare-toggle").onclick = () => {
  showDifferences = !showDifferences;
  render();
};
function scrub() {
  if (!state || draft || (busy && !scrubbing)) return;
  scrubTarget = histories.get(selected)?.[Number($("history").value)];
  if (!scrubTarget || scrubbing) return;
  scrubbing = true;
  action(async () => {
    try {
      await pause();
      while (scrubTarget) {
        const checkpoint = scrubTarget;
        scrubTarget = null;
        const result = await request("read", {
          name: selected,
          checkpoint: checkpoint.id,
        });
        if (!scrubTarget) {
          viewing =
            checkpoint.id ===
            state.branches.find((b) => b.name === selected).head.id
              ? null
              : result;
          render();
        }
      }
      announce(
        `Viewing generation ${current().checkpoint.generation}, revision ${current().checkpoint.revision}.`,
      );
    } finally {
      scrubbing = false;
      scrubTarget = null;
    }
  });
}
$("history").oninput = scrub;
$("history").onchange = scrub;
$("focus").onpointerdown = (event) => {
  if (!state || busy || beforePreview || pointer !== null || event.button !== 0)
    return;
  event.preventDefault();
  $("focus").focus({ preventScroll: true });
  if (!beginDraft()) return;
  pointer = event.pointerId;
  $("focus").setPointerCapture(pointer);
  hover = coordinates(event);
  draft.begin(hover, tool);
  render();
};
$("focus").onpointermove = (event) => {
  if (!state) return;
  hover = coordinates(event);
  if (pointer === event.pointerId) draft?.move(hover);
  $("cell-coordinate").textContent =
    `${String(hover[0]).padStart(2, "0")} / ${String(hover[1]).padStart(2, "0")}`;
  scheduleRender();
};
function endPointer(event, cancel = false) {
  if (pointer !== event.pointerId) return;
  if (!cancel) draft?.move(coordinates(event));
  draft?.end(cancel);
  pointer = null;
  if ($("focus").hasPointerCapture(event.pointerId))
    $("focus").releasePointerCapture(event.pointerId);
  render();
}
$("focus").onpointerup = (event) => endPointer(event);
$("focus").onpointercancel = (event) => endPointer(event, true);
$("focus").onlostpointercapture = (event) => endPointer(event, true);
$("focus").onpointerleave = () => {
  if (pointer !== null) return;
  hover = null;
  $("cell-coordinate").textContent = "64 × 48 cells";
  scheduleRender();
};
$("focus").onkeydown = (event) => {
  if (!state || busy || pointer !== null) return;
  if (
    (event.ctrlKey || event.metaKey) &&
    event.key.toLowerCase() === "z" &&
    draft
  ) {
    event.preventDefault();
    draft.undo();
    beforePreview = false;
    render();
    return;
  }
  if (["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) {
    event.preventDefault();
    hover ??= [Math.floor(state.width / 2), Math.floor(state.height / 2)];
    if (event.key === "ArrowLeft") hover[0] = Math.max(0, hover[0] - 1);
    if (event.key === "ArrowRight")
      hover[0] = Math.min(state.width - 1, hover[0] + 1);
    if (event.key === "ArrowUp") hover[1] = Math.max(0, hover[1] - 1);
    if (event.key === "ArrowDown")
      hover[1] = Math.min(state.height - 1, hover[1] + 1);
    $("cell-coordinate").textContent = `${hover[0]} / ${hover[1]}`;
    $("focus").setAttribute(
      "aria-label",
      `Colony grid, cell ${hover[0]}, ${hover[1]}, ${live(decode(current().board), state.width, ...hover) ? "alive" : "empty"}. Enter drafts a change with the ${tool} tool. Arrow keys move selection.`,
    );
    scheduleRender();
  } else if (event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    hover ??= [Math.floor(state.width / 2), Math.floor(state.height / 2)];
    if (!beforePreview && beginDraft()) {
      draft.begin([...hover], tool);
      draft.end();
      render();
    }
  }
};
for (const button of document.querySelectorAll("[data-tool]"))
  button.onclick = () => {
    tool = button.dataset.tool;
    $("tool-hint").textContent =
      tool === "auto"
        ? "Auto toggles a cell; dragging repeats that change."
        : tool === "paint"
          ? "Click or drag to add living cells."
          : "Click or drag to erase living cells.";
    render();
  };
$("undo-edit").onclick = () => {
  draft.undo();
  beforePreview = false;
  render();
  announce("Last stroke undone.");
};
$("clear-edits").onclick = () => {
  draft.clear();
  beforePreview = false;
  render();
  announce("Draft cleared. No history was changed.");
};
$("cancel-edits").onclick = () => {
  draft = null;
  beforePreview = false;
  render();
  $("focus").focus({ preventScroll: true });
  announce("Draft discarded. No history was changed.");
};
$("preview-before").onclick = () => {
  beforePreview = !beforePreview;
  render();
};
async function applyAndFocus(runAfter) {
  await action(() => applyDraft(runAfter));
  if (!draft) $(running ? "pause" : "focus").focus({ preventScroll: true });
}
$("apply-edits").onclick = () => applyAndFocus(false);
$("apply-run").onclick = () => applyAndFocus(true);
$("reset").onclick = () =>
  action(async () => {
    await pause();
    $("reset-dialog").showModal();
  });
$("reset-dialog").addEventListener("close", () => {
  if ($("reset-dialog").returnValue === "reset") action(initialize);
});
$("dismiss-error").onclick = () => error("");
$("retry").onclick = () =>
  action(async () => {
    client?.terminate();
    connect();
    await initialize();
  });
window.addEventListener("pageshow", (event) => {
  if (event.persisted && client?.closed) action(initialize);
});
document.addEventListener("visibilitychange", () => {
  if (document.hidden && state && !busy && !client?.closed) action(pause);
});
window.addEventListener("pagehide", () => {
  stopReplay();
  client?.terminate();
});
new ResizeObserver(() => {
  const scale = Math.max(
    1,
    Math.ceil(
      ($("focus").getBoundingClientRect().width *
        Math.min(devicePixelRatio, 2)) /
        64,
    ),
  );
  if ($("focus").width !== scale * 64) {
    $("focus").width = scale * 64;
    $("focus").height = scale * 48;
    // Resizing clears a canvas. Paint synchronously so a resized viewport
    // never presents an empty frame while waiting for requestAnimationFrame.
    const shown = current();
    if (shown)
      paint($("focus"), shown.board, comparisonAt(shown.checkpoint).board, {
        focus: true,
        overlay: comparisonView === "overlay" && showDifferences,
        changes: draft
          ? beforePreview
            ? []
            : draft.changes
          : shown.checkpoint.kind === "mutation"
            ? shown.checkpoint.changes
            : [],
      });
  }
}).observe($("focus"));
new ResizeObserver(() => {
  const width = $("reference").getBoundingClientRect().width;
  if (!width || !state) return;
  const scale = Math.max(
    1,
    Math.ceil((width * Math.min(devicePixelRatio, 2)) / state.width),
  );
  if ($("reference").width === scale * state.width) return;
  $("reference").width = scale * state.width;
  $("reference").height = scale * state.height;
  const comparison = comparisonAt(current().checkpoint);
  if (comparison.board)
    paint($("reference"), comparison.board, null, {
      focus: true,
      overlay: false,
      interactive: false,
    });
}).observe($("reference"));
connect();
await action(initialize);
