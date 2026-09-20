#!/usr/bin/env python3
"""Normalize the checked-in curated OSM snapshot. No network in the default path.

Regenerate the current revision with expand_places.py followed by this script.
Do not refresh frozen inputs in place: changed inputs need a new catalog revision
and upgrade path. Never edits road data.
"""
import hashlib
import json
import math
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
F = ROOT / 'fixtures'


def project(lat, lon):
    return (round((lon + 74.0170) * 111320 * math.cos(math.radians(40.7003))),
            round((lat - 40.7003) * 110540))


def points(e):
    if e['type'] == 'node':
        return [e]
    if e['type'] == 'way':
        return e.get('geometry', [])
    return [p for m in e.get('members', []) if m.get('role') == 'outer'
            for p in m.get('geometry', [])]


def main():
    raw = (F / 'places-source/osm.json').read_bytes()
    source = json.loads(raw)
    objects = {(e['type'], e['id']): e for e in source['elements']}
    expansion_raw = (F / 'places-source/expansion.json').read_bytes()
    expansion = json.loads(expansion_raw)
    objects.update({(e['type'],e['id']):e for e in expansion['elements']})
    road_raw = (F / 'manhattan-drive.json').read_bytes()
    roads = json.loads(road_raw)['nodes']
    legacy = {p['id']: p for p in json.loads((F / 'gazetteer.json').read_text())}
    curated = json.loads((F / 'places-curation.json').read_text())
    boundary_raw = (F / 'places-source/boundary.json').read_bytes()
    boundary = json.loads(boundary_raw)['elements'][0]
    assert boundary['tags']['name'] == 'Manhattan'
    segments = [(a, b) for m in boundary['members'] if m.get('role') == 'outer'
                for a, b in zip(m.get('geometry', []), m.get('geometry', [])[1:])]
    def inside(lat, lon):
        return sum(1 for a,b in segments if (a['lat']>lat)!=(b['lat']>lat)
                   and lon < (b['lon']-a['lon'])*(lat-a['lat'])/(b['lat']-a['lat'])+a['lon']) % 2 == 1
    places = []
    review = []
    for c in curated:
        e = objects[c['osm_type'], c['osm_id']]
        tags = e.get('tags', {})
        assert not any(tags.get(k) == 'yes' for k in ['disused', 'demolished', 'proposed'])
        geometry = points(e)
        assert geometry, c['name']
        # Use a real geometry vertex, not a bounding-box center masquerading as an entrance.
        # For areas, select the boundary vertex closest to the geometry's bbox center.
        lat = (min(p['lat'] for p in geometry) + max(p['lat'] for p in geometry)) / 2
        lon = (min(p['lon'] for p in geometry) + max(p['lon'] for p in geometry)) / 2
        if c.get('display_hint'):
            lat, lon = c['display_hint']['lat'], c['display_hint']['lon']
        pin = min(geometry, key=lambda p: (p['lat']-lat)**2 + ((p['lon']-lon)*0.76)**2)
        assert inside(pin['lat'], pin['lon']), c['name']
        x, y = project(pin['lat'], pin['lon'])
        alias = c.get('alias')
        if alias:
            anchor = next(n for n in roads if n['id'] == legacy[alias]['node'])
        else:
            anchor = min(roads, key=lambda n: ((n['x']-x)**2+(n['y']-y)**2, n['id']))
        approach = round(math.hypot(anchor['x']-x, anchor['y']-y))
        connected = approach <= 200 or alias is not None
        place = dict(id=f"p:{e['type']}:{e['id']}", name=c['name'], category=c['category'],
                     x=x, y=y, node=anchor['id'] if connected else None, approach_m=approach,
                     attachment='legacy_anchor' if alias else ('approximate' if connected else 'unavailable'),
                     source_url=f"https://www.openstreetmap.org/{e['type']}/{e['id']}",
                     coordinate_method='osm_node' if e['type']=='node' else 'osm_boundary_vertex',
                     aliases=[alias] if alias else [])
        places.append(place)
        review.append(dict(name=c['name'], source_id=place['id'], lat=pin['lat'], lon=pin['lon'],
                           anchor=place['node'], approach_m=approach, attachment=place['attachment'],
                           caveat='Geometric connection; entrance and legal driving access are not asserted.'))
    assert len(places) == len({p['id'] for p in places}) == 1701
    assert {a for p in places for a in p['aliases']} == set(legacy)
    manifest=dict(schema=2,catalog=3,profile='curated-all',count=len(places),
                  attribution='© OpenStreetMap contributors · ODbL 1.0',
                  source_timestamp=source['osm3s']['timestamp_osm_base'],
                  source_sha256=hashlib.sha256(raw).hexdigest(),
                  expansion_sha256=hashlib.sha256(expansion_raw).hexdigest(),
                  expansion_sources=expansion['sources'],
                  road_sha256=hashlib.sha256(road_raw).hexdigest(),
                  boundary=dict(osm_id=boundary['id'],version=boundary['version'],sha256=hashlib.sha256(boundary_raw).hexdigest()),
                  places=sorted(places,key=lambda p:p['id']))
    original = json.loads((F / 'places-catalog-v2.json').read_text())['places']
    by_id = {p['id']:p for p in places}
    assert all(by_id[p['id']] == p for p in original), 'Original destinations must remain unchanged'
    assert manifest == json.loads((F / 'places-catalog-v3.json').read_text()), 'Frozen OSM catalog changed'
    from extract_subway import main as subway_main
    subway_main()
    (F / 'places-quality.json').write_text(json.dumps(review,indent=2,ensure_ascii=False)+'\n')
    print(f"{len(places)} destinations; {sum(p['node'] is not None for p in places)} road connections")
    print(f"{sum(p['node'] is None for p in places)} destinations remain visible without routing connections")

if __name__ == '__main__':
    main()
