# Manhattan addresses

Implemented 2026-09-20 on published `stratadb v1.2.3` (`6fc481c33473efd7d1724284107b67be08625dcd`). The [implementation plan](address-search-plan.md) records the design and upstream gaps.

## What is available

The Explore and Directions search fields now find addresses, landmarks, parks, and stations. Try `230 W 55th St`, `350 fif`, `270 E 2 St`, or `94½ Greenwich Street`. Selecting a result opens the existing map card and zooms to it. Address cards support route endpoints, nearby stations by directed street distance, addresses within 1 km, and a bounded relationship explorer. Closures includes **Compare address access**, with paged affected addresses.

Search ranks exact names first, then leading-name matches; name-only searches favor places over numbered addresses, while house-number queries enforce the exact number. Search is local after import; no runtime geocoding service is called. Addresses are fetched as small result pages, not downloaded into the browser's landmark layer. The 1,852 existing destinations, missions, and static subway network are preserved.

The first catalog accepts **63,103 address points**, of which **59,235** have approximate road anchors and **3,868** remain searchable without routing. The new `addresses_v1` graph contains **112,562 nodes and 186,200 directed relationships**. Those four graphs total **128,610 nodes / 219,510 edges** before closures. The subsequent [journey graph](journeys.md) brings current city totals to **194,617 nodes / 421,077 edges**. Graph records are not a count of unique physical locations.

## Source and quality

Pinned inputs and metadata are in `fixtures/address-source/`; acquisition is explicit through `tools/fetch_addresses.py`. The [NYC table](https://data.cityofnewyork.us/City-Government/AddressPoint/uf93-f8nk) underlies the user's map view. SHA256 hashes, retrieval time, borough filter, query URL, and source update time are recorded in the source manifest. Fetching refuses to overwrite an existing pin and rejects changing row counts/update metadata during acquisition.

`python3 tools/extract_addresses.py` regenerates the normalized catalog offline. It verifies source hashes, uses explicit normalization/attachment rules, and leaves the frozen road/place/subway fixtures unchanged. `fixtures/addresses-quality.json` accounts for all 63,245 raw rows: 63,103 accepted, 141 excluded by status, and one collapsed duplicate. `fixtures/address-resolutions.json` records the duplicate resolution and retained source identities. NYC documents [million-valued BINs as unknown/unassigned](https://github.com/CityOfNewYork/nyc-geo-metadata/blob/main/Metadata/Metadata_BuildingFootprints.md); they do not become building nodes.

Default search includes built and null-status records. Null status is displayed as unverified. Proposed, permitted, under-construction, and retired entries are excluded from suggestions and retained in the quality report. The current domain dictionary is pinned with the export. Source ranges stay ranges, fractions and suffixes remain part of the house number, and apartment/unit inputs ask for a street address instead of claiming an exact unit match.

Attachments use a local grid of named road segments. A candidate must match the normalized source street, be within 100 m of its segment, and have an incident road node within 200 m. Equally close unrelated segments and small isolated components are rejected. Roosevelt Island and Marble Hill ZIPs are explicitly outside the street coverage. Attachment evidence records the segment, distance, candidate count, and component size. The quality report has 3,451 unmatched addresses, 18 ambiguous attachments, and 399 outside the supported street coverage.

These are geometric anchors, not surveyed entrances or legal access instructions. Routes and radius/station results report directed street-network distance; the dashed approach is separate and approximate. The street graph is not a walking graph and has no turn restrictions. The nearby-station feature does not ride the subway or estimate arrival times; [Directions](journeys.md) separately offers Subway + walking itineraries.

OSM enrichment preserves all 936 source address tags in `address-enrichment-v1.json`. Exact normalized address agreement plus coordinates within 80 m produces 762 conservative place/address links. Matching both an address and its linked place is deduplicated in suggestions; explicit numbered-address input favors the address. Existing place documents are unchanged.

## Persistence and replacement interfaces

| App adapter | Responsibility now | Native replacement |
|---|---|---|
| `src/search.rs::Index` | Immutable exact/token/prefix lookup derived from published records; numeric constraints, bounded term expansion/candidates, deterministic cursors | Indexed fields [#3482](https://github.com/stratalab/strata-core/issues/3482), embedded search [#3483](https://github.com/stratalab/strata-core/issues/3483) |
| `src/addresses.rs::load` | Same-version typed graph/document hydration and membership validation | Historical batch hydration [#3485](https://github.com/stratalab/strata-core/issues/3485) |
| `src/addresses.rs::import_rows` | Bounded document batches and graph upserts, validation, final readiness manifest | Mixed graph/document atomic updates [#3486](https://github.com/stratalab/strata-core/issues/3486) |
| `tools/extract_addresses.py` | Offline spatial candidates and address-specific attachment policy | Spatial retrieval [#3458](https://github.com/stratalab/strata-core/issues/3458); NYC policy stays application-owned |
| `World::address_stations`, `address_discover`, `address_impact` | Native Strata SSSP followed by address/station membership joins and filtering | Bounded/multi-source algorithms [#3484](https://github.com/stratalab/strata-core/issues/3484) |
| `src/addresses.rs::explore` | Two-hop traversal using real versioned Strata neighbor calls; ≤100 output nodes | Efficient neighbor pages [#3489](https://github.com/stratalab/strata-core/issues/3489) |

The address graph has typed address, street, building, road-anchor, and place-reference nodes. Relationships are `on_street`, `in_building`, `near_road`, and `at_address`. Road/place references join separate graphs in the application. Semantic relationships cannot create route shortcuts. Native engine SSSP remains distinct from application Dijkstra used to draw paths.

Import batches use 256 JSON documents and 512 graph entries, with nodes before edges. Import validates counts, documents, and graph memberships, then writes `ready:addresses-v1`. Partial staging is replayed by deterministic upserts; recovery never deletes the graph. Existing scenarios upgrade additively; the readiness marker participates in their effective current version. Historical reads before address publication return `failed_precondition.island.address_version`. This release has one immutable address revision and no interactive address edits; sharing a catalog on a newly forked scenario is safe because the fork inherits the same immutable records. Reopen hydrates each branch's persisted state.

Hydration builds and then discards a full relationship snapshot to verify memberships. Interactive relationship exploration uses bounded storage neighbor calls, not a permanent full adjacency cache. The engine currently hydrates a hub's whole adjacency before returning its page, so the output limit does not bound internal storage work (#3489). No historical address cache is retained. Search uses four admission slots independently of the existing two-active/eight-queued graph workload limits.

Closure impact uses two full engine distance arrays and joins them once to the address catalog. It never performs one shortest-path query per address and never treats a UI radius cutoff as unreachable. Its categories distinguish unconnected, already unreachable, newly unreachable/reachable, farther/closer, and unchanged. It compares the original scenario-parent street version with the current scenario. For scenarios predating addresses, the newly published immutable address membership is applied to both road versions and the response names both road and catalog versions.

Creation accepts an optional `request_id` on `/api/scenarios` and `/api/close`. A same-JSON batch durably reserves the scenario number and retry token; successful repeated requests return the same live scenario. Reusing a token with a different closure or for a completed/archived scenario is refused. A partially failed creation that has written its intent but has not published a live scenario may require reopening the server for recovery; it never silently creates a second fork. The UI preserves an outstanding retry token in session storage and shows progress during the blocking storage operation, which runs off the async request thread.

## HTTP surface

- `GET /api/search?q=...&branch=city&version=...&category=...&limit=8&cursor=...`: unified summaries; category is optional (`address`, `landmark`, `park`, `transit`). Maximum 20; query maximum 200 bytes. Exact totals are omitted if candidate/term limits truncate work. Cursors bind query, branch, version, filter, and ranking policy.
- `GET /api/addresses/{id}?branch=...&version=...`: versioned record, provenance, binding, and persisted relationships.
- `POST /api/route`: existing endpoint tokens/coordinates plus address IDs and an optional expected `version`. The response preserves `length_m` and adds explicit network/approach metadata for addresses. Mutation coordination prevents a mixed route/publication view.
- `POST /api/address-explore`: `seed`, optional branch/version and limit; fixed two-hop traversal with real edge direction, deduplicated edges, and explicit truncation.
- `POST /api/addresses/{id}/nearest-stations`: optional branch/version; five nearest connected stations by outgoing network distance.
- `POST /api/address-discover`: origin, optional branch/version/max_m (≤10,000); counts plus the nearest 100 addresses and explicit truncation. Unreachable, unconnected, and outside-radius counts are separate.
- `POST /api/scenarios/{id}/address-impact`: origin, optional expected version, status filter, limit (≤100), and cursor; aggregate counts plus paged results.

`/api/places` is unchanged. `/api/meta` includes address catalog/count/graph statistics and current branch versions. `/api/city` remains the street graph, with no full address payload.

## Verification and reproducible workloads

- `python3 tools/test_extract_addresses.py`: source accounting, suffix/fraction normalization, unique IDs, road-name/radius constraints, island exclusions, and BIN policy.
- `cargo test --test addresses`: query boundaries/aliases/ranges/cursors, historical bindings, durable reopen, graph limits, and five real process-termination/replay checkpoints. Run debug to enable the existing crash hooks.
- `cargo test --release --test places --test subway --test golden --test geo --test route --test persist -- --test-threads=1`: existing routes, catalogs, scenarios, branch admission, and durable recovery regressions.
- `tools/check_addresses.cjs`: address search/card/station/relationship/radius/endpoint flows, apartment feedback, and mobile/dark browser checks. Read-only against database state.
- `python3 tools/check_addresses_http.py`: disposable cache only; independent Dijkstra verifies station ordering and all affected addresses, retry idempotency, reopening, audit, and four-client search.
- `cargo run --release --example address_probe -- 1000`, then `10000`, `63103`, and `63103 --durable`: temporary storage/search/fork/reopen profiles. See [benchmark results](benchmarks/README.md#manhattan-addresses).

The original city/desk durable database was backed up consistently with its prior binary, migrated on a copy, and checked against the original branch/history records before live rollout. The live database was then upgraded in place: city and `desk-0001` retained their prior branch metadata and operation history, street counts remained 12,862/28,802, and the audit verified the scenario chain. Live address browser checks passed. The rollout backup is `/tmp/island-live-rollout-20260920` (outside version control). Existing legacy catalog crash tests remain intact; the new small address fixtures exercise the identical importer at document, node, edge, validation, and publication checkpoints without repeating a full 63k import per kill point.
