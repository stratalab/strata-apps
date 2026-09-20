# Car and Subway + walking

Directions offers two modes. **Car** preserves the existing directed street routes
and closure comparisons. **Subway + walking** finds the lowest estimated-time
journey on a separate pedestrian and train-pattern graph, with colored train legs,
dotted walking legs, boarding/exit stations, headsigns, transfers, and clickable
steps. Short trips may recommend walking throughout. Addresses, landmarks, map
intersections, and all 151 subway stations work as inputs; pedestrian attachment
can fail explicitly if no connected path is within 150 m.

These are typical weekday estimates, not live departure instructions. The UI
states this alongside the itinerary. Walking is modeled at 1.35 m/s. Each train
boarding includes an assumed four-minute wait and two-minute access allowance;
exiting adds one minute. A station coordinate is not an entrance survey. Subway
geometry is schematic; walking geometry follows the extracted OSM ways. No fare,
accessibility, traffic ETA, temporary-service, or turn-restriction claims are made.
Car mode continues to report street distance rather than invented traffic times.

## Data and topology

`tools/fetch_walking.py` explicitly pins OSM highway ways within Manhattan's
administrative area, including Roosevelt Island and Marble Hill. Its source URL,
query, retrieval/OSM timestamps, and SHA256 are in `fixtures/walking-source/`.
Startup never downloads changing map data. `tools/extract_journeys.py` regenerates
`fixtures/journeys-v1.json` offline and verifies the raw OSM and GTFS hashes.

Walking uses a dedicated access policy: public pedestrian streets/paths/steps and
walkable roads; no motorways, trunks, private/no-foot ways, indoor paths, areas,
construction, or cycle-only ways. Car one-way tags do not restrict walking;
`oneway:foot` does. Both endpoints must lie in the pinned Manhattan boundary.
Components smaller than 100 nodes are excluded. Degree-two chains collapse with
full geometry and length retained; name/direction changes and regularly sampled
vertices remain for attachment. Station approaches are limited to 150 m and are
explicitly approximate. The source has **65,133 retained walking vertices**, and
all **151 stations** have a pedestrian approach, independently of car access.

Train patterns come from the existing pinned MTA GTFS, with calendar and date
exceptions evaluated for **Monday 2026-09-21**. Trips originating between 10:00
and 16:00 New York time are grouped by service, headsign, full stop sequence, and
pickup/drop-off policy. Manhattan segments are split at excluded-borough stops;
filtering never invents a direct connection across an omitted borough. Median
arrival-to-arrival elapsed times include intermediate dwell. There are **56
patterns**. This models typical daytime service on that reference day, not a
query-time departure schedule or the old all-calendar topology union.

Each pattern position has its own on-train node. Riding preserves that pattern;
changing trains requires alighting and paying a new boarding allowance. Explicit
GTFS transfers use their minimum time (or a conservative default). Ordinary
pedestrian routes can also connect stations. Distances on ride legs are schematic
station-to-station distances, not track mileage; the product emphasizes duration
and walking distance. Car closures leave this independent network unchanged.
Pedestrian closures or transit disruptions would require their own operation type.

## Strata integration and replacement points

The new `journeys_v1` graph has **66,007 nodes / 201,567 directed edges**. Edge
weights are integer seconds. Types are `walk`, `access`, `board`, `ride`, `alight`,
and `transfer`; graph properties persist source geometry and itinerary metadata.
City totals across all five graphs are **194,617 nodes / 421,077 edges** before
closures. The original street, place, subway-topology, and address graphs remain
unchanged.

Import stages use deterministic 512-entry upserts, validate graph counts, and
publish `ready:journeys-v1` last. The marker's version participates in effective
branch versions and scenario forks. Existing branches upgrade additively without
rewriting their events. Replays never call `delete_graph`. Startup hydrates a
versioned Strata adjacency snapshot and validates every edge/weight against the
hashed fixture. Immutable geometry and labels are shared from that fixture;
route connectivity and weights come from the persisted graph. The city snapshot
is safely shared for scenario requests because current operations modify only the
car graph; any future pedestrian/transit operation must invalidate this assumption.

`src/journeys.rs` isolates import, hydration, attachment, and route assembly.
Native Strata outgoing SSSP computes each journey cost. Application Dijkstra
reconstructs predecessors and legs, and its result must exactly equal the native
cost. Replace that reconstruction when [#3456](https://github.com/stratalab/strata-core/issues/3456)
exposes paths. [#3457](https://github.com/stratalab/strata-core/issues/3457) tracks
edge-property hydration; [#3458](https://github.com/stratalab/strata-core/issues/3458)
tracks the spatial lookup currently served by a small application grid.
[#3484](https://github.com/stratalab/strata-core/issues/3484) covers bounded/weighted
multi-source graph algorithms useful for evaluating multiple access candidates.
The static cost model and GTFS interpretation remain product policy. No distinct
new engine failure was reproduced; existing tickets cover these fallbacks.

`POST /api/route` adds optional `mode: "car" | "transit"` (default `car`). Existing
from/to tokens, coordinates, branch, and expected version are retained. Transit
responses add `legs`, `duration_s`, `walking_m`, `boardings`, `transfers`, reference
date, assumptions, and native algorithm timing. Invalid modes return 400; stale
versions return 412. Both modes run off the HTTP thread under the existing bounded
analysis admission. Missions explicitly retain car routing.

## Verification

- `python3 tools/test_extract_journeys.py`: pedestrian access policy, source
  accounting, pattern continuity, and station attachment.
- `cargo test --test journeys`: one-way service, transfer penalties, walking
  fallback, approximate approaches, zero-length trips, missing coverage, native
  distance agreement, and three real process-termination/replay checkpoints.
- `python3 tools/check_journeys_http.py`: read-only independent Python Dijkstra
  on the complete fixture, bidirectional and transfer trips, closure isolation,
  default-car compatibility, invalid modes, and stale versions.
- `tools/check_journeys.cjs`: both UI modes, train itineraries, stations without
  car anchors, addresses, mode switching, per-step camera, and mobile/dark views.
- Existing address, map UX, car route, persistence, and full mission/closure browser regressions passed. The mission regression uses an explicitly named disposable durable copy for this larger graph.

Use `ISLAND_URL` for HTTP/browser probes; their default is a separate server on
port 7454. Browser probes also accept `ISLAND_PLAYWRIGHT_PATH` and
`ISLAND_CHROMIUM`. The live database was backed up with its previous binary at `/tmp/island-before-journeys-20260920`, then upgraded in place. Branches, operation history, and the event-chain audit were preserved; live journey browser checks passed.

A disposable durable migration preserved the original operation history, reopened successfully, and passed scenario creation/retry/inherited-transit/audit checks (one creation sample: 1.51 s). See [recorded measurements](benchmarks/README.md#multimodal-journeys).

The full cache-mode city still exhibits slow graph-heavy forks (#3475); browser regression requests allow 120 seconds for that known engine workload. The mission regression can also run against an explicitly named disposable durable copy by setting `ISLAND_TEST_DB=/tmp/island-...` to the exact server database path; it refuses other durable databases. Durable and cache timings must not be conflated.
