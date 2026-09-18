# Simplification: play first

The visitor experience now centers on Play, changing cells, and trying another
future. This supersedes the prominent tool panels and guided walkthrough from
the delivery slices: those exposed too much of the implementation at once.

## Main experience

- One Play/Pause button. Changing a cell pauses playback; Play applies the whole
  group of changes and resumes. No separate Apply workflow is required.
- A small Undo and Cancel row appears only while there are pending changes.
- Rewind appears once time has passed. Editing a past moment and pressing Play
  creates the child automatically, preserving the original recorded future.
- Try another future makes a copy of the displayed moment. Other futures are
  selectable as small colony previews without stopping live playback. A paused
  simulation stays paused when switching. Control is labeled Original; new forks
  are Your world 01, Your world 02, and so on.
- No walkthrough, tool modes, speed slider, numeric dashboard, graph, chart, or
  comparison settings on the initial screen. The copy is a short invitation.
- Look closer reveals precision playback/editing, comparisons, lineage, saved
  differences, and the database explanation. The chart loads only when opened.
  Closing it restores the uncluttered single-grid view.
- A short optional How it works dialog provides the basic idea and keyboard keys.

Playback continues without a generation cap. Rewind keeps the latest 1,200
checkpoints per colony, including edits at the same generation. Older moments
roll away; existing children keep running after their source moment expires.
Exact historical forks and grouped mutations still use real Strata operations.
See [Continuous playback and recent history](continuous-playback.md) for retention
and storage details.

## Implementation and validation

`web/playful.css` defines the simplified presentation. `web/index.html` places the
optional panels inside a closed details element; they are not just dimmed on the
main page. `web/app.mjs` applies pending changes through the primary Play button
and avoids loading comparison charts until requested.

The existing UI, editing, and branching suites exercise precision controls by
explicitly opening Look closer. The release suite separately verifies the main
journey with the disclosure closed: Play, rewind, edit, Undo, Play with changes,
and a preserved parent future. It caps the initial visible button count at ten,
including the six colony selectors, and tests optional detail views separately.

Chromium, Firefox, and WebKit pass the simplified journey and automated WCAG A/AA
checks for initial, help, editing, paired, and mobile states. Website source
syncing, artifact pinning, route, and listing remain in place. See
[Website integration](website-integration.md) for build and publishing commands.

Local website preview: `http://localhost:4323/demos/colonies/`.
No public deployment is performed by this change.
