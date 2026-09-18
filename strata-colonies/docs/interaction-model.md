# Slice 3: interaction model

Status: implemented and browser-verified on 2026-09-13.

The browser demo at `http://localhost:7421` now supports deliberate, grouped
editing and playback of recorded history.

## Try it

1. Click or drag on a colony. Playback pauses and the grid becomes a draft.
2. Use **Auto** to toggle the first cell and repeat its new state along the
   stroke, **Paint** to add cells, or **Erase** to remove them.
3. Review the highlighted changes. **Show before** shows the unedited board;
   **Show draft** returns to editing. **Undo stroke** (or Ctrl/Cmd+Z while the
   grid is focused) removes the last gesture. Arrow keys and Enter/Space also edit.
4. Choose **Run with changes**, or **Apply & stay paused**. All net cell changes
   become one mutation checkpoint at the current generation, with a new revision.
5. Scrub backward. **Replay history** and the Step button now move through saved
   checkpoints, including multiple revisions at the same generation. Replay
   stops at the latest moment; simulating further requires Run experiment.
6. Edit an earlier moment and apply the draft to create a child branch. The
   parent's recorded future remains intact.

## Behavior and implementation

- `web/edit-draft.mjs` keeps the base board, pending board, and undo stack separate
  from the database. Fast pointer movements interpolate crossed cells. Revisiting
  a cell during painting does not toggle it again. A cancelled pointer gesture
  restores the board from before that gesture.
- Clear edits resets the draft; Discard draft exits it. Neither writes to Strata.
  Cancelling a Control or historical draft does not create an empty branch.
- `web/app.mjs` sends the unique net changed cells through the existing real
  worker `mutate` operation. The engine records one mutation checkpoint using
  its existing KV, JSON, and event writes; those writes are separate commits.
- The draft captures the displayed checkpoint before awaiting Pause. If an
  in-flight tick has advanced the branch, applying forks the captured checkpoint
  instead of overwriting the newer head. Editing Control also creates a child.
- Branch selection, history navigation, simulation, and explicit forking are
  disabled while a draft is open. Apply or discard it to continue. Reset retains
  its confirmation dialog and pauses playback before opening.
- History replay uses real worker reads and never creates checkpoints. Speed is
  labeled in recorded moments per second during history inspection. Scrubbing
  coalesces rapid selections; cancelled replay reads cannot replace a newer view.
- The editing controls use the existing right sidebar on desktop and follow the
  grid on mobile. Touch gestures paint without scrolling the page under the grid.

## Validation

```bash
npm run test:browser
npm run test:ui
npm run test:interactions
npm run dev:browser
```

The feasibility suite still passes native Life parity, real WASM history/forks,
separate sessions, and 1,000-generation runs with six and twelve colonies.

The updated UI suite verifies the visual foundation with draft/apply semantics,
keyboard/touch editing, historical forks, reset, responsive layout, and recovery
from engine loading failure.

The new interaction suite checks continuous/idempotent strokes, undo and gesture
cancellation, draft isolation, grouped checkpoint metadata, retained pre-edit
boards, same-generation replay, replay pause/completion, rapid scrubbing, stale
read cancellation, historical draft forks, Apply and Run, a forced in-flight tick,
and touch dragging. Results and screenshots are generated under `artifacts/`.

These are automated Chromium checks, not yet cross-browser or physical-device
certification. Undo applies to pending strokes; applied mutations remain in
history. Session lifetime and existing application caps remain unchanged.

## Follow-up

Slice 4 has since added clickable lineage, exact source navigation, parent
comparisons, paired boards, and a saved-differences chart. See
[Branching experience](branching-experience.md). Website integration and
publishing remain slice 5. The native Rust interface remains unchanged.
