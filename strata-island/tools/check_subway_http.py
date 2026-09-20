#!/usr/bin/env python3
"""Read-only all-station BFS correctness and HTTP timing probe."""
import argparse
import concurrent.futures
import hashlib
import json
from pathlib import Path
import time
import urllib.request

p = argparse.ArgumentParser()
p.add_argument('--url', default='http://127.0.0.1:7453')
p.add_argument('--report', default='/tmp/island-subway-http.json')
args = p.parse_args()
raw = (Path(__file__).resolve().parents[1] / 'fixtures/subway.json').read_bytes()
pin = json.loads(raw)


def run(station):
    seed = station['id']
    expected = {seed}
    for _ in range(2):
        expected |= {e['target'] for e in pin['edges'] if e['source'] in expected}
    request = urllib.request.Request(args.url+'/api/subway/explore',
        data=json.dumps(dict(seed=seed, depth=2)).encode(), headers={'Content-Type': 'application/json'})
    started = time.perf_counter()
    with urllib.request.urlopen(request, timeout=30) as response:
        data = json.load(response)
    elapsed = (time.perf_counter()-started)*1000
    assert {s['id'] for s in data['stations']} == expected, seed
    return dict(seed=seed, version=data['version'], http_ms=elapsed,
                snapshot_ms=data['snapshot_ms'], algorithm_ms=data['algorithm_ms'])


with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
    samples = list(pool.map(run, pin['stations']))
assert len({r['version'] for r in samples}) == 1
report = dict(workload='All 151 station seeds, outgoing BFS depth 2 plus induced subgraph',
              concurrency=4, requests=len(samples), correct_results=len(samples),
              source_sha256=hashlib.sha256(raw).hexdigest(),
              caveat='One local release/cache run with concurrent debug recovery tests; not isolated or a production guarantee',
              samples=samples)
for field in ['http_ms', 'snapshot_ms', 'algorithm_ms']:
    values = sorted(r[field] for r in samples)
    report[field] = dict(p50=values[len(values)//2], p95=values[int(len(values)*.95)])
Path(args.report).write_text(json.dumps(report, indent=2)+'\n')
print(json.dumps({k:v for k,v in report.items() if k != 'samples'}, indent=2))
