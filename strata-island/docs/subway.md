# Manhattan subway layer

**Routing update:** [Car and Subway + walking](journeys.md) adds a separate pedestrian/train-pattern graph and typical weekday itineraries. The original topology layer described below remains available for exploration.

Catalog revision 4 adds **151 MTA stations** to the original 1,701 OSM places,
for **1,852 searchable locations**. The independent `subway` graph contains 151
nodes and 842 directed connections (ride edges by service and GTFS transfers).
There are 121 station complexes and 25 service IDs in the pinned feed. The
`places` graph now has 3,035 nodes and 3,666 edges; the street graph is unchanged.

## Use it

Run `cargo run --release -- --dataset v2`. Open **Layers → Subway network** to
toggle the overlay or select a service. Search a station under **Transit**; its
details show daytime services, station and complex identity, and source attribution.
Selecting a station opens a nonmodal place card and zooms the map to it. **Explore subway · 2 hops** finds stations
through Strata's outgoing BFS and induced subgraph. Each connection is one ride
between consecutive scheduled stops or one transfer; an express connection can
skip intermediate local stops. Selecting a result opens that station's details.

Stations with street anchors work in the existing route endpoints and discovery
features. Car routing and missions use the directed street graph; Subway + walking uses the separate journey graph.
Roosevelt Island and Marble Hill have subway graph connections but no street
anchor: the road extract covers Manhattan island, so we do not invent a street
connection across water or beyond that extract. All 151 stations remain visible
and searchable. There are 149 added road anchors (1,814 anchored destinations in
total), all labeled approximate. Station coordinates are not entrance locations.

## Sources and reproducibility

- [MTA Subway Stations](https://data.ny.gov/d/39hk-dx4f): downloaded station records,
  including the MTA station ID, GTFS stop IDs, complex ID, borough, coordinates,
  line names and daytime services.
- [MTA developer resources](https://www.mta.info/developers): regular subway GTFS
  from `https://rrgtfsfeeds.s3.amazonaws.com/gtfs_subway.zip`, including stops,
  routes, trips, stop sequences, and transfers. The pinned feed declares version
  `20260826-X-long-term-supplement-trip-ids`, effective 2026-05-26–2026-10-31.

Raw inputs, retrieval time, source URLs and SHA-256 hashes are in
`fixtures/subway-source/`. Regenerate offline with:

```sh
python3 tools/extract_subway.py
python3 tools/test_extract_subway.py
```

The existing `extract_places.py` workflow verifies that its OSM output exactly
matches frozen catalog v3, then regenerates the combined catalog with this importer.
Refreshing the raw inputs requires a new dataset/catalog revision, frozen prior
fixtures, and a tested migration; startup never downloads changing upstream data.

Filter by MTA borough `M`, which includes Roosevelt Island and Marble Hill.
153 Manhattan source rows resolve to 151 distinct MTA station IDs. West 4 St and
145 St each have two GTFS IDs; merge those records into one location per station.
Do not merge distinct stations simply because they share a name or complex.
Place identity is `p:mta:station:<station_id>`; metadata retains all parent GTFS IDs
and complex membership. Existing OSM terminals/landmarks remain separate original
places, preserving their historical identity and aliases.

Each GTFS trip is sorted by numeric stop sequence. Form consecutive pairs across
the **complete** trip, then keep pairs whose endpoints resolve to Manhattan
stations. Filtering the stop list first would fabricate connections across
excluded boroughs. Direction comes from actual trips; reverse edges are not
invented. Self edges created by merging multiple GTFS IDs are omitted. Transfer
edges come only from explicit allowed GTFS transfer rows, without reverse-edge
inference or proximity-based transfers.

The graph is the union of services in the static feed, including different
calendar patterns. It does not assert simultaneous availability. The overlay
draws straight schematic connections between station coordinates, not tracks or
tunnels. Daytime service labels come from the station dataset; filtered network
membership comes from the GTFS edges, which also include other service patterns.
This topology endpoint has no live arrivals, service alerts, departure-time routing, accessibility routing, or fares. The separate journey adapter now supplies typical weekday estimates and transfer allowances.

## Strata integration and upgrades

Station JSON documents and typed `transit` place nodes use the existing semantic
ontology, road anchors, categories and branch-local bindings. The separate subway
graph binds its nodes to the same station documents, with `ride:<route_id>` and
`transfer` directed edges. Street closures modify only `manhattan`.

Catalogs v1–v3 remain frozen. An additive v4 import upserts documents and graph rows,
imports subway data, then publishes catalog readiness. Recovery replays upserts;
it never deletes a partially imported graph. Existing scenario branches upgrade
independently, retain street closures and event histories, and preserve old place
snapshots. New scenarios inherit the subway graph through the branch fork.

`GET /api/subway?branch=city` reads connectivity from a Strata adjacency snapshot
at the branch's published version. Pinned metadata supplies names, coordinates,
and colors. `POST /api/subway/explore` takes `seed`, optional `branch`, and `depth`
1–6 (default 2), and runs outgoing Strata BFS plus an induced subgraph. Responses
include version, station results, connection count, and separate snapshot and
algorithm timings. Both endpoints use the existing bounded graph-job admission.
Map pan/zoom and filtering use the hydrated browser snapshot.

Known engine limitations remain relevant:

- [#3471](https://github.com/stratalab/strata-core/issues/3471): SSSP cannot filter
  edge types. Separate graphs keep transit edges out of street-route distances.
- [#3456](https://github.com/stratalab/strata-core/issues/3456): SSSP lacks
  predecessor output; a future multimodal router still needs path reconstruction.
- [#3457](https://github.com/stratalab/strata-core/issues/3457): adjacency snapshots
  do not expose arbitrary edge properties; timetable metadata needs another read
  path if time-aware routing is added.
- [#3477](https://github.com/stratalab/strata-core/issues/3477): large durable graph
  deletion exceeds a commit limit; imports recover by deterministic upserts.

No new engine defect has been reproduced by this subway workload. The topology's
lack of timetable/platform-state routing is an explicit application scope choice,
not a claimed Strata defect. The typical journey adapter now supplies a dedicated pedestrian graph, coherent train-pattern state, and transfer costs. Departure-aware routing would additionally evaluate schedules at the requested time and require finer platform/entrance information; the driving graph never stands in for walking.

## Validation

`tests/subway.rs` compares every persisted edge with the pinned topology, checks
all station documents and bindings, independently verifies BFS reachability,
rejects invalid seeds/depths, and confirms street closures leave subway edges intact.
`tests/places.rs` checks catalog preservation, scenario operations and restart.
`tests/recovery.rs` exercises process termination during import (including subway
import), scenario operations and upgrades from catalogs v1, v2 and v3. Run recovery
in the default debug profile because its kill checkpoints use `debug_assertions`.
`tools/check_subway.cjs` covers service filtering, station details, BFS, unanchored
stations, street endpoints, and dark/mobile layouts in Chromium.
