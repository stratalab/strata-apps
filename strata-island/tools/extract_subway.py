#!/usr/bin/env python3
"""Build catalog v4 and a Manhattan service graph from pinned MTA inputs, offline.

Station identity is MTA station_id, not GTFS parent stop or station complex.
Edges are consecutive stops in complete trips, filtered AFTER pairing. The union
of scheduled services is a topology for exploration, not a timetable router.
"""
import collections
import csv
import hashlib
import io
import json
import math
from pathlib import Path
import zipfile

from extract_places import project

ROOT = Path(__file__).resolve().parents[1]
F = ROOT / 'fixtures'
SOURCE = F / 'subway-source'


def ride_pairs(sequence, parents, station_ids):
    """Never bridge an excluded borough by filtering a trip before pairing."""
    for a, b in zip(sequence, sequence[1:]):
        src = station_ids.get(parents.get(a, a))
        dst = station_ids.get(parents.get(b, b))
        if src and dst and src != dst:
            yield src, dst


def main():
    provenance = json.loads((SOURCE / 'manifest.json').read_text())
    for filename, digest in provenance['sha256'].items():
        assert hashlib.sha256((SOURCE / filename).read_bytes()).hexdigest() == digest
    rows = json.loads((SOURCE / 'stations.json').read_text())
    grouped = collections.defaultdict(list)
    for row in rows:
        if row['borough'] == 'M':
            grouped[row['station_id']].append(row)
    roads = json.loads((F / 'manhattan-drive.json').read_text())['nodes']
    base = json.loads((F / 'places-catalog-v3.json').read_text())
    places = list(base['places'])
    stations, station_ids = [], {}
    for station_id, group in sorted(grouped.items(), key=lambda kv: int(kv[0])):
        group.sort(key=lambda r: r['gtfs_stop_id'])
        r = group[0]
        assert len({(g['complex_id'], g['gtfs_latitude'], g['gtfs_longitude']) for g in group}) == 1
        x, y = project(float(r['gtfs_latitude']), float(r['gtfs_longitude']))
        assert -1600 <= x <= 9600 and -600 <= y <= 20400
        anchor = min(roads, key=lambda n: ((n['x']-x)**2+(n['y']-y)**2, n['id']))
        approach = round(math.hypot(anchor['x']-x, anchor['y']-y))
        # The street extract covers Manhattan island, not Roosevelt Island or Marble Hill.
        # Do not snap across water even if a geometric neighbor happens to be close.
        connected = approach <= 200 and station_id not in {'222', '296'}
        pid = f'p:mta:station:{station_id}'
        services = sorted({s for g in group for s in g['daytime_routes'].split()})
        metadata = dict(station_id=station_id, complex_id=r['complex_id'],
                        gtfs_stop_ids=[g['gtfs_stop_id'] for g in group],
                        routes=services, lines=sorted({g['line'] for g in group}))
        p = dict(id=pid, name=f"{r['stop_name']} · {' '.join(services)} subway",
                 category='transit', x=x, y=y, node=anchor['id'] if connected else None,
                 approach_m=approach, attachment='approximate' if connected else 'unavailable',
                 source_url='https://data.ny.gov/d/39hk-dx4f',
                 coordinate_method='mta_station_coordinate', aliases=[], subway=metadata)
        places.append(p)
        stations.append(dict(id=pid, name=r['stop_name'], x=x, y=y, **metadata))
        for g in group:
            assert g['gtfs_stop_id'] not in station_ids
            station_ids[g['gtfs_stop_id']] = pid
    with zipfile.ZipFile(SOURCE / 'gtfs_subway.zip') as z:
        def table(name):
            return list(csv.DictReader(io.StringIO(z.read(name + '.txt').decode('utf-8-sig'))))
        stops = table('stops')
        parents = {s['stop_id']: s['parent_station'] or s['stop_id'] for s in stops}
        assert set(station_ids) <= set(parents), 'Station source and GTFS feed do not align'
        trips = {t['trip_id']: t for t in table('trips')}
        sequences = collections.defaultdict(list)
        for s in table('stop_times'):
            sequences[s['trip_id']].append((int(s['stop_sequence']), s['stop_id']))
        edges = set()
        for tid, sequence in sequences.items():
            route = trips[tid]['route_id']
            for src, dst in ride_pairs([s for _, s in sorted(sequence)], parents, station_ids):
                edges.add((src, dst, 'ride:' + route))
        # Explicit GTFS transfers only; no proximity-invented transfers or reverse edges.
        for t in table('transfers'):
            if t['transfer_type'] == '3':
                continue
            src = station_ids.get(parents.get(t['from_stop_id'], t['from_stop_id']))
            dst = station_ids.get(parents.get(t['to_stop_id'], t['to_stop_id']))
            if src and dst and src != dst:
                edges.add((src, dst, 'transfer'))
        used_routes = {kind[5:] for _, _, kind in edges if kind.startswith('ride:')}
        routes = [dict(id=r['route_id'], name=r['route_short_name'], color=r['route_color'],
                       description=r['route_long_name']) for r in table('routes') if r['route_id'] in used_routes]
        feed = table('feed_info')[0]
    assert len(stations) == 151 and len(places) == 1852
    assert len({p['id'] for p in places}) == len(places)
    network = dict(schema=1, source=provenance, feed=feed,
                   coverage='MTA borough M, including Roosevelt Island and Marble Hill',
                   geometry='Schematic connections between station coordinates; not track geometry',
                   semantics='Union of scheduled service connections; no time, direction-platform, fare, or accessibility routing',
                   stations=stations, routes=sorted(routes, key=lambda r: r['id']),
                   edges=[dict(source=s, target=d, kind=k) for s, d, k in sorted(edges)])
    catalog = dict(base, catalog=4, profile='curated-subway', count=len(places),
                   subway_source=provenance, attribution=base['attribution']+' · MTA Open Data / NYCT GTFS',
                   places=sorted(places, key=lambda p: p['id']))
    for filename, data in [('places-curated.json', catalog), ('subway.json', network)]:
        (F / filename).write_text(json.dumps(data, indent=2, ensure_ascii=False)+'\n')
    print(f'{len(stations)} stations; {len(set(s["complex_id"] for s in stations))} complexes; '
          f'{len(edges)} directed connections; {len(routes)} services; {len(places)} places; '
          f'{sum(p["node"] is not None for p in places)} street anchors')


if __name__ == '__main__':
    main()
