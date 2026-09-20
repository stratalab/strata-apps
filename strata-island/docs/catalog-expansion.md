# Catalog revision 2: 500 additional landmarks

This records the earlier 550-place revision. The current catalog includes [all 1,701 eligible places](catalog-all.md).

The V2 console now contains **550 destinations**: 523 landmarks, 14 parks and 13 transit destinations. The original 50 records, coordinates, road attachments and six compatibility aliases are unchanged. The road extract remains 12,862 nodes and 28,802 directed edges.

## Sources and selection

Pinned OSM tourism, historic and cultural/amenity queries are saved with gzip-compressed full geometry/metadata responses in `fixtures/places-source/candidates/`. `places-source/expansion.json` contains the selected 500 source objects and input hashes/timestamps. OSM attribution and each object's source link remain available in the console.

The offline selector filters to the pinned Manhattan borough boundary and supported map extent, rejects inactive features, named infrastructure, tiny memorials and duplicate representations, and favors knowledge identifiers and cultural feature classes. Round-robin 2.5 km latitude bands spread the selection from downtown to Inwood. This is an automated, reproducible selection, not a popularity ranking or a claim that every destination has been manually inspected. The 500 additions include museums, theaters, libraries, monuments, public art, historic districts and religious buildings.

`fixtures/places-selection.json` records acceptance/rejection counts and feature distribution. `fixtures/places-quality.json` records display geometry and attachment distances. There are 537 approximate/legacy road connections and 13 places without a road connection; unconnected places remain searchable and visible. A nearest road node is not a verified entrance or legal driving approach.

Regenerate without network access:

```sh
python3 tools/expand_places.py
python3 tools/extract_places.py
```

Both commands reproduce the checked-in artifacts byte for byte. Do not replace the frozen `places-catalog-v1.json`: its exact hash identifies the supported predecessor. Future source changes require a new catalog revision and migration.

## Graph and upgrade behavior

The semantic `places` graph grows to **985 nodes and 1,087 directed relationships**: 550 typed destinations, 432 shared road anchors and three categories. Each place has one typed category edge and, when connected, one typed road-anchor edge. SSSP uses the separate, unchanged road graph; relationship exploration uses Strata BFS and induced subgraphs.

Opening the existing V2 database upgrades the official city and each saved scenario additively. A new readiness document is published only after documents, typed nodes, bindings and relationships validate. Interrupted upgrades replay safely. The old readiness document and all historical graph/document versions remain intact. Historical queries read the place catalog and graph at the same version: pre-upgrade snapshots contain 50 places, current snapshots 550.

Scenario history exposes `current_version` independently of the last closure operation. Reopen/close requests use that current version, including a catalog-only upgrade; stale pre-upgrade mutation requests are rejected. Existing closures and audit events are preserved. A restart after a completed migration does not write another catalog revision.

The UI hydrates all version-pinned catalog pages (100 per request), uses bounded keyboard suggestions, and limits map marker/label density while preserving search access to every destination. Hit-testing only considers rendered markers.

## Checks and stress coverage

Integration tests cover catalog preservation, exact graph counts, directed-distance results against an independent router, closure impact, and migration of existing scenarios. Three additional abrupt-process termination cases interrupt an upgrade after documents, graph writes and readiness publication; recovery checks old 50-place history, the current 550-place catalog, reopening and a second restart.

`island-stress --profile curated-550` exercises the expanded real data; `curated-50` still imports the frozen predecessor for comparisons. Curated profiles now report semantic graph size, first/middle/last typed pages, semantic snapshot construction, and cached BFS plus induced subgraphs, alongside road analytics and branch storage workloads.

The expansion exposed an engine deletion limit, filed as [strata-core #3477](https://github.com/stratalab/strata-core/issues/3477). Import recovery now resumes deterministic writes instead of deleting and rebuilding the staging graph; it can also resume partially defined draft ontology. Existing frozen ontology and history remain intact. This workaround is exercised by the fresh-import crash cases, in addition to the additive-upgrade cases.

Verification completed: 40 Rust tests passed across the regression, catalog and recovery suites; Clippy passed with warnings denied. Browser checks covered the 550-place counter, selection of Carnegie Hall from catalog position 325, routing, relationship exploration, discovery, closure impact/reopening, themes and mobile layout. All six HTTP catalog pages contain 550 unique IDs at one version. The running durable demo was upgraded after a local backup; its original Port Authority–Grand Central route remains 1,464 m.
