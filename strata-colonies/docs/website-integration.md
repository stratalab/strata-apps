# Slice 5: polish and website integration

Status: implemented and verified locally on 2026-09-13. Publishing is a separate release action.

The later [Play first](play-first.md) pass supersedes the walkthrough and visible
control panels described below. Website integration and release commands remain
current.

## Visitor experience

- A collapsible quick-start guide takes the visitor through running colonies,
  inspecting an earlier moment, creating a child, and comparing with its parent.
  It uses normal app operations and does not create a canned outcome.
- How to play opens a keyboard-accessible dialog explaining the journey, Life
  rules, controls, session lifetime, and application caps. Opening it pauses the
  simulation. Escape closes it and returns focus to the opener.
- The grid announces the selected cell's coordinates and whether it is alive.
  Drafts retain their existing pointer, touch, keyboard, undo, and focus behavior.
- Mobile editing/comparison sections can be collapsed from the sticky transport.
  Starting a draft reopens them. Desktop controls remain on the right.
- Simulation limits disable further stepping/running at the head while recorded
  history remains accessible. Backgrounding an idle UI pauses playback, and
  returning through the browser's page cache restarts a terminated session.
- A no-JavaScript message and actionable browser-load error replace implementation
  instructions. Decorative coordinate labels have sufficient contrast.
- Only the collapsed-guide preference is stored locally. The database still
  lasts for the tab session and is cleared by reload; no server session is shared.

## Website integration

The website repository is `stratalab/stratadb.org`.
The implemented route is `/demos/colonies/`, with a new entry on
`/resources/demos/`. It is a full-page Astro route, without an iframe or nested
site navigation. Its CSS uses the website's current tokens and shared fonts.
Canonical, social-preview, and icon metadata are included.

`scripts/sync-website.mjs` copies the owned app sources and records their hashes.
Website `verify-colonies.mjs` checks that they stay in sync. The site no longer
pins its own bundle: `fetch-wasm.mjs` stages whatever strata-core release the
site is building against, and `stage-colonies.mjs` copies it in. The worker
requires Strata 1.2.2 or newer and accepts anything above that floor, so an
upstream release upgrades the demo rather than breaking it. What catches a real
incompatibility is the website's `visual-smoke.mjs`, which loads the demo, waits
for six colonies and requires a generation to advance. Nothing depends on this
machine's app path at deploy time. See the website's `docs/colonies.md` for
maintenance.

## Reproduce

From this repository:

```bash
npm ci
npm run setup:browsers
npm run test:browser
npm run test:ui
npm run test:interactions
npm run test:branching
npm run website:sync -- /path/to/stratadb.org
```

From the website, using its supported Node version:

```bash
npm run check
npm run preview
```

Then run the final suite here against the built preview's actual URL:

```bash
COLONIES_SITE_URL=http://127.0.0.1:4321/demos/colonies/ npm run test:release
```

Playwright requires the normal browser host libraries. On this machine, WebKit's
missing `libavif16`, `libgav1-1`, and `libyuv0` were extracted into the ignored
`artifacts/browser-libs/` directory. The test command used
`LD_LIBRARY_PATH=$PWD/artifacts/browser-libs/usr/lib/x86_64-linux-gnu`.
The three libraries were also placed in Playwright’s user-local WebKit cache
because its launcher replaces the library path. No privileged system package
installation was needed.

## Verification and release boundary

The game suites cover the native Life reference, real-engine history and forks,
draft editing, replay, lineage, aligned comparisons, failures, and 1,000-generation
sessions. The release suite passes the guided journey in Chromium, Firefox, and
WebKit, preserves the parent's recorded future, and checks keyboard focus,
mobile layout, session reset, and guide preferences.

Axe checks WCAG A/AA rules in initial, help, draft, paired, and mobile states.
Results are written to `artifacts/release-verification.json` and individual
`artifacts/accessibility-*.json` files. These are automated checks, not complete
assistive-technology or physical-device certification.

The website's `npm run check` includes the new route in its visual smoke suite.
Its build/source/link/redirect/transcript checks and runtime audit pass. Existing
warnings about an empty architecture collection and historical redirect pages
are unrelated to Colonies. The pre-existing Hub catalog edit was preserved.

The route is integrated and reviewable locally. No commit, push, or public
website deployment was performed. The existing Pages workflow publishes pushes
to the website's `main` branch. Export/import, durable browser storage, and
shareable experiment recipes remain future enhancements outside this revamp.
