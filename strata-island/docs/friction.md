# Engine friction

Protocol:

1. `Log::push` a finding (deduped by title). Runtime hits use `hit`.
2. Add a row here.
3. File a **new** issue on `stratalab/strata-core`. Do not patch the engine from this app. Do not reuse an older ticket — the engine team owns duplicates.

Designed-around set (all filed from island, 2026-09-19):

| PR | Surface | Title | Issue |
|---|---|---|---|
| 1 | engine.graph | `sssp` is distances-only; no path unpack | [#3456](https://github.com/stratalab/strata-core/issues/3456) |
| 1 | engine.graph | No `list_edges`; adjacency drops properties | [#3457](https://github.com/stratalab/strata-core/issues/3457) |
| 1 | engine.graph | `list_nodes` full-loads then paginates; no bbox/nearest | [#3458](https://github.com/stratalab/strata-core/issues/3458) |
| 1 | engine.graph | `GraphService` reads still `&mut self` | [#3459](https://github.com/stratalab/strata-core/issues/3459) |
| 1 | engine.graph | `sssp` re-scans every edge for negatives | [#3460](https://github.com/stratalab/strata-core/issues/3460) |
| 1 | engine.graph | Node/edge graph cannot encode turn restrictions | [#3461](https://github.com/stratalab/strata-core/issues/3461) |
| 1 | engine.graph | Graph adapters refuse promotion | [#3462](https://github.com/stratalab/strata-core/issues/3462) |
| 1 | engine.open / ipc | Library-opened DBs do not host IPC | [#3463](https://github.com/stratalab/strata-core/issues/3463) |
| 1 | engine.graph | `bulk_insert` is many commits; crash is a half-city | [#3464](https://github.com/stratalab/strata-core/issues/3464) |
| 1 | engine.graph | Edge weight is `f64` only; no integer meters | [#3465](https://github.com/stratalab/strata-core/issues/3465) |
| 1 | engine.graph | Bindings cannot name another branch | [#3466](https://github.com/stratalab/strata-core/issues/3466) |
| 5 | engine.graph | Graph does not promote; compare is the landing | [#3462](https://github.com/stratalab/strata-core/issues/3462) |
| 5 | engine.branch | Durable parent-delete with live children is refused | engine `failed_precondition.engine.branch_has_children` (do not reuse #3196) |
| 5 | engine.event | Canonicalize `EventPayload` before `new` | engine canonicalize class (do not reuse #3188) |

Hits that land at runtime get a new row and a new issue, same protocol.

PR6 freeze: `CITY_VERSION = 1`. `POST /api/audit` is `graph_info` on `city` plus `verify_chain` on each desk. Failures are `Kind::Bug` with `error.code()`. The plat does not shell out to `strata`.

## Graph expansion planning findings — 2026-09-19

Inspected published `stratadb v1.2.3`, commit `6fc481c33473efd7d1724284107b67be08625dcd`. Cargo.lock, the local dependency source, and the remote tag agree. Current main was not tested. These are capability/scaling findings, not claims of measured latency regressions.

| Kind | Surface | Finding and evidence | Issue | Planned response |
|---|---|---|---|---|
| Gap | `GraphAdjacencyIndex::sssp` | No edge-type allow-list. An executed three-node cache probe returned distance 2 via semantic edges where the street-only distance is 100; filtered BFS excludes those edges. | [#3471](https://github.com/stratalab/strata-core/issues/3471) | Separate street and place-relationship graphs; join stable road-node references. |
| Friction | `GraphService::batch_write` | Source unconditionally builds complete node and edge maps for every nonempty batch, including a single-edge update. Timing/RSS scaling is not yet measured. | [#3472](https://github.com/stratalab/strata-core/issues/3472) | Benchmark fixed-size mutations against increasing graphs; retain atomicity and report batch cost separately. |
| Friction | `GraphService::nodes_by_type` | Source scans/sorts the full matching type index before applying cursor/limit. Timing scaling is not yet measured. | [#3473](https://github.com/stratalab/strata-core/issues/3473) | Benchmark uncached first/middle/last pages; bound product caches without hiding engine cost. |
| Friction | `GraphService::graph_info` | Source loads/decodes all nodes and edges to compute counts and updated version. Timing scaling is not yet measured. | [#3474](https://github.com/stratalab/strata-core/issues/3474) | Use acknowledged readiness versions for routine bookkeeping; measure explicit metadata audits. |

All four reports were filed and verified open. #3471 includes the complete runnable probe; the other reports include exact pinned source links and scaling reproduction specifications. The [expansion plan](graph-expansion-plan.md#12-engine-shortcomings-and-issue-process) defines the required reproduction, reporting, and regression workflow for subsequent implementation work. Repeated occurrences reuse the same island finding; a distinct newly discovered shortcoming gets a focused new report with related tickets linked.


## Curated V2 implementation findings — 2026-09-19

- **Measured scaling:** [#3472](https://github.com/stratalab/strata-core/issues/3472), [#3473](https://github.com/stratalab/strata-core/issues/3473), and [#3474](https://github.com/stratalab/strata-core/issues/3474) now have executed 1k/10k/100k cache results attached. See [raw artifacts and qualifications](benchmarks/README.md). Earlier source-only entries describe their original evidence, not the current reproduction status.
- **New engine shortcoming:** [#3475](https://github.com/stratalab/strata-core/issues/3475), graph-heavy `fork_current` takes 26–28 seconds at 100k nodes / 400k edges in a minimal engine-only release/cache probe. Related closed #2527 and older #1569 are linked in the report. Workaround: keep interactive city/branch counts bounded and isolate larger benchmarks. Current main is untested; no regression assertion is made. Reproducer: `cargo run --release --example fork_probe -- 100000`.
- **App durability configuration, fixed:** default `DurabilityMode::Standard` buffers writes and does not guarantee acknowledged-write survival on process kill. The fork checkpoint exposed this. Local opens now explicitly use `Always`; all eight termination checkpoints pass. This is documented engine behavior, not an engine defect.
- **App concurrency, fixed:** scenario ID allocation, cap validation, writes, and publication now share a mutation coordinator; concurrent creation cannot exceed city plus three scenarios.
- **OSM attachment quality:** the Met's initially selected western geometry point snapped to an isolated service-road component. Curation now selects a real vertex on the Fifth Avenue side. Inwood Hill Park remains unconnected beyond the configured radius. These are data/model issues, not engine shortest-path defects.
- **Ontology modeling:** links declare concrete endpoint types; category-specific relationship names preserve typed place classes without pretending the engine supports polymorphic source types.

## 550-place catalog expansion

- **App scaling fixes:** followed all bounded catalog pages instead of truncating at 100; derived semantic snapshot budgets from catalog capacity; bounded map markers/labels and hit-tested only visible points.
- **App versioning fixes:** additive catalog revisions preserve existing scenarios. Historical typed nodes, JSON bindings and neighbors now read one requested version; current scenario readiness includes catalog-only upgrades. These were application assumptions, not new engine defects.
- **Data quality:** 500 additional source-identified landmarks selected offline after boundary/infrastructure/inactive/duplicate filtering. 13 of 550 lack a road anchor within 200 m and remain explicitly unconnected. See [catalog notes](catalog-expansion.md).
- Existing engine issues #3471–#3475 remain the tracked limitations. Expanded curated stress profiles now include semantic typed paging, snapshot and BFS/subgraph measurements.

- **New engine limitation:** [#3477](https://github.com/stratalab/strata-core/issues/3477): durable `delete_graph` builds one commit containing all graph/index tombstones and can exceed the storage mutation limit. The 550-place graph exposed this on recovery of an unpublished initial import. Engine-only release/Always reproduction: a 3,000-node/3,000-edge ring imports with 512-row chunks, then deletion fails with `invalid_argument.engine.persistence` / `mutation count exceeds configured limit`; a 550-node/550-edge control succeeds. `examples/delete_graph_probe.rs` preserves the reproducer. Database open succeeds; no WAL corruption was observed. Fixed the app to resume staging imports via deterministic upserts and reuse frozen ontology, eliminating graph deletion from recovery. Current engine main is untested.

## Full pinned catalog (1,701 places)

The semantic graph now has 2,806 nodes and 3,366 relationships. `curated-all` storage/analytics and four-client HTTP results are in [the benchmark notes](benchmarks/README.md). The existing bounded-page UI and deterministic import replay scale to this catalog. The benchmark setup now follows all typed-index pages before choosing its middle/last cursors; this corrects an application benchmark assumption once the landmark type exceeds 1,000 nodes. No new engine issue was observed in the executed full-catalog workloads.

### Manhattan subway workload

Catalog v4 adds 151 MTA station locations and an independent subway graph with
842 directed service/transfer edges. All 151 BFS depth-two HTTP queries matched
independent traversal of the pinned input, and street closure forks retained
unchanged subway connectivity. No new engine defect was reproduced. The separate
graph continues the #3471 workaround; #3456 and #3457 remain relevant to future
path reconstruction and timetable metadata. See [subway implementation](subway.md)
and [the recorded workloads](benchmarks/README.md#manhattan-subway-catalog).

## Address-search planning findings — 2026-09-19

The [address-search implementation plan](address-search-plan.md) records the data,
product, migration, and graph workloads. These findings are source-verified API
gaps, not executed address-scale latency measurements or newly reproduced storage
defects. The app remains on published `stratadb v1.2.3`
(`6fc481c33473efd7d1724284107b67be08625dcd`). Inspected main
`f74c95cc6b542739148156c66db874d8c0074a73` has byte-identical graph service,
graph analytics, JSON service, and Database API files. Main was not built or
runtime-tested.

| Kind | Surface | Evidence / required capability | Issue | Planned fallback |
|---|---|---|---|---|
| Gap | `JsonService` | Index creation persists entries, but no public field-predicate/index query consumes them. Address search needs exact compound component lookup. | [#3482](https://github.com/stratalab/strata-core/issues/3482) | Immutable application exact/token indexes built from published documents. Related #2703, #2206, #1891. |
| Gap | embedded search | `Database` has no text-search service; JSON/graph reads have no token-prefix retrieval API with bounded work and version coherence. | [#3483](https://github.com/stratalab/strata-core/issues/3483) | Application prefix index; explicit candidate limits/truncation. Related #2252, #2249, #3017. |
| Gap | `GraphAdjacencyIndex::sssp` | One zero-cost source; no distance cutoff, weighted multiple seeds, winning-source labels, or algorithm work/cancellation budget. | [#3484](https://github.com/stratalab/strata-core/issues/3484) | Full SSSP and post-filtering; repeated incoming SSSP for explicit catchment workloads only. |
| Friction | historical hydration | JSON `batch_get` is Latest-only; historical graph/binding/document hydration requires individual reads. | [#3485](https://github.com/stratalab/strata-core/issues/3485) | Explicit same-version point reads and bounded snapshot caches. No historical correctness limitation is claimed for those reads. |
| Gap | mixed graph/document writes | Graph and JSON batches commit separately; no public mixed graph + bound JSON + KV atomic batch. | [#3486](https://github.com/stratalab/strata-core/issues/3486) | Staged deterministic upserts and a final publication manifest; related #3127 and #3464. |

All five reports include pinned source links, concrete address use cases,
acceptance criteria, and related issues for engine-team consolidation. Existing
island reports #3456–#3466, #3471–#3475, and #3477 cover the previously identified
spatial, path, edge-property, filtering, import, paging, metadata, fork, and
deletion constraints; the plan maps each applicable workaround. Add the planning
findings to the runtime log when their address feature/fallback is implemented.

The source audit also found 63,245 Manhattan rows but 63,244 distinct address-point
IDs, with a conflicting building reference on ID `5217738`. That is an NYC input
quality issue, not a Strata defect. The plan requires a recorded resolution and
explicit status/range/attachment policies before publication.

## Address implementation findings — 2026-09-20

The [implemented address layer](addresses.md) publishes 63,103 addresses and a
112,562-node / 186,200-edge relationship graph. The application adapters for
#3482–#3486 are implemented and appear in the runtime findings log. Exact/prefix
retrieval stays behind `search::Index`; versioned hydration, publication, and
street attachment have separate replacement points. Native Strata SSSP powers
station/radius/closure calculations, followed by application membership joins.

- **New measured friction:** [#3489](https://github.com/stratalab/strata-core/issues/3489):
  `GraphService::neighbors_with_selector` loads and hydrates all matching neighbors,
  sorts, then applies the cursor/limit. A first page of 10 at a 2,108-address street
  took 25.9 ms in the local release/Always probe. Output remains bounded; internal
  work is not. This is distinct from typed-node pagination #3473. The report
  includes pinned source and an executed measurement comment; main is untested.
- **Measured existing constraints:** address-only full cache fork took 16.5 s;
  full-city cache scenario creation took 23.7 s. The app moves creation off the
  async HTTP thread and reserves durable retry tokens to prevent duplicate forks.
  It does not claim to fix engine fork cost (#3475). Typed pages and metadata
  remain tracked under #3473/#3474. See [raw results](benchmarks/README.md#manhattan-addresses).
- **Data quality, resolved explicitly:** duplicate NYC ID `5217738` retains both
  source-row references and selects the later record with a valid BIN. Proposed,
  permitted, under-construction, and retired rows are excluded. Placeholder BINs
  never become building nodes. All 3,868 unconnected addresses remain searchable
  with a recorded attachment-failure reason; no synthetic road edges are added.
- **Recovery and versions:** deterministic staged upserts and a final readiness
  marker passed five process-kill/replay checkpoints. A copied original durable
  city/scenario database migrated and reopened with preserved scenario history.
  Road/catalog versions are explicit when comparing a pre-address scenario.
- **Correctness verification:** independent Dijkstra matched all 588 affected
  addresses in a disposable closure, including all 17 newly unreachable results.
  No radius cutoff was interpreted as unreachable. Existing route, persistence,
  catalog, subway, and browser flows passed alongside the new address checks.

## Car and Subway + walking — 2026-09-20

The [journey graph](journeys.md) adds 66,007 nodes / 201,567 edges, with pedestrian
access and coherent train-pattern states. Native Strata SSSP computes weighted
travel cost; application predecessor reconstruction checks equality before
assembling an itinerary. This reuses #3456 rather than filing another missing-path
issue. The metadata join and spatial candidate grid reuse #3457/#3458; broader
bounded multi-source access searches remain covered by #3484. Train patterns and
boarding assumptions are application policy, not claimed engine defects.

Six complete-graph HTTP cases matched independent Dijkstra; car-closure scenarios
left transit results unchanged. No distinct new engine failure was reproduced.
Three process-termination checkpoints verify staged journey import replay. The
larger graph also inherits the existing metadata and branch-fork scaling concerns
(#3474/#3475); the current feature does not claim to resolve those costs.

The larger cache fork exceeded a 30-second HTTP test timeout; a separate Always-durable creation sample took 1.51 seconds. Evidence and qualifications were added to [#3475](https://github.com/stratalab/strata-core/issues/3475#issuecomment-5748003372). Cache regression request timeouts now allow 120 seconds without treating those timings as product latency targets.
