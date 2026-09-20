# strata-island

A Manhattan-island drive map on [StrataDB](https://stratadb.org).

> I closed 42nd Street. The official city still routes through it. I compared the two paths.

Standalone app in this repo. It takes `stratadb` from the published `v1.2.3` tag — `cargo run` in this folder does not need a `strata-core` checkout. To build against a local engine, add a `[patch]` in an untracked `.cargo/config.toml`; do not edit `Cargo.toml`.

```bash
cargo test
cargo run --release -- --dataset v2
# http://127.0.0.1:7450
```

Requires Rust 1.91 or newer. The repository includes pinned seed fixtures;
`build.rs` decompresses the larger address and journey fixtures into Cargo's
build directory. No map-data download or existing database is needed to build
and run. The first V2 launch imports the seeds into a local database and can
take several minutes. Database directories, build output, raw source downloads,
local configuration, and uncompressed copies of the large fixtures are ignored.

For Python fixture checks or HTTP verification tools, first expand the two
compressed seeds (Python 3, standard library only):

```bash
python3 - <<'PY'
import gzip
from pathlib import Path
for path in Path("fixtures").glob("*.json.gz"):
    path.with_suffix("").write_bytes(gzip.decompress(path.read_bytes()))
PY
```

When intentionally updating a large fixture with the extraction tools, repack
its `.json.gz` using `gzip.compress(data, compresslevel=9, mtime=0)` before
building or committing. The compressed seeds are the build inputs; source
downloads and database files are never committed.

V2 uses `./island-db-v2`, with 1,852 named destinations (1,701 pinned OSM destinations and all 151 Manhattan MTA subway stations), plus 63,103 NYC address points, graph discovery, and closure history. The CLI still defaults to V1 (`./island-db`) when `--dataset` is omitted. Existing V1 data is preserved. Durable writes sync before acknowledgement; one process owns each database directory.

The map fills the screen. Choose **Explore**, **Directions**, **Missions**, or **Closures** in the floating panel. Search for an address, landmark, park, or station in Explore; selecting it zooms the map and opens a place card. Back returns to the previous place or search results. Use **Reachable from start** to find destinations by Strata network distance, open **Explore relationships** for typed graph connections, and **Compare destination access** after creating a closure. Select street sections on the map or search by street name, preview your selection, and create a named closure. Undo/clear edits, reopen the corridor, and explore before/after versions. The current route and original 42nd Street corridor remain available as presets.

Open **Missions → Landmark Passport** for three exploration missions: Midtown icons, a museum circuit, and downtown green spaces. Strata chooses the next reachable stop by road distance; animated simulated journeys collect visits and badges saved in your browser. Closures can block mission progress until you restore access.

Use **Layers → Subway network** to toggle the schematic network or filter a service. Search a station, open its details, and choose **Explore subway · 2 hops** to traverse ride and transfer connections with Strata BFS. Stations also work as street-route destinations where a road anchor exists. Choose **Car** or **Subway + walking** in Directions. Subway trips include walking legs, named services, transfers, and typical weekday estimates without live arrivals. See [routing data and assumptions](docs/journeys.md). See [subway data and graph model](docs/subway.md).

Interaction details: [Map-centered navigation](docs/map-ux.md).

Implementation and API details: [Curated graph expansion](docs/v2-implementation.md). Stress commands and measurements: [Graph benchmarks](docs/benchmarks/README.md).

`CITY_VERSION = 1` is frozen.

## Original V1 talk script (~90 s)

1. **Run** `cargo run --release`. The navigation console opens on midtown with the first route already drawn. Camera never calls `graph()`.
2. **Pick** Port Authority → Grand Central (already selected). **Find route.** The blue route uses West 42nd between 5th and 8th (`used_closed: true`).
3. **Create closure scenario.** `fork_at_version` + `delete_edge` on `desk-0001`. Parent RAM is not rebuilt.
4. Blue still goes through 42nd. Orange detours. The sidebar shows both distances and the added distance. Switch between saved scenarios with the scenario picker.
5. **Compare changes.** `graph_entities > 0`. There is no Promote — graph adapters refuse promotion. **Archive** removes the selected scenario.
6. **System → Run audit.** `graph_info` on `city` matches the RAM snapshot; `verify_chain` on each desk.

That is the demo. V1 stops here.

The console includes light/dark themes, an island overview, landmark and street-label toggles, keyboard pan/zoom, and touch pan/pinch. Click two intersections for a custom route, or use the named-place pickers. Coastlines and parks are a simplified schematic backdrop; streets and route distances come from the frozen extract.

## What this is not

- A schematic map: no tileserver, MapLibre, or live OSM updates.
- **Not a legal Manhattan drive.** Directed shortest path on a node/edge graph. No turn bans, no no-left, no U-turn expansion.
- Not promote. Compare is the landing.
- Search covers pinned Manhattan places and addresses, without global geocoding or apartment-level matches. V1 retains six aliases.

## License

Map data © OpenStreetMap contributors, [ODbL](https://www.openstreetmap.org/copyright). Subway station records and schedule data are from [MTA Open Data and NYCT GTFS](https://www.mta.info/developers). Address points are from [NYC OTI / NYC Open Data](https://data.cityofnewyork.us/City-Government/AddressPoint/uf93-f8nk). The interface credits these sources.

Design and future workloads: [Named places and graph workloads](docs/graph-expansion-plan.md).

Address search: [63,103 Manhattan addresses](docs/addresses.md), with map cards, street routing, nearby stations, graph exploration, and closure impact. [Implementation plan and native Strata replacements](docs/address-search-plan.md).

V1 plan: [docs/implementation-plan.md](docs/implementation-plan.md). Engine friction: [docs/friction.md](docs/friction.md).
