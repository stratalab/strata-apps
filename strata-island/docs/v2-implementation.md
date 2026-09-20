# Curated graph expansion

Implemented 2026-09-19 against published Strata v1.2.3. The [design plan](graph-expansion-plan.md) remains the longer-term workload specification; this document records the shipped scope and concrete choices.

## Run

```sh
cargo run --release --bin island -- --dataset v2
# http://127.0.0.1:7450 · ./island-db-v2
```

V1 remains the CLI default and uses `./island-db`. V2 refuses a V1, mismatched, or unowned nonempty database directory. Local durable opens use `DurabilityMode::Always`; every acknowledged write is synced. The default buffered mode does not guarantee process-kill survival and failed the first termination test. This was an app configuration issue, corrected without filing an engine defect. V1's road data and six compatibility IDs remain unchanged.

## Data and console

- Exactly 1,852 pinned destinations: 1,674 landmarks, 14 parks, and 164 transit/portal destinations (including 151 MTA subway stations). The original hospital and tunnel destinations are retained for compatibility.
- Source: pinned OSM objects with complete geometry, source revisions/timestamps, a pinned Manhattan borough boundary (relation 8398124), source hashes, stable identities, and attribution. Raw snapshots and the query are under `fixtures/places-source/`; explicit selections and reasons are in `fixtures/places-curation.json`.
- `python3 tools/expand_places.py` selects all 1,651 eligible additions from pinned candidates; `python3 tools/extract_places.py` regenerates normalized data and the quality report entirely offline. It validates boundary inclusion and preserves the frozen road extract. Display locations use actual OSM nodes or geometry vertices, not bounding-box centers. The Met explicitly selects its Fifth Avenue side to avoid the isolated western service-road component.
- 1,814 destinations have road anchors; 38 (including Inwood Hill Park) remain unconnected. The 36 OSM exceptions exceed the 200 m attachment radius; Roosevelt Island and Marble Hill subway stations are outside the island street extract. Existing aliases retain their original anchors, including three approaches beyond 150 m. Every non-legacy connection is labeled approximate; no entrance/legal-driving assertion is made.
- Searchable endpoint comboboxes, a paginated place browser, categories, collision-managed map labels, map-click details, route actions, real-location/anchor connectors, and a bounded relationship diagram.
- Nearest/within-distance discovery ranks results by directed road distance. Impact distinguishes unchanged, farther, closer, newly unreachable, newly reachable, and already unreachable places.
- Select named street sections on the map or from search, preview/undo/remove selections, and create a named custom closure (both existing directions). The original 42nd Street corridor and current route remain presets (maximum 200 directed edges). Reopen/close again; select a completed historical version for destination discovery. Map routes show the current scenario, as labeled in the UI.

Catalog revision 3 includes all eligible named cultural/historic landmarks from a deterministic, geographically distributed OSM selection. The first 550 normalized records and all six aliases are preserved exactly. See [selection and upgrade notes](catalog-all.md). Map markers are spaced by zoom, capped at 180 visible points and 60 labels; Revision 4 adds all 151 Manhattan MTA stations while preserving all 1,701 OSM records exactly; all 1,852 locations remain searchable. The endpoint catalog follows all 19 bounded API pages at one version. The subway layer draws its own station pins and schematic service connections. See [subway implementation](subway.md).

See [custom closures and exploration missions](interactive-missions.md) for the map editor and browser-local Landmark Passport gameplay.

## Engine work

`manhattan` contains only streets. `places` contains typed landmark/park/transit nodes, shared anchor references, categories, typed relationships, and branch-local JSON bindings. Because ontology links declare one concrete source type, relationship names are category-specific: `park_near_road`, `park_in_category`, etc. The anchor-to-road join uses a stable `road_node_id`; it is not an engine cross-graph edge.

Import defines and freezes the ontology, writes documents, bulk-loads nodes/edges, validates typed nodes, bindings and relationship membership, then writes a dataset-ready record. An interrupted import replays deterministic upserts without deleting graphs. Catalog v4 readiness is published after the separate subway graph is imported. Place documents and membership are immutable in this iteration; place-edit UI is a later feature.

Strata supplies typed-node reads, node/neighbor/binding reads, adjacency snapshots, SSSP, BFS, induced subgraphs, WCC, atomic graph batches, branch forks/comparison/deletion, historical reads, and event verification. Search text and route path reconstruction remain application-side. Pan/zoom never invokes graph APIs. Snapshot construction is measured separately from algorithms; historical discovery labels a newly constructed snapshot as uncached.

Scenario creation serializes ID allocation, cap checks, engine mutation, and publication. Parent-side intent closes the fork-before-child-document crash window. Each operation records pending state, applies an atomic graph batch, appends one uniquely keyed completion event, writes an immutable ready document, and publishes the completed version. Replays are idempotent. Reopening restores exact original street weights/properties. Branch history is bounded to 100 operations. Historical snapshots are request-scoped, not retained indefinitely.

Current road snapshots use shared `Arc` values. Graph analysis admits two active jobs and eight queued jobs; excess work gets HTTP 429. Blocking jobs hold their permits until they actually finish, including when the client disconnects. `/api/graph-metrics` reports completed jobs, total service time, and queue occupancy. Runtime engine errors enter the findings log; GitHub reporting is an engineering workflow, never a web-app side effect.

## API

| Route | Implemented behavior |
|---|---|
| `GET /api/places` | `q`, `category`, `branch`, optional current `version`, opaque query/version-bound cursor, limit 1–100 |
| `GET /api/places/{id}` | Current branch snapshot's node, typed neighbors, binding resolution, display metadata |
| `POST /api/discover` | `origin`, `branch`, `category`, `max_m`, optional completed historical `version` |
| `GET /api/subway` | Current branch subway topology; optional `branch` |
| `POST /api/subway/explore` | Station `seed`, `branch`, `depth` 1–6; outgoing BFS + induced subgraph |
| `POST /api/explore` | `seed`, branch, depth ≤3, limit ≤100; BFS and induced edges |
| `POST /api/scenarios` | Closure `{name, between, edges:[{src, edge_type:"street", dst}]}` |
| `POST /api/scenarios/{id}/operations` | `{id, closed, expected_version}`; idempotent close/reopen |
| `GET /api/scenarios/{id}/history` | Bounded completed operations and their ready versions |
| `POST /api/scenarios/{id}/impact` | Origin; compare current scenario with official city |
| `GET /api/graph-metrics` | Diagnostic job counters and occupancy |

Existing V1 endpoints remain available. `/api/gazetteer` still returns six aliases. The curated catalog is served separately.

## Verification and findings

Tests compare Strata discovery distances against the independent route implementation, validate bindings/relationships, verify before/after edge counts and parent isolation, exercise concurrent scenario admission, reopen durable history, and reject schema/version mismatches. Eight process-termination checkpoints cover import documents/graph/ready, scenario fork/batch/event/ready, and reopening after graph mutation. Writers exit without destructors; verification opens a new writer.

Browser checks exercise keyboard selection, search, details, BFS visualization, impact, close/reopen, mobile layout, and light/dark themes. [Benchmark artifacts](benchmarks/README.md) separate storage work, cached analytics, and end-to-end HTTP admission.

Source-confirmed issues #3472–#3474 now include measured scaling results. New [#3475](https://github.com/stratalab/strata-core/issues/3475) records 26–28 second graph-heavy forks using a minimal engine-only reproduction. The [friction ledger](friction.md) and System findings retain links/workarounds.

Remaining plan extensions are explicit: place editing, arbitrary multi-corridor operation composition, optional area memberships, further real POI catalogs, richer engine instrumentation, and the full combinatorial stress matrix. Iterative analytics remain optional. The current release supplies the curated product flow and executable baseline workloads; it does not claim those extensions have shipped.

Browser checks are reproducible with `tools/check_ui.cjs` against a disposable cache-mode server (default port 7453). Install Playwright outside the app or set `ISLAND_PLAYWRIGHT_PATH`; set `ISLAND_CHROMIUM` if supplying an existing browser. `ISLAND_URL` and `ISLAND_UI_OUTPUT` override the test server and screenshot directory. The script refuses to mutate a durable server.
