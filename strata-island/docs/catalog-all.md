# Catalog revision 3: all eligible pinned Manhattan landmarks

The catalog contains **1,701 places**: 1,674 landmarks, 14 parks and 13 transit destinations. This adds 1,151 destinations to the previous 550-place catalog; every previous record, identity, coordinate, alias and road attachment is unchanged.

## What “all” means

The three saved OSM tourism/historic/cultural queries returned 2,546 records, representing 2,491 distinct OSM objects in the query bounding box. This is a query-specific snapshot, not a census of all Manhattan features or all worldwide OSM landmarks. The definition includes museums, galleries, theaters, libraries, historic buildings, places of worship, public art and similar cultural features. OSM does not use one universal landmark classification: https://wiki.openstreetmap.org/wiki/Key:tourism and https://wiki.openstreetmap.org/wiki/Key:historic describe overlapping tags.

The existing selector excludes the original 50 records, then rejects inactive features, generic names, infrastructure, small memorial subfeatures, missing geometry, points outside the pinned Manhattan boundary/supported extent and duplicate site representations. It yields **1,651 additional eligible landmarks**, all now included. Adding the 23 original landmarks gives 1,674 landmarks; retaining 27 original parks/transit destinations gives 1,701 total places. No eligible candidates remain unselected.

The source queries include overlapping results, and the original curated 50 include destinations not in those queries. Do not subtract the final count directly from 2,491 to infer a rejection count. `fixtures/places-selection.json` records distinct input counts and filter outcomes; `fixtures/places-source/candidates/` contains the compressed source responses and queries. Eligibility is automated, not a claim that every object is a major attraction or has been manually reviewed.

## Graph and migration

- 1,665 places have approximate/legacy road attachments; 36 remain visible and searchable without a road anchor within the configured 200 m radius. An attachment is not a verified entrance or legal driving approach.
- The semantic graph has **2,806 nodes and 3,366 relationships**: 1,701 destinations, 1,102 shared road anchors and three categories.
- The road graph remains 12,862 nodes and 28,802 directed edges. Strata SSSP, relationship BFS/subgraphs and closure impact operate over the expanded place mapping.
- Revision 3 upgrades either the 50-place or 550-place catalog additively, including existing scenarios. Readiness hashes for every stored predecessor are validated; downgrades are refused. Historical snapshots retain their original catalog sizes.
- The exact earlier fixtures are frozen as `places-catalog-v1.json` and `places-catalog-v2.json`. Future source refreshes need a new catalog revision. The 1,000-place intermediate selection was never published.
- Import recovery replays deterministic upserts; it keeps the workaround for Strata issue [#3477](https://github.com/stratalab/strata-core/issues/3477), avoiding oversized graph deletion.

Regenerate completely offline:

```sh
python3 tools/expand_places.py
python3 tools/extract_places.py
```

The generated catalog, curation, source bundle and quality/selection reports reproduce byte for byte. The extractor checks that every one of the previous 550 normalized records is unchanged.

## UI and workload bounds

The console loads all 18 version-pinned API pages. Endpoint suggestions stay bounded; map density stays capped at 180 markers and 60 labels, with selected destinations prioritized. All places remain searchable regardless of map density. The browser check selects Manhattan Bridge Arch, a new destination beyond the first 1,500 catalog entries.

`island-stress --profile curated-all` measures the full catalog; `curated-50` and `curated-550` still load their frozen fixtures. Typed benchmark cursor positions now use the complete landmark index, including the second page beyond 1,000 nodes. Older benchmark reports remain labeled with their original catalog sizes.

## Verification

The catalog/scenario integration suite and recovery suite passed (seven tests). Recovery includes eight fresh-import/scenario termination checkpoints, one interrupted upgrade from the 50-place revision, and three interrupted upgrades from the 550-place revision. Historical place counts, reopening, audit chains and a second restart are checked. Clippy passed with warnings denied.

Browser checks passed for the full catalog, keyboard selection/routing to Manhattan Bridge Arch, details and relationship exploration, discovery, closure impact/reopening, light/dark themes and mobile layout. All 18 API pages matched the 1,701-record fixture exactly. The four-client, 100-request discovery check returned 100 HTTP 200 responses with consistent distances.

The running durable database was backed up and upgraded from 550 to 1,701 places. Its original Port Authority–Grand Central route remains 1,464 m. No new engine defect was observed in these checks; existing tracked limitations remain documented in the friction ledger.
