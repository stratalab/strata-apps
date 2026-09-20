# Regenerating the Manhattan drive extract

Runtime never hits OSM. `fixtures/manhattan-drive.json` is the golden.

## Recipe (frozen)

See `docs/implementation-plan.md`. Filters do not move to hunt a count band.

```sh
python3 tools/extract_manhattan.py
```

Uses Overpass (`overpass-api.de`, then mirrors) for the frozen WGS84 bbox and highway allow-list. Equivalent to clipping Geofabrik `new-york` to that bbox and keeping the same tags.

After a regenerate, record in `src/extract.rs`:

- `EXTRACT_NODE_COUNT`
- `EXTRACT_EDGE_COUNT`
- `EXTRACT_BYTES`
- `EXTRACT_FNV1A64` (`fnv1a64` of the file bytes)
- `CLOSURE_EDGE_COUNT`

Then freeze. Do not loosen the allow-list to hit 4k–8k. PR2 recorded **12862** nodes / **28802** directed edges / **8** closed 42nd triples.

## Pins

Talk pair is pinned after collapse (PR3: parent path must use a closed 42nd triple; do not retune weights):

- `poi:port-authority` → `n:42435663` (8th & 42nd, west end of the closed corridor)
- `poi:grand-central` → `n:9140654137` (Park & 42nd)

Post-collapse snap is 200 m to the nearest *kept* intersection (collapse can move the vertex off the WGS84 pin). Keep-near-pin during collapse stays 40 m.

## License

© OpenStreetMap contributors (ODbL). Footer on the plat.
