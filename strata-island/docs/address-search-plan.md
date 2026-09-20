# Manhattan address search and graph workloads

**Status:** implemented 2026-09-20; see [implementation, APIs, validation, and replacement adapters](addresses.md). The original design below is retained for context.\
**Date:** 2026-09-19.\
**Engine:** published `stratadb v1.2.3`, commit `6fc481c33473efd7d1724284107b67be08625dcd`.\
**Depends on:** the existing map-centered UX, catalog v4, subway topology, and closure scenarios.\
**Related:** [map UX](map-ux.md), [subway model](subway.md), [graph expansion](graph-expansion-plan.md), [engine findings](friction.md).

## 1. Outcome and scope

Search for a Manhattan address, landmark, or station in one field. Selecting a result zooms the map and opens the existing nonmodal card, with Directions and From here actions. Addresses are also valid route endpoints. Then close a street and inspect which addresses become farther away or unreachable from a chosen origin.

Use NYC AddressPoint as the primary address source and the existing OSM data for landmark enrichment. Import Manhattan-wide coverage after explicit quality filtering; do not quietly cap the catalog at a convenient number. Keep addresses out of the always-visible landmark layer. Strata owns persisted records, relationships, versions, and scenarios; application search indexes and map rendering are derived from that state.

Required first delivery: reproducible import, address graph, unified search, cards, endpoint resolution, recovery, and scale measurements. Required follow-up delivery: address closure impact, nearest station by directed street distance, and a bounded relationship explorer. Both deliveries are part of this plan. Multi-stop delivery missions, bulk station catchment visualization, typo correction, live geocoding, transit journey planning, and legal walking/driving navigation are later extensions.

No implementation step requires upgrading Strata first: each missing API below has an explicit fallback. Reconsider fallbacks only after a published engine release passes the app's correctness and recovery checks.

## 2. Verified starting point

- Catalog v4: 1,852 destinations = 1,701 OSM places + 151 Manhattan MTA stations. Preserve their IDs, aliases, mission progress, and subway connections.
- Street graph: 12,862 nodes / 28,802 directed edges. Semantic places graph: 3,035 nodes / 3,666 edges. Subway graph: 151 nodes / 842 edges. These are separate graph records, not counts of unique physical locations.
- `src/main.rs::api_places` currently filters a hydrated array by name terms; it is not an address index. `src/places.rs::Place` has no address fields.
- Existing OSM source files contain house-number and street tags for 936 of the 1,701 places. This measures the selected source files, not OSM's overall address coverage. Preserve frozen fixtures; extract enrichment into a separate revisioned artifact.
- `fixtures/manhattan-drive.json` provides road endpoints, lengths, and names. It has no NYC street-code mapping or detailed entrance/curb geometry. Street attachment will remain an approximation.
- `src/places.rs::load_at` already joins typed graph nodes, historical documents, and relationships. Its per-record calls and full-index typed pagination must be measured before extending the pattern to tens of thousands of addresses.

### NYC source audit

The user's [map view](https://data.cityofnewyork.us/City-Government/AddressPoint/6xyb-j5pk) references the underlying [AddressPoint dataset `uf93-f8nk`](https://data.cityofnewyork.us/City-Government/AddressPoint/uf93-f8nk). Filter `boroughcode = '1'`; do not use the map view ID as the table API ID. [API documentation](https://dev.socrata.com/foundry/data.cityofnewyork.us/uf93-f8nk).

Read-only aggregate queries on 2026-09-19 returned:

| Observation | Count | Interpretation |
|---|---:|---|
| Manhattan source rows | 63,245 | Raw rows, not a promised accepted-address count |
| Distinct address-point IDs | 63,244 | At least one source identity collision requires resolution |
| Rows with geometry / house number / full street name | 63,245 each | Presence alone does not establish validity |
| Rows with house-number ranges | 3,106 | Do not expand into fabricated individual addresses |
| Distinct raw BIN values | 43,908 | Includes placeholder values; not an accepted building count |
| Distinct actual street codes | 873 | Validate missing/invalid codes and name aliases |

Reproduce with `/resource/uf93-f8nk.json` and this SoQL:

```sql
SELECT count(*) AS rows, count(distinct addresspointid) AS ids,
       count(distinct bin) AS bins, count(distinct b7sc_actual) AS streets,
       count(the_geom) AS geometry, count(house_number_range) AS ranges,
       count(house_number) AS house_numbers,
       count(full_street_name) AS streets_named
WHERE boroughcode = '1'
```

Duplicate ID `5217738` represents `270 E 2 ST` at identical coordinates in two rows, with BINs `1000000` and `1091925`; the latter has a later modified date. Preserve both raw rows and add a reviewed resolution record. Prefer the valid building reference only after confirming BIN placeholder semantics. Never rely on row order or last-write-wins.

The [attached NYC metadata PDF](https://data.cityofnewyork.us/api/views/uf93-f8nk/files/8e7624d3-c34c-4fe8-b049-6328fee361e8?download=true&filename=AddressPoint.pdf) describes points placed near street frontage inside building footprints. It does not establish that every point is a surveyed entrance. Its older field schema is supplemented by the official [ArcGIS layer's coded domains](https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/AddressPoint_view/FeatureServer/0?f=pjson): status 1 proposed, 2 permitted, 3 under construction, 4 built, 5 retired. Validation separately distinguishes delivery-point verification, field verification, unvalidated, not posted in the field, and NYC research. Pin both resources and verify their applicability to the downloaded table.

The status query returned 9,472 built, 7 proposed, 14 permitted, 115 under construction, 5 retired, and 53,632 null-status rows. Null is common and does not mean verified or inactive. Default search includes built and null-status records after other quality checks; retain null as unknown. Exclude retired and not-yet-built statuses from ordinary destination suggestions, accounting for them in the quality report. Unexpected future codes are quarantined pending an explicit policy revision. Validation is provenance, not proof of legal access.

## 3. Source acquisition and quality rules

Add `tools/fetch_addresses.py` for explicit network acquisition and `tools/extract_addresses.py` for deterministic offline normalization. Keep fetching out of server startup, UI interactions, tests, and benchmark runs.

1. Save table metadata, applicable dictionary, query, retrieval time, reported source update time, and raw rows under `fixtures/address-source/`. Order pages by a verified unique source-row key, not the nonunique address-point ID. Capture row counts and source update metadata before/after acquisition; discard and retry if they change. Prefer a single bounded export when supported. Hash the completed files.
2. Select the borough, validate point coordinates, required address components, field types, and source identifiers. Preserve address numbers as strings, including suffixes/hyphens. Preserve original fields alongside normalized values.
3. Apply the status policy above using pinned coded domains. Excluded rows remain in the source/quality report. Null-status addresses are searchable as unverified, with routing disabled unless attachment is independently acceptable. Do not treat every unvalidated source address as nonexistent, or field verification as proof of current entrance access.
4. Group by address-point identity and record every duplicate decision in `fixtures/address-resolutions.json`. Equivalent rows may collapse while retaining all source-row IDs. Conflicting coordinates/address strings are quarantined until resolved; display-address equality alone never merges distinct source identities.
5. Represent source ranges as ranges. An interior number must not be advertised as an exact recorded address. Range matching requires confirmed interval/parity semantics; otherwise only the recorded range label is searchable. Apartment/unit geocoding is out of scope and must not silently claim an exact unit match.
6. Preserve actual/vanity street codes, BIN, ZIP, source status/validation, and timestamps. Missing or placeholder BINs do not create a shared fictional building node. No BBL/tax-lot relationship is invented: that field was not present in the inspected table.
7. Produce `fixtures/addresses-v1.json`, a source manifest, OSM address-enrichment artifact, and `fixtures/addresses-quality.json` with accepted/rejected/quarantined totals and reasons. Every raw row must be accounted for. Regeneration from pinned inputs must be byte-identical.

Use Manhattan borough geography for search coverage. Addresses outside the existing main-island street extract can still be found and shown on the map, but remain unconnected. In particular, geographic proximity must not create links across water to Roosevelt Island or Marble Hill. Extend map fitting/display bounds as needed without expanding the street-routing claim.

### Street attachment

Build an offline spatial candidate index over named street segments. Normalize NYC street names and existing OSM names using explicit, tested aliases, preserving East/West distinctions. Prefer candidates on the address's street; do not use an unconstrained nearest-node fallback.

For the initial routing model, choose a road node incident to a matching street within the existing 200 m maximum. Use segment proximity, component diagnostics, stable tie-breaking, and a documented ambiguity threshold to select or reject candidates. Preserve the chosen street segment, candidate distances, reason/confidence, road fixture hash, and any curated override. Multiple plausible sides/streets, bridges, parks, campuses, and disconnected components go into the review set. Do not force every address to have an anchor.

This remains an approximate node attachment, not entrance routing or an along-edge virtual junction. Keep the street graph unchanged, use the same anchor in both directions, and show a dashed geometric approach connector separately from the road route. Report `network_distance_m` and `approach_distance_m` separately; approach length is not a verified traversable route. Exact segment splitting with direction-aware connectors is a later model revision and must preserve closure identity.

## 4. Records and graph model

Add `src/addresses.rs` and a separate graph `addresses_v1`. The existing `places` ontology is frozen; do not add address types to it or modify catalog v4 in place. Address imports have independent revision/readiness records. OSM enrichment links to existing places without rewriting their documents.

Each address document contains a namespaced stable ID (`a:nyc:<addresspointid>` after collision resolution), source-row identities, display and canonical address components, raw codes, geometry, optional valid BIN/ZIP, range data, aliases, provenance, and explicit attachment status. Store source hash, normalization-policy revision, and attachment-policy revision in the catalog manifest.

| Node type | Identity | Relationship |
|---|---|---|
| `address` | Resolved NYC address-point ID | `on_street` → street; optional `in_building` → building; optional `near_road` → anchor |
| `street` | Borough + valid actual street code; deterministic named fallback when missing | Groups related addresses; preserve multiple source names as aliases |
| `building` | Valid NYC BIN | Incoming `in_building` exposes multiple addresses for one building |
| `anchor` | Existing road node ID with a local namespace | Carries a stable reference to the separate `manhattan` graph |
| `place_ref` | Existing OSM/MTA place ID | Optional `at_address` → address when evidence supports the association |

Define concrete endpoint types and freeze this ontology before import. JSON bindings remain branch-local. `place_ref` and road-anchor references are explicit application joins across graphs; do not call them native cross-graph edges. Only create landmark/address associations from matching normalized source address plus consistent geometry or a reviewed override. Nearest-point proximity alone is insufficient. MTA station coordinates are not street entrance addresses.

For accepted counts A addresses, B valid buildings, S streets, R used road anchors, and P linked place references, expected graph nodes are A+B+S+R+P. Edges are A street links plus actual building, road, and place/address links. Report actual counts rather than treating all 63,245 source rows as distinct nodes. This should exceed 100k semantic nodes if most raw building identities survive validation, but that is a planning estimate, not a measured import result.

Hydrate compact query indexes and anchor memberships from persisted state at a published version. Source fixtures seed the database; request handlers must not bypass persisted state by reading fixture relationships. Search-only users do not need a full address adjacency snapshot loaded. Construct graph snapshots lazily for graph features with explicit budgets and cache accounting.

## 5. Search, endpoints, and map behavior

Add `src/search.rs` with an immutable index over published addresses and existing destinations. On the pinned engine, use an exact composite lookup plus sorted token dictionaries/posting lists in application RAM, hydrated from Strata. Avoid scanning 63k full JSON objects or allocating lowercase strings on every keystroke. Keep the adapter boundary clear so engine search can replace it later.

Normalization handles case, Unicode normalization, repeated spaces, punctuation, conventional street suffixes, numeric/ordinal avenue aliases, and optional Manhattan/New York/NY/ZIP components. Keep directionals and house-number suffixes significant. Normalize aliases in both queries and documents. Do not fuzz house numbers; general spelling correction is deferred.

Rank exact canonical addresses first, then exact aliases/place names, then token-prefix matches with deterministic kind/name/ID tie-breaks. A geographic bias may break otherwise equal matches but must not override a conflicting house number or street directional. Place categories remain unchanged. Suppress duplicate suggestions when a confidently linked landmark and address describe the same choice, while retaining the address as a searchable alias and detail.

- Empty search retains featured destinations. Require enough text for prefix lookup; numeric-only or very broad input may prompt for a street instead of suggesting a misleading match.
- Return 8 suggestions by default, maximum 20. Cap query length at 200 bytes and bound prefix expansion/candidate work. Responses state truncation when a work budget is reached; do not claim an exact total from truncated candidates.
- Filter constraints before final top-k. Cursor identity includes normalized query, filters, ranking revision, branch, and publication version. Reject mismatched/stale cursors; do not append mixed-version pages.
- Debounce input around 150 ms; abort superseded requests and discard stale responses even if cancellation arrives too late. Keyboard selection, focus restoration, touch targets, and screen-reader combobox announcements are required.
- Selecting a result uses the existing zoom/card/back-stack behavior. Show source/quality/attachment details under About this place. An unconnected address remains selectable but Directions explains why no route is available.
- Keep all addresses out of `/api/city`, the normal landmark marker collection, and initial browser hydration. Draw selected addresses and bounded query/impact results only. Preserve the mobile map viewport and nonmodal card.

### API contract

These are proposed app endpoints, not current Strata APIs:

| Endpoint/change | Contract |
|---|---|
| `GET /api/search?q=&branch=&version=&kinds=&cursor=&limit=` | Unified address/place/station summaries, stable IDs, kind, coordinates, attachment summary, publication version, next cursor, truncation, and search backend metadata |
| `GET /api/addresses/{id}?branch=&version=` | Versioned address document, graph relationships, source/quality details, explicit unavailable-at-version result |
| `POST /api/route` | Extend endpoint resolution to address IDs while retaining existing place aliases/node tokens; reject unknown/unconnected IDs and keep origin/destination metadata in the response |
| `POST /api/address-explore` | Typed/directional, bounded graph relationships; depth ≤2, ≤100 returned nodes, explicit truncation |
| `POST /api/scenarios/{id}/address-impact` | Origin, matching catalog revision, before/after versions, status filter, stable page cursor/limit; aggregate counts plus paged details |
| `POST /api/addresses/{id}/nearest-stations` | Up to 5 stations ranked by outgoing directed street-network distance; approach distances and unavailable attachments kept separate |

Keep `/api/places` backward compatible. Centralize endpoint resolution rather than adding address-specific branches throughout `route.rs`. A result token's version is validated on selection/routing; stale state is refreshed explicitly. Every composite response names its branch and effective version.

## 6. Features that exercise the graph

| Feature | Required Strata work | Application work / correctness rule |
|---|---|---|
| Address card | Versioned node lookup, binding resolution, typed neighbors | Display the persisted document and attachment evidence |
| Other addresses in this building / on this street | Incoming typed neighbors with bounded pages; optional BFS/subgraph | Do not collapse a building into one address; expose high-degree truncation |
| Nearest station for one address | Outgoing engine SSSP on the street snapshot | Join eligible station anchors; rank network distance, not straight-line distance; no subway ride edges |
| Addresses within a distance radius | Engine SSSP; address-to-anchor graph membership | Filter distances after SSSP on the current API; distinguish outside-radius from unreachable |
| Closure impact | Existing branch fork and exact edge mutations; SSSP before and after; optional WCC and branch compare | Join distances to persisted address anchors, then aggregate/page; never run SSSP once per address |
| Scenario/history inspection | Same-version graph/document reads, historical snapshots and branch comparison | Parent remains unchanged; recorded versions reproduce the result after restart |

Closure impact must classify unchanged, farther, closer, newly unreachable, newly reachable, already unreachable, and unconnected separately. Do not reuse the current place-impact helper's 100 km result cutoff as an unreachable test; use the full SSSP distance arrays. Weak components are diagnostics, not proof of directed reachability. Capture the scenario's actual comparison baseline rather than always assuming the latest `city` branch state.

The address graph supplies memberships; shortest-path computation still runs on the isolated road graph because weighted edge-type filtering is missing. Persisted semantic edges must never become transport shortcuts. Display the directed street-model limitation and separate connector distances consistently across search, routes, and impact.

For a future bulk station catchment, run incoming SSSP once per distinct eligible station anchor and take per-address minima, or adopt a tested engine multi-source API. Keep this explicit benchmark/extension out of initial request handlers. Shared station anchors require deterministic station-ID ties; station coverage outside the road extract stays unavailable.

## 7. Publication, upgrades, and recovery

Use immutable catalog revisions with a final `ready:addresses-v1` manifest. Record source/normalized hashes, graph name, schema/policy versions, accepted and rejected counts, node/edge counts, and validation digest. Derive the acknowledged publication version from the manifest write outcome rather than predicting a commit number.

1. Import into a disposable durable database first. Define ontology; batch documents with `batch_set_or_create`; import nodes before edges with deterministic upserts and bounded chunks. Check every batch result. Tune chunk sizes against mutation limits; do not assume 512 graph entries imply a universal safe row count for all indexes.
2. Verify graph/doc/binding/attachment identity and fixture digests. Replay incomplete staging deterministically on reopen. Do not call `delete_graph` as recovery cleanup.
3. Write the manifest last, then build and atomically install an immutable query snapshot keyed by branch + publication version + policy revision. A process stopping after manifest publication simply rebuilds the derived index on restart.
4. Centralize effective version selection in `World`: current reads use the maximum acknowledged places/subway/address/scenario publication relevant to the response and read every component at that one retained version. Explicit historical reads use exactly the requested version. Versions before address publication return address-catalog-unavailable, not present-day addresses.
5. Upgrade existing active scenarios additively under the mutation coordinator, preserving road closures, operation history, event chains, and old readiness records. New scenarios inherit addresses through the normal fork. Do not share branch-specific mutable caches; identical immutable catalog data may be shared only when identity/hash equality is established.
6. Serialize migration/publication with scenario writes. Publish each branch atomically to app readers, and serve a controlled loading/error state if its address revision is unavailable. No fallback to another branch's latest documents.

Keep the old places v4 fixture byte-identical. If implementation discovers a necessary place-schema change, freeze v4 separately and design a v5 migration before modifying current inputs. Address-only enrichment should avoid that migration.

Before rollout, stop the writer briefly to create a consistent backup of the durable database plus the current binary; preserve the existing `desk-0001` scenario. Measure migration on a copy. Restart with the new binary only after copied-database checks pass. Verify branch histories, closures, subway counts, address counts, and route results. Roll back by restoring the paired binary/database backup, not by running an older binary over a partially upgraded database. This planning task does not perform the rollout.

## 8. Strata surface gaps and tracking

New reports were filed for the address workload. Existing island findings are linked where they already capture the same missing surface. Related older engine proposals are linked inside new reports for engine-team consolidation. These are source-verified capability findings, not invented benchmark failures.

| Missing/helpful surface | Issue | Plan response |
|---|---|---|
| Usable indexed JSON equality/range/compound field queries | **New [#3482](https://github.com/stratalab/strata-core/issues/3482)**; related #2703, #2206/#1891 | Application exact/posting index; do not create indexes with no usable read path |
| Public embedded token-prefix search/autocomplete with bounded work | **New [#3483](https://github.com/stratalab/strata-core/issues/3483)**; related #2252, #2249, #3017 | Application prefix index; typo matching remains later |
| Bounded and weighted multi-source shortest paths, owner labels, cancellation | **New [#3484](https://github.com/stratalab/strata-core/issues/3484)** | Full single-source SSSP + filtering; repeated incoming SSSP only for explicit catchment workloads |
| Historical batch JSON/graph/binding hydration | **New [#3485](https://github.com/stratalab/strata-core/issues/3485)** | Explicit per-record historical reads, bounded immutable caches |
| Atomic graph + bound JSON + optional KV updates | **New [#3486](https://github.com/stratalab/strata-core/issues/3486)**; related #3127 | Staging, deterministic replay, final publication manifest |
| Spatial bbox/nearest queries and efficient graph enumeration | [#3458](https://github.com/stratalab/strata-core/issues/3458) | Offline segment index, compact map/query snapshots |
| Weighted edge-type filtering | [#3471](https://github.com/stratalab/strata-core/issues/3471) | Separate semantic/street/subway graphs |
| Shortest-path predecessor/path result | [#3456](https://github.com/stratalab/strata-core/issues/3456) | Existing application Dijkstra for drawing routes; compare distances with engine SSSP |
| Bulk edge/property access in adjacency | [#3457](https://github.com/stratalab/strata-core/issues/3457) | Versioned relationship reads and validated road-source joins |
| Graph pagination, metadata, and small-batch cost scale with full graph | [#3473](https://github.com/stratalab/strata-core/issues/3473), [#3474](https://github.com/stratalab/strata-core/issues/3474), [#3472](https://github.com/stratalab/strata-core/issues/3472) | Separate cold storage measurements from caches; no graph_info on every search |
| Large graph fork latency and graph deletion limits | [#3475](https://github.com/stratalab/strata-core/issues/3475), [#3477](https://github.com/stratalab/strata-core/issues/3477) | Benchmark full address forks; keep scenario admission bounded; replay imports |
| Chunked graph import and cross-branch binding limitations | [#3464](https://github.com/stratalab/strata-core/issues/3464), [#3466](https://github.com/stratalab/strata-core/issues/3466) | Branch-local bindings, explicit publication protocol |
| Repeated negative-edge scans and mutable graph read handles | [#3460](https://github.com/stratalab/strata-core/issues/3460), [#3459](https://github.com/stratalab/strata-core/issues/3459) | Cache snapshots, run algorithms off the DB lock, measure snapshot/algorithm costs separately |

API evidence: pinned [Database capabilities](https://github.com/stratalab/strata-core/blob/6fc481c33473efd7d1724284107b67be08625dcd/crates/engine/src/api/database.rs#L289), [JSON index creation](https://github.com/stratalab/strata-core/blob/6fc481c33473efd7d1724284107b67be08625dcd/crates/engine/src/data/json/service.rs#L686), [latest-only batch_get](https://github.com/stratalab/strata-core/blob/6fc481c33473efd7d1724284107b67be08625dcd/crates/engine/src/data/json/service.rs#L280), [graph service](https://github.com/stratalab/strata-core/blob/6fc481c33473efd7d1724284107b67be08625dcd/crates/engine/src/data/graph/service.rs), and [SSSP](https://github.com/stratalab/strata-core/blob/6fc481c33473efd7d1724284107b67be08625dcd/crates/engine/src/data/graph/analytics.rs#L234).

The four service/API files above are byte-identical in inspected main commit `f74c95cc6b542739148156c66db874d8c0074a73`; main was not built or runtime-tested. Feature feasibility does not rely on old issue titles or undocumented internals. NYC normalization, source conflicts, and attachment uncertainty are application/data responsibilities, not engine defects. Further distinct engine findings require a pinned version, minimal reproduction/source evidence, exact error when applicable, expected contract, workaround, and a linked issue before being called resolved.

## 9. Delivery sequence and acceptance gates

| Milestone | Files / work | Exit gate |
|---|---|---|
| M1: pinned source and quality | Fetch/extract tools, address source/manifest/quality/resolution fixtures, normalization and attachment helpers | Status/range/BIN policy documented; duplicate resolved; every row accounted for; deterministic regeneration; sampled map review of ambiguous attachments |
| M2: storage and publication | `src/addresses.rs`, `src/lib.rs`, `src/world.rs`, readiness/recovery integration | Fresh and existing-scenario imports survive reopen/crash checkpoints; historical versions coherent; old fixture/road/subway digests and scenario histories preserved |
| M3: unified search and card | `src/search.rs`, `src/main.rs`, endpoint resolution in `src/world.rs`, `static/app.js`, `static/index.html`, `static/style.css` | Address/name/station queries work in Explore and Directions; no full-address browser payload; keyboard/mobile/card regressions pass; unconnected/range/stale cases explicit |
| M4: graph interactions | Address explorer, station-distance and impact handlers; adapt shared distance/impact helpers | Directed graph oracle agrees; graph relationships come from persisted state; result pages/aggregate statuses complete and stable |
| M5: scale and rollout | `src/bin/island-stress.rs`, new benchmark profiles, HTTP/browser checks, documentation | Cold/warm/cache/durable/concurrent results recorded; copied durable migration verified; paired backup and live smoke/audit complete |

M1 precedes M2; M2 publication contract precedes M3 API/UI. M4 uses M2 snapshots and M3 endpoint resolution. Start scale probes during M2 rather than discovering fork/import cost only at rollout. Register runtime findings in `src/findings.rs` when the corresponding fallback is implemented; the planning findings are recorded now in this document and `docs/friction.md`.

### Required validation

- **Data/search:** exact addresses, avenue ordinal aliases, East/West ambiguity, numeric prefix boundaries, suffixes/hyphens, punctuation/Unicode, ZIP constraints, missing fields, ranges, inactive/unknown status, duplicate identity, equivalent display labels, and no invented apartment match. Test golden queries against the accepted pinned catalog and an independent brute-force normalization/search oracle on small datasets.
- **Attachment/graph:** wrong-side streets, cross-water candidates, islands outside coverage, disconnected components, shared anchors, one-way edges, multiple addresses per building, and multiple stations per anchor. Searchability must survive lack of a route connection.
- **Impact/history:** independent small-graph Dijkstra checks; routes exceeding an arbitrary UI radius are not unreachable; compare the correct parent/version; stable pages across a pinned result; no scenario contamination; reopening reproduces counts and distances.
- **Recovery:** kill before/after document chunks, graph node/edge chunks, validation, ready manifest, cache publication, and a scenario upgrade. Use `Always` durability, deterministic replay, and verify no duplicate/stale membership links. Preserve the existing legacy catalog-upgrade recovery cases.
- **UI:** search races, keyboard/assistive navigation, card stack, map selection/zoom, unconnected addresses, route endpoints, closure controls, all existing missions, subway exploration, narrow mobile viewport, and light/dark themes. Mutating tests run on disposable databases; live checks remain read-only.

### Measurement and admission

Run address profiles at 1k, 10k, and the full accepted catalog; use synthetic larger graphs only in a separately labeled stress profile. Record machine, engine SHA, input hashes, durability, repetitions, and concurrency. Report import time, commit counts, file/WAL/allocated bytes, peak RSS, snapshot/index build time, cold restart, typed page cost, binding hydration, high-degree neighbor pages, BFS, SSSP, graph_info, fork, closure mutation, and archive behavior. Report node/edge counts by graph and branch separately from disk bytes.

Report search parsing/candidate/ranking/hydration time separately from HTTP and rendering. For graph requests, separate persistent snapshot reads, cached snapshot hits, engine algorithm time, membership joins, and serialization. The algorithms run on `GraphAdjacencyIndex` in memory; cached queries are not persistent graph reads.

Initial acceptance targets on the recorded development machine: warm search p95 ≤100 ms at four clients (excluding debounce), ≤250 ms for a warm card response, and ≤1 s for warm address-impact computation on the current street graph. These are targets to measure, not present performance claims. Do not adopt a startup/fork/RSS claim until the full address import is measured; if scenario creation exceeds 2 s, expose progress and keep it off the request/UI critical path with bounded job admission. Persist the operation result and prevent duplicate retry forks.

Retain the existing two-active/eight-queued graph-job limit and explicit busy responses; account separately for search work so autocomplete cannot starve closure jobs. Do not hold the database mutex while ranking or running graph algorithms. Bound historical snapshot caches by bytes as well as entry count, discard stale response work, and report that current SSSP itself cannot be interrupted early. No hidden full graph reads on pan/zoom.

Completion means both product deliveries work from persisted Strata state, accepted input counts are explainable, existing scenarios and history survive, performance evidence is checked in, and every designed-around engine gap has a linked report and a tested fallback.
