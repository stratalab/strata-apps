# Strata Island: named places and graph workloads

Implementation update: the initial 50-place release has since expanded to all 1,701 eligible destinations. See [catalog revision 3](catalog-all.md) for the complete pinned landmark selection, migration behavior, and verification. The original planning scope below is retained for context.

**Status:** Curated V2 product flow and baseline stress tooling implemented. See [implementation status](v2-implementation.md) for shipped behavior, verification, and explicitly remaining workload extensions.\
**Date:** 2026-09-19.\
**Engine baseline:** published `stratadb v1.2.3`, commit `6fc481c33473efd7d1724284107b67be08625dcd`. Verified against Cargo.lock, local dependency source, and the GitHub tag.\
**Scope:** expand the named-place experience and exercise Strata's graph storage, typed relationships, traversal, analytics, branches, and historical reads.\
**Predecessor:** [V1 implementation plan](implementation-plan.md). This document governs the proposed next iteration; it does not retroactively change V1's frozen dataset contract.

## 1. Outcome

The next demonstration should make this sequence possible:

> Find a park or transit destination reachable within two kilometers. Inspect how it connects to the city. Close a street on a scenario. See which destinations get farther away or become unreachable. Move between the before/after versions and explain every graph change.

A larger list of labels is only the first deliverable. The defining features must invoke real Strata graph operations, return results derived from persisted graph state, and produce independently testable evidence. Smooth map rendering remains local to the browser.

The first expansion uses a **curated catalog of roughly 50 major landmarks, parks, and transit destinations**, as selected by the user. This is the total initial catalog, retaining the original six destinations through compatible entries/aliases, rather than 50 additional places. Selective map labels keep the console readable. The product scope is the supported Manhattan street network; larger real and synthetic datasets belong to the stress harness and do not expand the first-release catalog.

### Required outcomes

- Import a versioned OSM place snapshot, with source identity, category, real location, and an explicit connection to the road graph.
- Search and select places without a giant HTML dropdown or overlapping labels.
- Add graph-backed place inspection, category exploration, and network-distance discovery.
- Generalize closure scenarios to selected directed street segments; calculate their impact on destination reachability.
- Exercise current and historical graph reads and verify branch isolation after reopen.
- Ship reproducible storage/traversal/analytics workloads and a findings ledger linked to engine issues.

### Boundaries

- Distances describe the directed network. No live traffic, turn restrictions, legal driving claims, or invented travel times.
- Fetch OSM during extraction, not during UI interactions or benchmark runs.
- Do not promote graph changes while graph adapters lack promotion support.
- Keep the running V1 database intact. Build the next dataset in a separate directory.
- Do not patch strata-core from this app. Engine fixes arrive through a separately tested published release; a temporary local patch is only for reproducing/verifying an issue.

## 2. Baseline and verified engine surface

The app currently has 12,862 road nodes, 28,802 directed street edges, six named destinations, and one eight-edge closure. It already exercises graph import, adjacency construction, forks, edge deletion, branch comparison, and event verification. Its route implementation is application Dijkstra; its six JSON place documents are bound directly to road nodes.

The existing road extract is selected by a bounding box. Do not assume that its entire contents are strictly inside Manhattan's administrative boundary. Define the POI geography separately and record that choice.

The following surface was inspected in the **pinned release**, rather than inferred from old issue titles or current main:

| Capability | Available API | Planned use |
|---|---|---|
| Graph writes | `bulk_insert`, `upsert_node`, `upsert_edge`, `delete_node`, `delete_edge`, `batch_write` | Import, place edits, atomic street closures/reopens, mutation workloads |
| Typed nodes and schema | `define_object_type`, `define_link_type`, `freeze_ontology`, `nodes_by_type`, `ontology_summary` | Place types, relationship validation, category pages, integrity checks |
| Relationships | `neighbors` with direction/type/cursor; `get_node`, `get_edge` | Place-to-road/category/area inspection and graph change details |
| JSON bindings | `bindings_for_entity`, `resolve_binding_target` and historical variants | Resolve a selected graph place to its branch-local document |
| Snapshot reads | `adjacency_index`, `adjacency_index_at_version`, historical node/edge reads | Version-pinned analytics, history comparisons |
| Bounded traversal | `GraphAdjacencyIndex::bfs(GraphBfsOptions)`, `subgraph`, `degree` | Relationship explorer and bounded subgraph visualization |
| Distance and connectivity | `sssp`, `wcc` | Reachable places, nearest-by-network-distance, closure impact |
| Iterative analytics | `pagerank`, `personalized_pagerank`, `cdlp`, `lcc` | Optional topology diagnostics and separate stress workloads |
| Branch lifecycle | `fork_at_version`, `compare`, `delete` | Independent scenarios, version comparison, archive/reopen |

**Accounting distinction:** algorithms such as SSSP and BFS are Strata methods on an in-memory `GraphAdjacencyIndex`. They exercise the graph primitive's algorithm implementation, but do not read persistent rows on each run. Record snapshot construction, algorithm execution, application joins, and response serialization separately. Report direct storage calls separately from cached calls.

Source references for this audit are in section 13. Existing issues #3456–#3466 were checked and remain open as of this plan's date.

## 3. Product features mapped to graph operations

| Priority | Feature | Graph work that must actually happen | Acceptance evidence |
|---|---|---|---|
| P0 | Named-place catalog and selection | Import typed place nodes, relationship edges, and JSON bindings; hydrate from persisted state | Counts/digests match after restart; multiple places can share one road anchor without overwriting one another |
| P0 | Place details / “Connected to” | `get_node`, typed `neighbors`, binding resolution; inspect road anchor, category, and optional area | Returned relationship IDs correspond to engine edges at the requested version |
| P0 | Category browser | `nodes_by_type` for specific types; incoming `in_category` neighbors for broader groups | Stable paginated results; real page-cost telemetry, including uncached mode |
| P1 | “Reachable within 1 / 2 / 5 km” | Engine `sssp` on the directed road snapshot, then join its distances to graph-derived place anchors | Results match an independent small-graph oracle; unreachable and reverse-direction cases are correct |
| P1 | Nearest park / landmark / transit destination by road distance | One engine SSSP per origin/snapshot, category membership lookup, stable distance ranking | A geometrically closer place on an unreachable component never wins |
| P1 | Closure impact | Graph `batch_write` deletes exact street triples on a fork; parent/child SSSP plus `wcc`; branch `compare` | Show unchanged, farther, newly unreachable, and already-unreachable destinations separately |
| P1 | Relationship explorer | Bounded typed/directional BFS and `subgraph` | Limits and truncation are visible; semantic links never become drivable roads |
| P1 | Before / after history | Version-pinned node, edge, binding, adjacency, and JSON reads | Reopening reproduces the same identities, distances, and diffs for recorded versions |
| Later | Edit a place on a scenario | Update a place node, its binding/document, or its road relationship; inspect derived indexes and compare | Parent unchanged, child typed/binding indexes correct, crash repair idempotent |
| Later | Topology diagnostics | Road `degree`, `wcc`, optionally bounded-iteration PageRank/CDLP/LCC | Clearly labeled structural scores; never called popularity, traffic, or legal accessibility |

Do not add expensive graph calls to pan/zoom just to raise operation counts. User-triggered graph features and explicit benchmarks provide meaningful load.

## 4. Data acquisition and normalization

### OSM extraction

Add `tools/extract_places.py` beside the road extractor. It reads the existing road fixture; it must not regenerate or retune that graph while adding places.

1. Fetch named nodes, ways, and relations using a first-release allow-list for major landmarks (including museums and notable attractions), parks, and stations/terminals. Select accepted destinations through a checked-in curation manifest of stable OSM identities; do not publish every tag match. Broader categories such as food/drink, retail, hospitals, and charging stations are optional harness inputs or later product scope.
2. Restrict POIs using a pinned Manhattan boundary polygon and the supported road extent. Record the boundary's OSM identity/version/hash. A bounding-box query is only candidate retrieval. Keep boundary retrieval failures explicit instead of silently including neighboring boroughs.
3. Preserve `(osm_type, osm_id)`, canonical/alternate names, selected OSM tags, extraction timestamp, and attribution. Keep optional addresses, websites, and hours as source data; missing information remains absent. Exclude disused/demolished/proposed features from active destinations.
4. Use actual nodes, mapped entrances, or a point on the feature geometry for display/attachment. `out center` is a bounding-box center, not an entrance or a guaranteed interior point; flag center-only candidates and large-area fallbacks.
5. Deduplicate repeated representations conservatively. An OSM object identity is unique; a matching Wikidata identity plus compatible geometry can propose a merge. Same name or brand alone must not merge separate shops. Save aliases, source IDs, and merge/rejection reasons.
6. Emit deterministic records sorted by stable ID, a manifest, and a quality report. Commit normalized fixtures, source/query metadata, and hashes. Keep raw downloads as reproducible artifacts with retention/checksum documentation.

A previous exploratory midtown query returned 3,479 raw named features across a broad tag query, including 504 restaurant and 214 café features. It also included railway infrastructure and duplicates. These numbers demonstrate available source density; they are **not** accepted destination counts or Manhattan-wide estimates.

### Street attachment

Every accepted place has a real display coordinate and a separate optional road anchor:

- Prefer a mapped accessible entrance when available; otherwise choose a compatible nearby road node within the initial 200 m candidate radius.
- Match the same projected integer-meter coordinate system as the existing extract.
- Reject obviously incompatible connections across water/barriers; classify uncertain cases as approximate. Audit famous/large destinations manually. Nearest-point geometry alone does not prove access.
- Preserve the original six destination IDs and their existing route anchors through aliases, especially Port Authority → Grand Central.
- A place with no acceptable anchor remains searchable and visible, with “Routing connection unavailable.” Never attach it arbitrarily to make a test pass.
- Record attachment method, anchor ID, estimated approach distance, and quality flag. Repeated imports with the same inputs must produce the same attachment.

The initial route/network-distance result measures **road distance between anchors**. Show any estimated off-network approach distance separately; do not turn a geometric connector into a legal driving segment. The current golden route remains a road-distance assertion.

### Curation and dataset sizes

Target roughly 50 distinct destinations across landmarks, parks, and transit, distributed across the supported Manhattan extent. Preserve the original six without counting aliases as extra destinations. Prefer recognizable destinations with useful route endpoints; avoid filling the catalog with duplicate station entrances or subfeatures of a single landmark. Record category, selection rationale, source identity, and manual display/attachment review in the curation manifest. Report the exact accepted count after validation; do not invent or duplicate places to hit exactly 50.

The default V2 product and its release checks use this curated catalog (`curated-50`, with the actual count in its manifest). Development also retains the original-six baseline. Larger real profiles (250, 1k, 10k where available, and `real-full`) are optional harness-only extractions with separate manifests; they are not prerequisites for the initial catalog release. Synthetic 1k/10k/50k/100k profiles provide deterministic scaling coverage regardless of available real POI counts, with explicit synthetic identities that never appear as real destinations. The unchanged road graph still provides 12,862 nodes and 28,802 directed edges for product graph workloads.

## 5. Graph and document model

Keep **two graphs in the same branch and product space**:

| Graph | Nodes | Edges | Semantics |
|---|---|---|---|
| `manhattan` | Existing `n:<osm-node-id>` intersections | Existing directed `street` triples, positive integer-meter weights | Road routing and network analytics |
| `places` | `p:<osm-type>:<osm-id>`, `anchor:<road-node-id>`, `category:<slug>`, optional `area:<osm-id>` | `near_road` place → anchor, `in_category` place → category, `in_area` place → area | Place relationships, typed discovery, bindings, traversal |

An anchor is a reference node with a `road_node_id` property. It is not an engine-supported edge across two graphs; the application resolves the stable ID into `manhattan`. Deduplicate anchors shared by many places.

Give first-release places concrete object types such as `Landmark`, `Museum`, `Park`, and `Station`; group them with category relationships such as `landmarks`, `parks`, or `transit`. Broader types can be introduced in independently versioned harness or later product schemas. Define allowed relationship endpoint types and required properties, then freeze the ontology after validation. Store locations and routing-reference properties on the appropriate nodes. Store richer display metadata in one JSON document per canonical place and bind the **place node** to it, with branch omitted so the target resolves locally on a fork.

Initial cardinality: zero or one `near_road` relationship per place, one primary category, optional validated area memberships. This keeps attachment semantics explicit. Multiple entrances are a later modeled feature, not an accidental way to bridge disconnected roads.

With `P` places, `A` unique anchors, `C` categories, and `R` areas, the place graph has `P + A + C + R` nodes. Report exact membership/attachment counts rather than assuming every place has every edge.

### Why the graphs are separate

The pinned SSSP API accepts source and direction, but no edge-type filter. A compiled three-node probe returns distance 2 through category edges where a street-only route is 100. BFS can filter those edges. A unified heterogeneous routing graph would therefore require unsupported filtering or replacement analytics. See [#3471](https://github.com/stratalab/strata-core/issues/3471).

This split retains a real typed place graph while preserving correct engine road-distance queries. Weighted road queries never run over category, area, or place-association edges.

### Resolve the current binding constraint

V1 binds a single JSON destination directly to each selected street node. Its import uses a `HashMap<road_node, poi>`: multiple businesses snapped to one intersection would overwrite that binding choice. Bind each new place's own graph node instead. Retain V1 aliases in a versioned compatibility map; change binding tests to assert place ↔ document and place → anchor → road resolution independently.

## 6. Versions, import, scenarios, and recovery

### Separate version axes

- `CITY_VERSION = 1` and the road fixture hash remain the identifier of the unchanged road extract.
- Introduce `APP_SCHEMA_VERSION = 2` and an independently versioned `PLACE_CATALOG_VERSION` plus catalog/boundary hashes.
- Every branch has a readiness manifest containing dataset hashes, schema version, completed operation IDs, and acknowledged component commit versions.
- **Do not fork using only `manhattan.graph_info().updated_version()`.** Place documents, the place graph, and schema writes can be newer than the road graph. Use the commit version returned by the final dataset-ready JSON write, after all required components are complete. Refactor the adapter to retain write acknowledgements rather than discarding them.

Write each completed readiness record as an immutable `ready:<operation-id>` JSON document. On restart, recover its commit version with `JsonService::get_versioned`; this API returns the row's commit metadata in the pinned release. A mutable branch-head pointer may refer to that record, but its own later write version is not the dataset version. A crash before updating the pointer leaves a discoverable completed/pending operation to reconcile. Do not attempt to embed a write's unknown future commit version inside its own JSON payload.

### Import state machine

Proposed stages: `seeded → documents_written → graphs_written → validated → ready`.

Write deterministic IDs and replayable import batches. Use `bulk_insert` for large graph loads with an explicit chunk policy and record acknowledged progress. The engine's bulk operation commits multiple chunks; a stage marker must never imply that an incomplete graph is ready. Write bound JSON targets before graph bindings. Validate both graphs, ontology, attachments, and bindings before the final readiness acknowledgement. Expose the catalog only after the complete ready state is available.

Do not infer a successful import from counts alone: validate stable ID/edge digests, fixtures, binding targets, and stage hashes. Keep partially imported data quarantined from product requests. Reopening resumes or rebuilds a staging branch deterministically.

### Scenario mutations

- Allocate scenario IDs and enforce branch limits inside one serialized mutation operation. The current separate atomic counter read/DB lock/RAM publication permits concurrent request races; fix this app-side before stress runs.
- Store a pending operation document **before** mutation: operation ID, parent readiness version, exact edge triples, expected preimages/weights, and intended changes.
- Close a bounded corridor using one `GraphBatchWrite` on `manhattan`. Restore its recorded preimages for reopen. Show directed edges separately; a street name alone is insufficient to identify a corridor.
- Graph batches are atomic within a graph. JSON/event/graph writes do not form one public cross-capability transaction. Append the completion event and final ready marker after successful graph mutation; reconcile pending operations on restart using operation IDs and actual edge state. Do not emit duplicate completion events on retries.
- Set an initial interactive cap of 200 directed mutations per operation. Larger scripts run through the stress harness with explicit limits. The cap is an app limit, not a claimed engine batch maximum.
- Publish the new branch snapshot only after its complete state is ready. Keep an immutable previous snapshot available during work. Never expose a half-applied route/catalog pair.
- Generalize `repair_or_load_desk`: it currently assumes every desk means “delete the frozen eight 42nd Street edges.” V2 recovery must replay the scenario's recorded operations and must not repair arbitrary scenarios into a 42nd Street closure.
- Archive is serialized with selection/publication. Expire per-branch caches and pending history jobs; the official city remains usable.

A place edit spanning its JSON document and the place graph follows the same pending/completed protocol. Correct errors and recoverability are part of the feature, not deferred harness work.

### Rollout / existing data

Add an explicit dataset selection in the planned CLI (`--dataset v1|v2`). During development V1 stays the default. V2 uses a new directory, for example `--dataset v2 --db ./island-db-v2`; it refuses a mismatched nonempty directory with a clear dataset/schema error. The application never deletes or silently reimports `./island-db`.

Prove a V2 cold open, clean reopen, crash recovery, and the original route/closure golden before changing the shipped default. Existing V1 scenario conversion is deferred; V1 remains available for rollback. Document the new default/path together when that release is made.

## 7. Query architecture and application boundaries

```text
Offline OSM extraction → normalized fixtures + manifest
                                      ↓
                        Strata JSON + two graphs
                                      ↓
                  branch/version-pinned snapshots
                    ↙                 ↘
       Strata graph algorithms       display/search indexes
                    ↘                 ↙
                     API → browser console
```

### Snapshot ownership

Use immutable `Arc<BranchSnapshot>` values indexed by branch identity and its ready version. A snapshot contains a road `GraphAdjacencyIndex`, the app's polyline index, a place graph snapshot when needed, and catalog/search/attachment indexes derived from graph/document reads at the same version.

Cache validity includes graph identity, branch, ready version, road/catalog hashes, and projection options. A child mutation invalidates only affected child caches; it must not rebuild the parent. Retain full historical snapshots only within explicit LRU/count/byte limits. Old views must either return their requested version or an explicit unavailable-version error, never quietly switch to latest.

Do not clone the full city on every request as the current `DriveIndex` interface does. Measure retained snapshot memory per branch and history entry. A category index or origin-distance cache must be labeled application-side in metrics.

### Engine operations per feature

- **Search text:** bounded, accent/case-normalized RAM search hydrated from Strata; exact/prefix/token matches with stable tie-breaking. This is an application text index, not a claimed native graph full-text query.
- **Category pages:** call `nodes_by_type` or typed `neighbors` with a version and bounded page size. Cache product reads if measured cost warrants it; the benchmark must still exercise the uncached engine path.
- **Details:** fetch place node, typed relationships, and resolve its JSON binding at one requested version. Avoid an unbounded N+1 detail fetch for every search result.
- **Network discovery:** use engine `sssp(origin, Outgoing)` once per road snapshot/origin. Post-filter by distance budget and graph-derived category/anchor membership. Rank deterministically by road distance then place ID. Report total eligible/unreachable counts separately from a capped result page. Engine SSSP currently computes the full reachable set even for a small UI radius; record that work honestly.
- **Route preview:** engine SSSP provides distance/reachability; app Dijkstra reconstructs the selected polyline while #3456 remains open. Check their distance agreement. Compute paths only for chosen results, not for every place in a category.
- **Closure impact:** compare source-specific parent/child distance vectors keyed by stable road IDs. WCC explains weak topological fragmentation; it does not prove directed reachability. A strongly connected-component feature is not assumed available.
- **Graph explorer:** BFS defaults to explicit depth 2 and 200 visited nodes, allowed relationship types, and direction. Return `truncated`; subgraphs are induced by the selected node set and must not be called weighted route results.
- **Audit/history:** read graph/node/edge/binding/JSON states at an explicit version and compare identities, not just counts. `graph_info` is an explicit measured audit operation, not a poll-driven freshness token.

### Concurrency and budgets

Run graph I/O in bounded `spawn_blocking` work, with a serialized mutation coordinator. Never hold the DB mutex while awaiting or while rendering. Release it after acquiring immutable engine snapshots before running CPU analytics where the API permits. Time queue wait, lock wait, graph work, snapshot construction, algorithms, joins, and serialization separately.

Start with at most two concurrent expensive analysis jobs and a bounded queue of eight; expose busy/retry state. Deduplicate identical in-flight queries and discard stale UI responses. Disconnecting an HTTP client does not automatically cancel a running blocking task: count it until it finishes, and use explicit graph/iteration budgets. Prove API `Send`/`Sync` requirements in the first implementation spike rather than wrapping uncertain handles in unsafe code.

Keep the UI live cap at city + three scenarios. Stress-mode branch counts are separate and must not force every branch into RAM. Per-graph analytics budgets come from validated manifests and configured hard ceilings; do not merely reuse the existing 20k/80k budget for an expanded catalog. Exceeding a budget is a visible, measured refusal, never silent truncation.

## 8. API and console changes

These are proposed routes; none is implemented by this plan.

| Endpoint | Purpose / constraints |
|---|---|
| `GET /api/places?q=&category=&branch=&version=&cursor=&limit=` | Search/category page; default 20, max 100; opaque cursor bound to query and version |
| `GET /api/places/{id}?branch=&version=` | Details, real location, attachment quality, categories, binding provenance |
| `POST /api/discover` | Origin, category, max network meters, branch/version; bounded result page, explicit total/truncation |
| `POST /api/explore` | Seed place/category, direction, relation allow-list, bounded depth/node count |
| `POST /api/scenarios` | Fork the ready official city and apply a validated closure operation |
| `POST /api/scenarios/{id}/operations` | Close/reopen exact segments or edit a place; idempotency key and expected ready version |
| `POST /api/scenarios/{id}/impact` | Origin, category/catalog scope, parent/child versions; distance/reachability changes |
| `GET /api/scenarios/{id}/history` | Completed operation IDs and ready versions, paginated |
| `GET /api/graph-metrics` | Bounded developer diagnostics; cumulative counters, recent samples, cache status |

Every result identifies its branch, dataset, and version. Validate numeric ranges, names, body sizes, page limits, and job limits. Keep existing `/api/city`, `/api/route`, `/api/compare`, `/api/archive`, and `/api/audit` compatibility while adapting the UI. `/api/close` remains a convenience wrapper for the canonical 42nd Street operation. Keep `/api/gazetteer` as the six compatibility aliases during transition; the new UI does not fetch the full catalog through that endpoint.

Preserve the current console design:

- Accessible destination combobox with keyboard navigation, category chips, loading/empty/error states, and at most a small result list in the DOM.
- Places appear at their real coordinates. Route endpoints show the road anchor and optional connector so the two locations are not confused.
- Category-specific icons, viewport culling, spatial label collision handling, and zoom thresholds. Never draw every catalog label at once.
- Place detail sheet: name/type/address where present, route action, routing-connection status, and a “Connections” graph view.
- Discovery results show road distance, selected category, and branch. No synthetic ETA.
- Closure impact highlights affected destinations and separates “already unreachable” from “became unreachable.” Use both color and text/icon differences.
- Developer metrics remain in System. The normal flow speaks about places, routes, and scenarios rather than graph node identifiers.

## 9. Stress harness and measurement contract

Add a standalone `island-stress` binary plus deterministic fixture generation and report scripts. The following commands describe the proposed interface, not commands available today:

```sh
cargo run --release --bin island-stress -- \
  --profile curated-50 --mode durable --db "$RUN_DIR/db" \
  --seed 42 --report "$RUN_DIR/report.json"

cargo run --release --bin island-stress -- \
  --profile synthetic-100k --mode cache --seed 42 \
  --memory-mb 2048 --concurrency 4 --report "$RUN_DIR/report.json"
```

The harness refuses an existing nonempty unowned database directory. Each run uses its own directory; it never opens the server's database or other applications' data. Source IDs/coordinates from synthetic fixtures cannot be mistaken for OSM facts.

### Workload matrix

| Dimension | Initial ladder |
|---|---|
| Real data | Six-place baseline; required curated ~50 product catalog; optional harness-only 250 / 1k / 10k / full extraction (when available) |
| Synthetic topology | 1k / 10k / 50k / 100k nodes; independently recorded 4×–10× directed-edge ratios |
| Shape | Grids, one-way cuts, disconnected islands, shared anchors, large category stars, skewed/high-degree hubs |
| Storage | Cache and durable local; cold open and warm repeat; same seed and authored graph |
| Bulk chunks | 128 / 512 / 800; identical records and logical final state |
| Mutation size | 1 / 8 / 100 / 200 edges interactive-shaped; 1k in explicit stress jobs |
| Branches | 1 / 4 / 16 / 32; 128 only after measured resource headroom |
| Query concurrency | 1 / 4 / 8 / 16 clients; measure queue/admission effects as well as service latency |
| Churn | 100 / 1k close–reopen–compare–archive cycles with deterministic edge sets |
| History | Current, pre-edit, post-edit, after reopen; retained-version failure handled explicitly |
| Iterative algorithms | Fixed explicit iteration ceilings and convergence criteria on selected profiles |

### Isolated workloads

1. Import JSON, nodes, edges, ontology, and bindings; measure each phase and final validation independently.
2. Cold graph snapshots, warm graph snapshots, cached snapshot acquisition, and app index construction.
3. First/middle/last typed and neighbor pages; constant result limit as total category size increases.
4. Node/edge reads, forward/reverse binding lookups, and version-selected reads.
5. BFS, subgraph, degree, WCC, SSSP; report algorithm time separately from acquiring the snapshot. Include typed filters, high-degree truncation, and many origins.
6. Small `batch_write` operations against increasing graph size, with equivalent single-operation baselines where semantics match.
7. Fork only; graph mutation only; snapshot rebuild only; compare only; archive only. Also measure the complete product action.
8. Graph metadata reads (`graph_info`) as an isolated scaling workload.
9. Repeated place metadata/type/binding edits and removals; verify reverse/type index cleanup and branch isolation.
10. Subprocess crash injection during import, fork/closure, document updates, event completion, and readiness publication. Terminate the writer process, then reopen in a new process. A clean `drop`/reopen test alone is not crash recovery.

### Metrics and reproducibility

Every report records app/engine commit, build profile, OS/CPU/RAM, storage device/filesystem, seed, profile, fixture hashes, graph counts, degree distribution, budgets, branch count, selected versions, warmup, repetitions, concurrency, and outcome codes.

Collect p50/p95/p99 and sample counts for query/mutation workloads, throughput, cold-open/import time, peak process RSS, total database bytes, retained snapshot bytes, and available journal/WAL growth. Report refusal/timeout/error rates and excluded/cancelled jobs. Record graph API invocations, requested/returned rows, cache hits/misses, and serialization bytes at the adapter boundary. Do not invent scanned-row counters if the engine does not expose them; distinguish source-inferred complexity from measured work.

For latency distributions: at least 20 warmup operations and 200 measured operations per repeat where affordable, three repeats. For cold import/reopen: at least three independent directories/processes. Report individual cold samples rather than presenting a meaningful p99 from three points. Separate allocator/page-cache warming from app snapshot-cache warming.

Initial **product targets**, to calibrate on a named reference machine in P0: cached search p95 ≤100 ms; selected-place detail/category page p95 ≤250 ms; network discovery p95 ≤500 ms on the curated ~50-place product catalog; a small closure-to-visible-results p95 ≤2 s. These are proposed UX budgets, not observed results or engine guarantees. Freeze the reference machine and baseline before accepting performance claims. Treat repeatable regressions above 20% on matched workloads as investigation triggers, with variability shown.

Stress profiles are allowed to exceed product budgets: the useful result is the saturation curve, failure mode, and reproducible report. Abort runaway runs at configured memory/disk/time limits and report the boundary reached.

## 10. Correctness and recovery tests

### Data/model

- Golden normalized catalog hashes, stable identities, alias preservation, conservative deduplication, geographic inclusion, and rejection counts. Verify curated-manifest membership, coverage of all three destination groups, and manual review of every selected display point/attachment.
- Named polygons/relations, centers outside geometry, entrances far from centers, duplicate names, missing optional tags, shared road anchors, and unconnected places.
- Frozen ontology rejects invalid endpoint types/properties; type and binding indexes survive edits, delete, fork, and historical reads.
- Changing catalog/schema versions cannot silently reopen a mismatched directory.

### Graph queries

- Independent small directed-graph oracle for distances, ranked eligible places, and reachability. Include same-anchor zero distance and reverse direction.
- Semantic-edge shortcut fixture from #3471; production road queries must be immune to it.
- BFS depth/node/type limits and `truncated`, induced-subgraph edge membership, WCC semantics, stable paging across ties/tombstones.
- Parent/child equality before mutation; exact changed street triples afterwards; unchanged place graph for road-only closure.
- Every compared graph is read at the same intended branch/version; graph diff counts alone are insufficient.

### Persistence/concurrency

- Cache and durable import, clean reopen, and process-kill tests at named state-machine boundaries.
- Failures after graph commit but before event/ready marker; failures after event but before ready marker; duplicate retry/recovery.
- Concurrent create requests cannot duplicate IDs or exceed caps; close/archive/read races never publish mixed versions.
- Version-tagged responses are either complete snapshots or explicit errors. No fallback from a requested historical version to latest.
- Memory ceilings refuse oversized graph snapshots cleanly; shutdown/restart releases resources; repeated archive/recreate does not retain app caches indefinitely.

### Browser/product

- Search keyboard navigation, category paging, click-to-route, custom intersections, selected-place map/anchor distinction, reduced-motion behavior, mobile/desktop layouts, and safe rendering of external OSM strings.
- Search and analysis responses arriving out of order cannot replace a newer selection.
- Map camera/label toggles make zero graph API calls after required snapshots load.
- Existing Port Authority → Grand Central route and 42nd Street closure invariants remain true.

Run focused tests as each phase lands, then `cargo test`, `cargo clippy --all-targets -- -D warnings`, formatting checks, and the relevant browser/harness scenarios. Keep correctness tests deterministic; wall-clock thresholds belong in controlled performance runs.

## 11. Delivery sequence

Each phase is independently reviewable and ends with a runnable app or reproducible report. Estimates are planning ranges for one engineer and exclude upstream engine fixes.

| Phase | Work / primary files | Exit gate | Estimate |
|---|---|---|---|
| P0 | Capability probes, operation timing/counters, `docs/friction.md`, baseline reports | Verified API matrix, current six-place performance/correctness baseline, filed findings | 1–2 days |
| P1 | `tools/extract_places.py`, `fixtures/places-*.json`, manifest and `src/places.rs` | Reproducible curated ~50 destinations, reviewed curation manifest/quality report, unchanged road hash, extraction rules encoded in tests | 2–3 days |
| P2 | Versioned import, two graphs, ontology/bindings, `src/store.rs`, `src/world.rs`, recovery tests | Separate V2 DB imports/reopens; original route golden passes; shared-anchor correctness | 3–4 days |
| P3 | Place/search/detail APIs and console combobox/layers/detail sheet | Curated ~50-place catalog usable on phone/desktop; category/details trace to graph reads | 2–3 days |
| P4 | `src/analysis.rs`, engine SSSP/BFS/subgraph/WCC features, versioned snapshots | Network discovery and relationship explorer pass independent oracle tests and expose real timings | 2–3 days |
| P5 | `src/scenario.rs`, generalized close/reopen, impact and history APIs | Arbitrary bounded closure, exact graph diff, impact classifications, process-kill recovery | 3–4 days |
| P6 | `src/bin/island-stress.rs`, generators, report tooling, targeted regressions | Cache/durable scaling report, concurrency/churn/recovery evidence, all actionable findings tracked | 2–3 days |

Expected total: approximately 15–22 engineering days. P6's harness foundations begin in P0; workload coverage grows in each phase. Place-edit UI and iterative-analytics UI in the priority table are follow-ups after the required discovery/closure flow; their engine workloads can be exercised by the harness before those optional product views ship.

Dependency order: P0 → P1 → P2 → P3/P4 → P5 → final P6 report.

## 12. Engine shortcomings and issue process

### Already tracked

The existing [friction ledger](friction.md) records #3456–#3466: missing SSSP paths, edge-property/listing limitations, node paging/spatial queries, mutable graph handles, repeated negative-weight checks, turn restrictions, promotion, IPC, bulk-import atomicity, integer weights, and cross-branch bindings. Keep links and reproduction status current. An open ticket is not proof a given release still has a behavior; rerun the relevant probe on upgrade.

### Findings from planning and implementation

| Issue | Finding | Evidence | Design consequence |
|---|---|---|---|
| [#3475](https://github.com/stratalab/strata-core/issues/3475) | Graph-heavy forks take 26–28 seconds at 100k nodes / 400k edges | Executed engine-only release probe; [scaling results](benchmarks/README.md) | Bound interactive graph/branch counts; isolate large stress processes |
| [#3471](https://github.com/stratalab/strata-core/issues/3471) | SSSP lacks edge-type filtering | Compiled/executed three-node cache probe plus pinned API inspection | Separate road and semantic graphs; no category shortcuts |
| [#3472](https://github.com/stratalab/strata-core/issues/3472) | Nonempty `batch_write` loads the full node/edge maps even for one edge | Pinned source inspection and [executed scaling runs](benchmarks/README.md) | Isolate batch cost; coalesce edits; preserve atomic semantics |
| [#3473](https://github.com/stratalab/strata-core/issues/3473) | `nodes_by_type` scans/sorts the full matching type index before cursor/limit | Pinned source inspection and [executed scaling runs](benchmarks/README.md) | Explicit uncached paging benchmark, bounded product cache |
| [#3474](https://github.com/stratalab/strata-core/issues/3474) | `graph_info` scans/decodes all nodes and edges for counts/version | Pinned source inspection and [executed scaling runs](benchmarks/README.md) | Use acknowledged ready versions for cache/fork bookkeeping; explicit audits |

These reports were opened on `stratalab/strata-core` during this planning pass and link to exact source. They describe gaps or scaling limitations, not unproven correctness bugs. Current main has not been tested.

### Required workflow throughout implementation

1. Record every encountered engine gap, friction, bug, or questionable contract in `docs/friction.md`. Runtime occurrences also enter `findings::Log` with the engine error code/class and operation context.
2. Separate engine behavior from app mistakes, OSM quality, unsupported product semantics, and deliberate budgeting. Track app fixes locally; do not file an engine defect for a client-side race or poor map attachment.
3. Minimize the reproduction. Record app/engine commits, dataset/graph sizes, storage mode, branch/version selectors, seed, exact API sequence, expected/actual results, frequency, and command/output. Performance findings need matched baselines and metrics; source-only findings must say so.
4. Search related tickets. Following this app's existing protocol, open a **new focused island finding** for a newly discovered shortcoming and link related older engine issues; do not silently substitute an old ticket. Repeated occurrences of the same island finding reuse its existing island ticket. Avoid broad omnibus reports and automatic issue creation on every request.
5. File on `stratalab/strata-core` as part of the implementation/QA work, using a body file or structured tool so code and newlines survive. The user has explicitly requested this reporting. No credentials or GitHub writes belong in the normal running web app.
6. Put the resulting URL in the local ledger and, where user-relevant, the compile-time/runtime findings panel. Record the workaround, feature impact, confidence/reproduction status, and regression test to add when fixed.
7. Treat data corruption, branch leakage, incorrect distances/diffs, and unrecoverable writes as blockers for the affected feature. Performance gaps can ship with measured limits and an explicit workaround; hiding the workload behind an app cache is not a benchmark result.
8. A phase is complete only when its actionable findings have issue URLs, or an explicit failed-filing record if GitHub is unavailable. After an upstream fix, verify a published release, rerun the reproduction and matched baseline, then update status without erasing history.

### Completion criteria

The expanded demo is complete when its curated roughly 50-destination catalog, graph discovery, closure impact, history, and recovery flows work from a clean V2 directory and after restart; the V1 demo remains available; required UI/browser/correctness checks pass; the stress report separates storage work from cached analytics; and every confirmed engine shortcoming encountered has a traceable report.

## 13. References

Pinned engine source (all at the audited commit):

- [Graph service: writes, bindings, typed reads, historical snapshots](https://github.com/stratalab/strata-core/blob/6fc481c33473efd7d1724284107b67be08625dcd/crates/engine/src/data/graph/service.rs)
- [Graph adjacency representation and budgets](https://github.com/stratalab/strata-core/blob/6fc481c33473efd7d1724284107b67be08625dcd/crates/engine/src/data/graph/adjacency.rs)
- [BFS, degree, and subgraph](https://github.com/stratalab/strata-core/blob/6fc481c33473efd7d1724284107b67be08625dcd/crates/engine/src/data/graph/traversal.rs)
- [SSSP, WCC, and LCC](https://github.com/stratalab/strata-core/blob/6fc481c33473efd7d1724284107b67be08625dcd/crates/engine/src/data/graph/analytics.rs)
- [PageRank, personalized PageRank, and CDLP](https://github.com/stratalab/strata-core/blob/6fc481c33473efd7d1724284107b67be08625dcd/crates/engine/src/data/graph/iterative.rs)
- [Versioned JSON reads for readiness recovery](https://github.com/stratalab/strata-core/blob/6fc481c33473efd7d1724284107b67be08625dcd/crates/engine/src/data/json/service.rs#L172)
- [Graph types and ontology](https://github.com/stratalab/strata-core/blob/6fc481c33473efd7d1724284107b67be08625dcd/crates/engine/src/data/graph/ontology.rs)

OSM and extraction:

- [OSM map feature tags](https://wiki.openstreetmap.org/wiki/Map_features)
- [Overpass query composition](https://dev.overpass-api.de/overpass-doc/en/criteria/chaining.html)
- [Overpass output geometry](https://dev.overpass-api.de/overpass-doc/en/full_data/osm_types.html) and [bounding-box centers](https://dev.overpass-api.de/overpass-doc/en/targets/formats.html)
- [OpenStreetMap attribution and license](https://www.openstreetmap.org/copyright)
- [Current extraction recipe](../tools/extract.md), [V1 friction ledger](friction.md), [existing tests](../tests)
