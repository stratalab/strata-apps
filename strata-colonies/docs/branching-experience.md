# Slice 4: branching experience

Status: implemented and browser-verified on 2026-09-13.

Preview: `http://localhost:7421`. Refresh the page to load this slice.

## Try it

1. Run a starter colony, pause, and scrub to an earlier revision.
2. Fork that moment, draft a mutation, and apply it.
3. Set **Compare with** to **Parent**. The overlay and difference count compare
   the displayed board with the parent's latest saved revision at that same
   generation. Switch to **Side by side** to inspect both labeled boards.
4. Open **Branch lineage** below the previews. Select any colony to visit its
   latest board; use its fork link to visit the exact source revision in its
   parent. **Visit fork point** in the sidebar does the same for the selected child.
5. Fork the child to explore a nested branch. Its ancestry remains visible.
6. Read **Saved differences** to see divergence across shared generations.
   **View recorded values** exposes the chart's generation, both revisions,
   and difference count as a table.

## Comparison semantics

The selected grid retains its exact checkpoint, including its revision. A draft
can also be compared before it is applied. The reference uses the **latest saved
revision at the same simulation generation**. That may be a later revision than
one used to create the fork. The fork-point links always use the exact stored
source checkpoint; they never substitute the parent's latest revision.

The chart uses the latest saved revision on both branches for every shared
generation. It covers saved history, including recorded futures beyond the
currently inspected moment. Drafts are not included. The grid and chart labels
make these distinctions explicit.

If the reference has no matching generation, the UI displays its recorded range
and leaves the comparison empty. Choosing Parent for Control explains that it
has no parent. Side-by-side shows a read-only reference, stacking below the
editable grid on narrow screens. Editing and comparison controls precede the
previews, lineage, and chart on mobile. Differences remain available in Overlay mode.

## Implementation

- `web/branch-explorer.mjs` renders stable, keyboard-accessible lineage buttons,
  exact fork-origin links, the SVG chart, its values table, and chart retry states.
- `web/app.mjs` supports Control/Parent references, historical board loading,
  side-by-side rendering, exact origin navigation, and fork markers on source
  timelines. Pending drafts disable lineage navigation. Switching targets never
  lets a delayed read substitute another reference's board. Failed reads remain
  visible until explicitly retried instead of continuously requesting them.
- `web/engine.mjs` adds a read-only `comparisonHistory` operation exposed through
  the worker as `comparison-history`. It reads retained KV/JSON snapshots, then
  derives differences. Requests can pin both head checkpoints so in-flight
  simulation cannot extend the requested chart. A bounded cache holds derived
  counts keyed by immutable checkpoint pairs; revisions invalidate the relevant
  points naturally. It does not change retention or write additional history.
- Lineage comes from parent checkpoint metadata recorded during actual Strata
  forks. This slice does not claim or introduce graph-database storage.
- `web/index.html` and `web/app.css` extend the existing branded layout.

## Validation

```bash
npm run test:browser
npm run test:ui
npm run test:interactions
npm run test:branching
npm run dev:browser
```

All four suites pass. The new branching suite covers:

- Parent comparison across different branch heads and same-generation revisions.
- Correctly labeled, read-only reference boards and responsive paired views.
- Historical and nested forks, preserved parents, and exact source navigation.
- Recorded chart values, revision changes, and requests pinned to older heads.
- Missing generations, absent parents, and target switching.
- Draft protection, delayed-read cancellation, explicit retry after reference or chart failure.
- Twelve branches on a narrow layout, source markers, and a clean reset.
- A read-only chart spanning 1,001 recorded generations through the real engine.

The initial full-history chart check took approximately 79 ms on this machine.
This is a local observation, not a browser/device performance guarantee. Prior
Life parity, retention, isolated-session, editing, and replay checks still pass.

The suite writes `artifacts/branching-verification.json`, `branching-desktop.png`,
and `branching-mobile.png`. Verification uses Chromium; broader browser/device
coverage remains part of the final polish slice.

## Follow-up

Slice 5 has since added first-visit guidance, accessibility checks, and the
StrataDB website route/listing. See [Website integration](website-integration.md)
for validation and release instructions. Session lifetime and application caps
remain unchanged. Publishing is a separate release action.
