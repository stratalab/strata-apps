#!/usr/bin/env python3
"""Select all eligible named Manhattan landmarks from pinned OSM candidates.

Offline, deterministic. Candidates are historic sites, cultural institutions and
public art; not a general business directory. Keep the original 50 intact.
"""
import collections
import gzip
import hashlib
import json
import math
import unicodedata
from pathlib import Path
from extract_places import points, project

F=Path(__file__).resolve().parents[1]/'fixtures'

def normalized(name):
    return ''.join(c for c in unicodedata.normalize('NFKD',name).casefold() if c.isalnum())

def main():
    original=json.loads((F/'places-source/osm.json').read_text())
    old=json.loads((F/'places-catalog-v1.json').read_text())['places']
    old_ids={(p['id'].split(':')[1],int(p['id'].split(':')[2])) for p in old}
    curation=json.loads((F/'places-curation.json').read_text())[:50]
    assert len(curation)==50 and {(p['osm_type'],p['osm_id']) for p in curation}==old_ids
    objects={(e['type'],e['id']):e for e in original['elements']}
    sources=[]
    raw_records=0
    raw_ids=set()
    for path in sorted((F/'places-source/candidates').glob('*.json.gz')):
        raw=gzip.decompress(path.read_bytes());d=json.loads(raw)
        raw_records += len(d['elements'])
        raw_ids.update((e['type'],e['id']) for e in d['elements'])
        sources.append(dict(file=path.name,sha256=hashlib.sha256(raw).hexdigest(),timestamp=d['osm3s']['timestamp_osm_base']))
        for e in d['elements']:
            if (e['type'],e['id']) not in old_ids:objects[e['type'],e['id']]=e
    boundary=json.loads((F/'places-source/boundary.json').read_text())['elements'][0]
    segments=[(a,b) for m in boundary['members'] if m.get('role')=='outer' for a,b in zip(m.get('geometry',[]),m.get('geometry',[])[1:])]
    def inside(lat,lon):
        return sum(1 for a,b in segments if (a['lat']>lat)!=(b['lat']>lat) and lon<(b['lon']-a['lon'])*(lat-a['lat'])/(b['lat']-a['lat'])+a['lon'])%2==1
    candidates=[];rejected=collections.Counter()
    for key,e in objects.items():
        if key in old_ids:continue
        t=e.get('tags',{});name=t.get('name:en',t.get('name','')).strip()
        if not name or normalized(name) in {'library','gallery','church','memorial','monument','artwork','museum','theatre','theater'}:
            rejected['missing_or_generic_name']+=1;continue
        if any(t.get(k) in {'yes','true','1'} for k in ['disused','demolished','proposed','abandoned','construction']) or any(k.startswith(('disused:','demolished:','abandoned:')) for k in t):
            rejected['inactive']+=1;continue
        if t.get('highway') or t.get('railway') or t.get('type') in {'route','network'} or t.get('building:part'):
            rejected['infrastructure_or_subfeature']+=1;continue
        kind=t.get('tourism') or t.get('historic') or t.get('amenity')
        # Curate destinations, not individual memorial benches or small commemorative plaques.
        if t.get('memorial') in {'bench','plaque','stolperstein'}:
            rejected['small_memorial']+=1;continue
        geometry=points(e)
        if not geometry:rejected['no_geometry']+=1;continue
        lat=(min(p['lat'] for p in geometry)+max(p['lat'] for p in geometry))/2
        lon=(min(p['lon'] for p in geometry)+max(p['lon'] for p in geometry))/2
        pin=min(geometry,key=lambda p:(p['lat']-lat)**2+((p['lon']-lon)*.76)**2)
        x,y=project(pin['lat'],pin['lon'])
        if not inside(pin['lat'],pin['lon']) or not(-1600<=x<=9600 and -600<=y<=20400):
            rejected['outside_supported_manhattan']+=1;continue
        priority={'museum':0,'attraction':1,'monument':2,'memorial':3,'theatre':4,'arts_centre':4,'library':5,'university':5,'place_of_worship':6,'gallery':7,'artwork':8}.get(kind,5)
        rank=(0 if t.get('wikidata') or t.get('wikipedia') else 1,priority,normalized(name),key)
        candidates.append(dict(element=e,name=name,kind=kind,x=x,y=y,rank=rank))
    # A matching source identity, or identical knowledge identity, or the same name at the same site,
    # denotes a duplicate representation; similarly named distant sites remain distinct.
    seen=[]
    for p in old:
        e=objects[p['id'].split(':')[1],int(p['id'].split(':')[2])]
        seen.append((normalized(p['name']),e.get('tags',{}).get('wikidata'),p['x'],p['y']))
    unique=[]
    for c in sorted(candidates,key=lambda c:c['rank']):
        wiki=c['element'].get('tags',{}).get('wikidata');name=normalized(c['name'])
        if any((wiki and wiki==w) or (name==n and math.hypot(c['x']-x,c['y']-y)<300) for n,w,x,y in seen):
            rejected['duplicate_site_representation']+=1;continue
        seen.append((name,wiki,c['x'],c['y']));unique.append(c)
    # Round-robin latitude bands keeps uptown represented; within each band,
    # prefer identified cultural/historic sites. This is a selection rule, not a popularity score.
    bands=collections.defaultdict(list)
    for c in unique:bands[c['y']//2500].append(c)
    additions=len(unique)
    selected=[]
    while len(selected)<additions and any(bands.values()):
        for band in sorted(bands):
            if bands[band] and len(selected)<additions:selected.append(bands[band].pop(0))
    assert len(selected)==additions, f'Only {len(selected)} distinct eligible landmarks; do not fabricate the remainder'
    for c in selected:
        e=c['element'];curation.append(dict(osm_type=e['type'],osm_id=e['id'],category='landmark',name=c['name'],alias=None,rationale=f"Named OSM {c['kind']} within the pinned Manhattan boundary; expanded cultural/historic landmark selection.",source_kind=c['kind']))
    (F/'places-curation.json').write_text(json.dumps(curation,indent=2,ensure_ascii=False)+'\n')
    bundle=dict(generator='Strata Island deterministic expansion selector',sources=sources,elements=[c['element'] for c in sorted(selected,key=lambda c:(c['element']['type'],c['element']['id']))])
    (F/'places-source/expansion.json').write_text(json.dumps(bundle,indent=2,ensure_ascii=False)+'\n')
    report=dict(additional=additions,total=len(old)+len(selected),raw_candidate_records=raw_records,raw_unique_candidates=len(raw_ids),eligible_unique=len(unique),eligible_landmarks=len(unique)+sum(p["category"]=="landmark" for p in old),eligible_total=len(unique)+len(old),remaining_unselected=len(unique)-len(selected),rejected=dict(rejected),kinds=dict(collections.Counter(c['kind'] for c in selected)),latitude_bands=dict(collections.Counter(c['y']//2500 for c in selected)),selection='Knowledge identifiers, feature class, name, OSM identity; round-robin 2.5 km north/south bands. Not a popularity ranking.')
    (F/'places-selection.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
if __name__=='__main__':main()
