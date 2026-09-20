# Initial graph stress measurements

2026-09-19 · `stratadb v1.2.3` / `6fc481c33473efd7d1724284107b67be08625dcd` · release build · Linux x86_64 · AMD Ryzen 7 7800X3D.

These are exploratory runs on a shared development machine, with other validation running concurrently. They establish executable workloads and observed scaling, not controlled regression baselines or performance guarantees. Raw samples and run parameters are in the adjacent JSON files. The app directory was uncommitted during these runs; `app_commit` alone does not identify the working tree. Later runner revisions also record a compiled source hash.

| Workload, p50 milliseconds | 1k nodes / 4k edges | 10k / 40k | 100k / 400k |
|---|---:|---:|---:|
| One-edge batch, storage | 8.60 | 92.80 | 1,156.11 |
| Typed first page, limit 20 | 1.06 | 14.23 | 116.46 |
| Graph metadata, storage | 6.54 | 79.10 | 805.36 |
| SSSP on cached snapshot | 0.025 | 0.26 | 3.50 |
| Branch fork | 34.77 | 651.76 | 34,555.84 |

The first two profiles use 20 warmups and 200 measured operations; the 100k profile uses 2 and 20. Snapshot measurements are capped at 20; branch actions have four individual samples. No p99 is reported for fewer than 100 samples. Fork percentiles are not statistically established from four samples. The batch re-upserts an existing edge with identical data. Typed nodes use one `Place` type; edges form a bidirectional ring, a forward stride, and a shared hub. The synthetic seed controls weights.

A separate engine-only fork probe, without prior query/edit workloads, measured 26,288.991 / 27,596.574 / 26,228.978 ms for three fresh child forks at 100k/400k. Run `cargo run --release --example fork_probe -- 100000`. Reported as [#3475](https://github.com/stratalab/strata-core/issues/3475), related to the earlier closed #2527.

The 100k process reached approximately 2.8 GiB peak RSS. `--memory-mb` configures the engine storage budget, not a process RSS ceiling. The matrix wrapper enforces a separate process RSS ceiling and time limit, recording termination as a failed run rather than a successful benchmark.

`curated-always.json` measures the real 12,862-node/28,802-edge road graph and imports the place graph in durable **Always** mode. It uses 20 repetitions, two warmups, and four concurrent cached SSSP jobs per parallel round. Its database is separate from the running demo. The early synthetic ladder predates the parallel-round field; do not infer concurrency results from missing fields.

`http-4.json` and `http-16.json` measure 200 read-only discovery requests against a cache-mode test server. Four clients: 200 successful responses, about 5.1 ms successful p95. Sixteen clients: 114 successful responses and 86 explicit HTTP 429 responses; successful responses remained distance/version-consistent. These runs include HTTP, serialization, and queueing. The server admits two active analysis jobs and eight waiting jobs. Rejected responses are counted separately and must not be described as successful throughput.

## Reproduce

```sh
cargo build --release --bins
python3 tools/stress_matrix.py --output /tmp/island-cache-runs --mode cache
python3 tools/stress_matrix.py --output /tmp/island-durable-runs --mode durable --profiles curated-50 synthetic-1000 synthetic-10000
# The wrapper requires a fresh output path, runs three independent repetitions,
# and records failures when a process exceeds 4096 MiB RSS or 600 seconds.

cargo run --release --bin island -- --dataset v2 --cache --bind 127.0.0.1:7451
python3 tools/stress_http.py --url http://127.0.0.1:7451 --clients 4 --requests 200 --report /tmp/http-4.json
```

The expanded matrix (all shapes, all branch counts/chunks, longer churn, iterative algorithms) remains a benchmark extension. The checked-in results cover only the explicitly recorded profiles. Do not extrapolate the product latency targets to large synthetic workloads.

## 550-place expansion

`curated550-cache.json` adds a release/cache real-data run with 20 repetitions, two warmups, four concurrent SSSP jobs, and one branch fork/compare/archive sample. It reports the unchanged road graph plus the 985-node/1,087-edge semantic graph. New `places_*` workloads measure typed first/middle/last pages, storage snapshot construction and cached BFS plus induced subgraphs. Single-process cache measurements are not durable p95/p99 claims; the fork measurement has one sample. `curated-50` still uses the original frozen fixture for future controlled comparisons.

```sh
cargo run --release --bin island-stress -- --profile curated-550 --mode cache \
  --repetitions 20 --warmup 2 --branches 1 --concurrency 4 \
  --report /tmp/curated550-cache.json
```

`curated550-http4.json` records 100 discovery requests from four clients against the 550-place cache server: all returned HTTP 200 with consistent distances; successful-response p95 was about 7.0 ms on this run. This measures the unchanged street graph with the larger place-result mapping, including HTTP/serialization and queueing.

`curated50-cache-baseline.json` repeats the frozen 50-place catalog with the same runner/settings. Both are single local cache runs; background recovery tests were active, so this is a scaling observation, not an isolated performance comparison. Median milliseconds:

| Workload | 50 places | 550 places |
|---|---:|---:|
| Typed first page (storage) | 0.070 | 0.773 |
| Semantic snapshot (storage) | 0.251 | 2.895 |
| BFS + induced subgraph (cached) | 0.002 | 0.009 |

## All eligible places (catalog revision 3)

`curated-all-cache.json` measures all 1,701 places: 2,806 semantic nodes and 3,366 relationships, alongside the unchanged road graph. Settings: release/cache, 20 repetitions, two warmups, four concurrent SSSP jobs, one branch fork/compare/archive sample. Typed first/middle/last cursors now come from the complete 1,674-landmark index, rather than only its first 1,000 records.

| Workload | Median ms |
|---|---:|
| Typed first page (storage) | 2.381 |
| Typed middle page (storage) | 2.418 |
| Typed last page (storage) | 2.310 |
| Semantic snapshot (storage) | 9.478 |
| BFS + induced subgraph (cached) | 0.013 |

`curated-all-http4.json` records 100 discovery requests from four clients: 100 HTTP 200 responses, consistent versions/distances, about 15.3 ms successful-response p95. These are single local cache runs with background recovery tests active; they are not isolated scaling or durable-latency guarantees. No new engine failures occurred in these workloads. Existing tracked engine limitations and workarounds still apply.

```sh
cargo run --release --bin island-stress -- --profile curated-all --mode cache \
  --repetitions 20 --warmup 2 --branches 1 --concurrency 4 \
  --report /tmp/curated-all-cache.json
```

## Manhattan subway catalog

`curated-subway-cache.json` uses catalog v4 (1,852 places, 3,035 semantic nodes,
3,666 semantic edges). The subway graph is inherited by branch forks; this
existing harness measures streets and place relationships. The original
`curated-all` profile remains pinned to catalog v3 for comparable historical runs.

`subway-http4.json` separately checks outgoing BFS depth 2 from **every one of the
151 stations**, with four clients. All 151 results match independent traversal of
the pinned 842-edge topology. HTTP p50/p95 were 8.87/13.01 ms; snapshot p50/p95
1.65/2.65 ms; BFS plus subgraph p50/p95 0.0038/0.0093 ms. Snapshot construction is
per request. These are single local release/cache runs with debug recovery tests
running concurrently, not isolated performance guarantees.

```sh
cargo run --release --bin island-stress -- --profile curated-subway --mode cache \
  --repetitions 20 --warmup 2 --branches 1 --concurrency 4 \
  --report /tmp/curated-subway-cache.json
python3 tools/check_subway_http.py --url http://127.0.0.1:7453 \
  --report /tmp/subway-http4.json
```

## Manhattan addresses

Recorded 2026-09-20 against `stratadb v1.2.3` (`6fc481c`), normalized address
fixture hash `236063f2897d7571`. The probe imports only the address graph into a
disposable database. Cache scaling and Always-durable results are separate series.
These are single local runs on a shared host, not isolated performance guarantees.

| Profile / raw result | Import ms | Hydrate/index ms | Search p95 ms | Fork ms |
|---|---:|---:|---:|---:|
| [1,000 cache](addresses-1000.json) | 196 | 26 | 0.034 | 47 |
| [10,000 cache](addresses-10000.json) | 954 | 298 | 0.412 | 864 |
| [63,103 cache](addresses-full-cache.json) | 8,522 | 1,811 | 2.810 | 16,508 |
| [63,103 Always durable](addresses-full-durable.json) | 13,893 | 2,588 | 2.866 | 767 |

Each search quantile uses 40 queries; other timings are individual samples.
Full size is 112,562 nodes and 186,200 edges. Peak process RSS, including parsing
the entire fixture before truncating smaller profiles, was 448,844 / 463,340 /
2,149,216 / 2,778,300 KiB respectively. This is not engine-only memory usage.
The full durable probe occupied 566,665,279 logical bytes after fork/archive/reopen,
including retained history and WAL; reopen plus hydration took 8.81 seconds.

The durable run measured graph metadata at 601 ms, a first typed page of 20 at
91 ms, and a first neighbor page of 10 at 25.9 ms on a street with 2,108 addresses.
The neighbor implementation hydrates the complete adjacency before pagination;
reported as [#3489](https://github.com/stratalab/strata-core/issues/3489).
Application result limits do not imply bounded engine work.

[HTTP verification](addresses-http4.json) uses the complete city in disposable
cache mode. Independent Python Dijkstra matched station ordering and every one
of 588 affected addresses (571 farther, 17 newly unreachable). Retry creation
returned the same scenario; reopening restored access and the audit passed.
Eighty search requests across four clients measured p50/p95 of 3.01/4.63 ms.
The full-city scenario fork took 23.7 seconds in cache mode; first impact results
took about 100 ms. These are observations, not latency targets.

```sh
cargo run --release --example address_probe -- 1000
cargo run --release --example address_probe -- 10000
cargo run --release --example address_probe -- 63103
cargo run --release --example address_probe -- 63103 --durable
# HTTP verifier intentionally requires a disposable cache server:
./target/release/island --dataset v2 --cache --bind 127.0.0.1:7453
python3 tools/check_addresses_http.py
```

## Multimodal journeys

[journeys-http.json](journeys-http.json) records six read-only itinerary checks on
2026-09-20 against the durable migration candidate, with 66,007 journey nodes and
201,567 weighted edges on published engine v1.2.3. Independent Python Dijkstra
matched every native SSSP result, including transfers, reverse travel, walking-only,
and zero-length trips. Queries on the existing closure branch produced identical
transit legs; default/car requests retained identical road paths.

Individual HTTP samples were 13–36 ms, with native SSSP at 8–14 ms on this run.
These are six heterogeneous samples on a shared host, not latency percentiles or
performance guarantees. Train costs are reference-weekday estimates and include
assumed boarding waits. Reproduce with `python3 tools/check_journeys_http.py` against
a disposable copy on port 7454; see [data and model](../journeys.md).
