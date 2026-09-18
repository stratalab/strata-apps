# Slice 1: browser feasibility

Date: 2026-09-13

Outcome: **proceed with the browser architecture**. The real Strata 1.2.2 WASM
engine supports the required history and fork operations in a module worker,
with ample local headroom for six starter colonies and twelve total branches.

This report records the original engineering slice. Its developer interface has
since been replaced by the [slice 2 visual foundation](visual-foundation.md).
The native app remains available separately. No website source or deployment was changed.

## Run it

Requirements: Node.js 22+, npm, Rust (`rustc` for native parity), and Playwright
Chromium. The browser runtime itself has no npm dependency and requires only a
static server and the Strata WASM bundle.

```bash
npm ci
npx playwright install chromium

# Stage the website's released bundle, or another verified 1.2.2 release bundle.
npm run wasm:stage -- /path/to/stratadb.org/dist/playground/pkg

npm run test:browser
npm run dev:browser
```

Open <http://127.0.0.1:7421>. The harness supports running/pausing, stepping all
colonies, selecting a branch, inspecting checkpoints, forking a selected
checkpoint, and clicking the large grid to mutate it. Editing the control or
history creates a child. New session clears the volatile experiment.

`web/pkg/` and `artifacts/` are generated and ignored. The staging script records
SHA-256 hashes and the worker rejects an engine older than 1.2.2. The
verifier checks bundle hashes before running. Automated checks never fetch a
mutable remote bundle. For future website integration, its existing release
pipeline supplies the pinned bundle; it should not add a second WASM download.

## Delivered code

| File                                                  | Responsibility                                                                     |
| ----------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `web/life.mjs`                                        | Packed boards, deterministic genesis/mutations, toroidal Life                      |
| `web/engine.mjs`                                      | Real Strata commands, branch heads, checkpoint coordinates, historical reads/forks |
| `web/worker.mjs`                                      | WASM initialization, serialized requests, playback, timing, disposal               |
| `web/client.mjs`                                      | Promise-based worker interface, events, rejection on worker failure                |
| `web/index.html`, `web/app.mjs`, `web/app.css`        | Browser interface (replaced the original probe UI in slice 2)                      |
| `scripts/verify-browser.mjs`                          | Real Chromium correctness, retention, and performance checks                       |
| `scripts/native-fixture.rs`                           | Reference boards generated from the existing Rust source                           |
| `scripts/stage-wasm.mjs`, `scripts/serve-browser.mjs` | Local artifact staging and static development server                               |

## Data and history contract

Each checkpoint writes a packed board to KV, metadata to JSON, and an event to
the branch's checkpoint log. A checkpoint is published to the UI only after all
three succeed. These remain separate engine commits; this is not a claim of
cross-capability atomicity. A persistence error disables further writes until
the session is reset instead of presenting partially persisted state as success.

Each branch has its own board, checkpoint list, and head generation. Checkpoints
record generation, revision, kind, changed cells, board commit version, final
event commit version, logical timestamp, and event sequence. Metadata and the
event store parent lineage. Graph presentation/storage follows in the branching
experience slice.

- Historical KV and JSON reads use the checkpoint's **logical timestamp**.
- Historical forks use the checkpoint's **commit version**.
- Simulation generation and revision are application coordinates, not database
  versions or timestamps.
- Reads validate that the returned board version and JSON generation/revision
  match the requested checkpoint.
- A fork calls `branch_fork_at_version`; it checks the child's inherited board
  before writing any child data. It does not simulate a fork by copying a JS board.
- A child's local timeline begins at its fork generation. Parent lineage preserves
  where that history came from. Ancestor timeline navigation is later UI work.
- Comparisons select the latest revision of each branch at the same generation.
  Missing generations reject; they do not silently compare different moments.

There is no Axum server, WebSocket connection, filesystem, or shared session in
this browser path. All database and simulation work executes inside the worker.
The page receives packed board snapshots for rendering.

## Validation

`npm run test:browser` passed with the following checks:

- 378 browser/native board matches across three grids and 21 generations,
  including a grid whose packed cell count is not byte-aligned.
- The worker's persisted 64×48 evolution matches the Rust reference.
- Old boards remain readable after later generations and same-generation edits.
- Forks from retained history inherit the exact board; nested forks preserve lineage.
- Child mutations and subsequent parent steps remain isolated.
- Generation-aligned comparisons return the expected one-cell difference.
- Unavailable checkpoints, unavailable comparison generations, invalid mutations,
  control edits, and duplicate branch names reject without falling back to head.
- Independent workers have separate sessions; disposed sessions reject reads.
- Scheduled playback emits updates; pause stops ticks; initialization and following
  requests execute in order.
- At 1,000 generations, initial snapshots on every branch and a midpoint snapshot
  remain readable in the 6- and 12-branch runs.
- A new fork from generation zero still works after 1,000 generations.
- Branch and generation limits reject further growth.

The harness was also exercised through actual browser controls: step, scrub,
fork, click to mutate, and reset, with no page errors. The existing nine native
Rust tests continue to pass.

## Measurements

Reference machine: AMD Ryzen 7 7800X3D, Linux x64. Headless Chromium
151.0.7922.34. Strata 1.2.2, grid 64×48, seed 42, 1,000 unthrottled generations.
Measurements below are from the final validation run on 2026-09-13.

| Measurement                       | 6 colonies |                        12 colonies |
| --------------------------------- | ---------: | ---------------------------------: |
| Median worker time per world step |     0.9 ms |                             1.6 ms |
| p95 worker time per world step    |     1.2 ms |                             2.0 ms |
| Largest world step                |     2.2 ms |                             3.9 ms |
| Wall time for 1,000 generations   |     1.28 s |                             2.08 s |
| WASM linear memory after run      |   22.4 MiB |                           42.9 MiB |
| Main-thread long tasks during run |          0 |                                  0 |
| p95 animation frame gap           |    16.7 ms |                            16.7 ms |
| Oldest-checkpoint reads, maximum  |     0.2 ms |                             0.1 ms |
| Historical fork after run         |     1.4 ms | Not attempted at the 12-branch cap |

At the proposed 8 generations/second, the worker has a 125 ms budget for each
world step. Both branch counts pass that gate. Historical reads and the measured
fork pass the provisional 100 ms action budget.

The benchmark includes Life evolution, all three persistence calls per branch,
snapshot construction, and worker message serialization. The main thread receives
every update and paints representative colony preview canvases while an animation
frame heartbeat and PerformanceObserver measure responsiveness. No simulation or
database call runs on the page thread.

The report at `artifacts/browser-feasibility.json` records unrounded values and
the tested bundle hashes. WASM linear memory measures allocated pages, not total
browser memory or live allocations; JS heap and rendering memory are additional.
Sub-millisecond readings are subject to browser timer resolution. This desktop
test is not a mobile performance claim, nor a benchmark of the completed UI.
The local bundle was already available, so these timings exclude network transfer.

## Artifact identity

- JavaScript: 17,168 bytes; SHA-256
  `c3cf29319ea2109717d9bebe8fec489599353267c1bb8028e6075ce5e58b602c`
- WASM: 9,221,536 bytes; SHA-256
  `505cb1934212d2d03cc3283b92e0533191846ce649422a909c8f7e520c9e7486`

## Constraints and next slice

- Session state is intentionally volatile. Refreshing loses it; no persistence
  claim is made. Export/import or shareable recipes remain future work.
- The original slice stopped at generation 1,000 or 1,200 checkpoints per branch.
  [Continuous playback](continuous-playback.md) supersedes these stop conditions:
  the app now retains a sliding window of 1,200 checkpoints per colony and frees
  expired database segments. The 12-colony cap remains.
- The checkpoint index is in worker memory. This slice does not implement durable
  recovery, restore from event logs, or cross-capability transaction support.
- The benchmark's extra six branches fork the same starter variant. This measures
  branching and storage headroom, not the quality of the final preset selection.
- Initial loading of the 8.8 MiB uncompressed WASM artifact needs a clear loading
  state and a cold-network check in the website integration slice.
- Next: implement the branded canvas/sidebar/previews using the site's existing
  fonts and tokens. Keep this worker contract independent of the presentation.
