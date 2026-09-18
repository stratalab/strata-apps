# Slice 2: visual foundation

Status: implemented and browser-verified on 2026-09-13.

## What changed

The browser demo at `http://localhost:7421` now uses a StrataDB-branded interface.
The native Rust app on port 7420 is still the original local application.

- Self-hosted General Sans and Commit Mono, copied with their licenses from the
  website. Shared black/charcoal surfaces, neutral text, ember accents, borders,
  and radii live in `web/tokens.css`.
- Large selected colony, readable cell grid, mutation marker, hover coordinates,
  and crisp canvas rendering across display sizes and pixel densities.
- Desktop controls remain in a sticky right sidebar. Mobile playback moves above
  the canvas and stays visible during scrolling.
- Six starter previews, stable DOM buttons, clear selection, and room for new
  branches through the existing twelve-branch cap.
- Playback, pause, step, speed, history inspection, and historical forks remain
  connected to the real worker engine.
- Living-cell and difference counts replace persistence counters and hashes.
  The comparison overlay distinguishes shared cells, added cells, and outlined
  missing cells. Comparing a younger branch retrieves Control at the matching
  generation instead of comparing different points in time.
- Historical inspection visibly changes the state badge and disables Run/Step.
  Return to latest and Fork from here are explicit actions. Forking captures the
  displayed checkpoint before pausing, preserving the selected moment.
- Clicking the control or a historical checkpoint creates a child before editing.
  The canvas also supports arrow-key selection and Enter/Space to toggle a cell.
- New experiment has a reset confirmation. Loading, errors, retry, disabled
  actions, and session lifetime have visible states.
- The engine-findings panel and developer-only copy are absent. A collapsed
  explanation describes the real database behavior without exposing internals.

## Files

- `web/index.html`: semantic page shell, controls, legend, and reset dialog.
- `web/app.css`, `web/tokens.css`, `web/fonts/`: standalone brand implementation.
- `web/app.mjs`: presentation state, canvas rendering, and worker interactions.
- `scripts/verify-ui.mjs`: desktop/mobile interaction and layout checks.

The original `probe.css` and `probe.mjs` have been removed. Worker storage and
simulation behavior are unchanged. Website integration and deployment remain
slice 5; the standalone token subset can be replaced by the website's own tokens
when the page is integrated.

## Validation

```bash
npm run test:browser
npm run test:ui
npm run dev:browser
```

The existing browser feasibility suite passes, including native parity, history,
fork isolation, and six-/twelve-branch performance at 1,000 generations.

The UI suite verifies:

1. Shared fonts load, controls sit to the right, and all six previews render.
2. Run/pause, step, speed, pointer/keyboard mutations, and difference toggle.
3. Historical inspection preserves the head and historical forks preserve parents.
4. Comparisons align generations when a fork trails its parent and Control.
5. Control editing creates a child; the branch cap disables further forks.
6. Reset cancellation preserves work and confirmation resets to six colonies.
7. No horizontal overflow at 1440px, 1024px, and 390px. Canvas redraws after resize.
8. Mobile transport precedes the grid, remains sticky, and touch editing works.
9. Reduced-motion preference and visible load-failure recovery.

Screenshots and the machine-readable result are generated under `artifacts/`:
`ui-desktop.png`, `ui-compact.png`, `ui-mobile.png`, and `ui-verification.json`.
These are Chromium checks, not yet cross-browser or real-device certification.

## Follow-up

Slice 3 has since added grouped drafts, drag painting/erasing, stroke undo,
before/after inspection, and recorded-history replay. It supersedes this slice's
immediate-click editing and disabled historical playback; see
[Interaction model](interaction-model.md) for current behavior and validation.

Full lineage, parent comparison views, deeper onboarding, and website integration
remain planned. Browser lifetime and provisional history limits remain as
described in the feasibility report.
