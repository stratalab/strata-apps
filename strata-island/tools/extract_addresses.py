#!/usr/bin/env python3
"""Offline NYC address normalization/attachment. No network; frozen places unchanged."""
import json, hashlib, math, re, unicodedata
from collections import defaultdict, Counter
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]; F=ROOT/'fixtures'
WORDS={'st':'street','ave':'avenue','av':'avenue','rd':'road','blvd':'boulevard','pl':'place','dr':'drive','ln':'lane','sq':'square','pkwy':'parkway','ter':'terrace','w':'west','e':'east','n':'north','s':'south',**{w:str(i) for i,w in enumerate(['first','second','third','fourth','fifth','sixth','seventh','eighth','ninth','tenth','eleventh','twelfth'],1)}}
def normalize(s):
    s=s.replace('½',' 1/2').replace('¼',' 1/4').replace('¾',' 3/4').replace('–','-')
    s=''.join(c for c in unicodedata.normalize('NFKD',s.lower()) if not unicodedata.combining(c))
    ts=re.findall(r'[^\W_]+(?:[-/][^\W_]+)*',s)
    return ' '.join(WORDS.get(t,re.sub(r'^(\d+)(st|nd|rd|th)$',r'\1',t)) for t in ts)
def project(lon,lat):return round((lon+74.017)*111320*math.cos(math.radians(40.7003))),round((lat-40.7003)*110540)
def distance_segment(x,y,a,b):
    dx=b['x']-a['x'];dy=b['y']-a['y'];t=max(0,min(1,((x-a['x'])*dx+(y-a['y'])*dy)/(dx*dx+dy*dy or 1)))
    return math.hypot(x-a['x']-t*dx,y-a['y']-t*dy)
def valid_bin(s):return bool(re.fullmatch(r'1\d{6}',s or '') and s!='1000000')
def main():
    manifest=json.loads((F/'address-source/manifest.json').read_text())
    for n,m in manifest['files'].items():assert hashlib.sha256((F/'address-source'/n).read_bytes()).hexdigest()==m['sha256']
    rows=json.loads((F/'address-source/rows.json').read_text());roads=json.loads((F/'manhattan-drive.json').read_text());nodes={n['id']:n for n in roads['nodes']}
    # Local grid is a replaceable spatial adapter, not a database spatial index.
    grid=defaultdict(list); seen=set(); adjacency=defaultdict(set)
    for e in roads['edges']:
        adjacency[e['src']].add(e['dst']);adjacency[e['dst']].add(e['src'])
        key=(min(e['src'],e['dst']),max(e['src'],e['dst']),normalize(e.get('name') or ''))
        if not key[2] or key in seen:continue
        seen.add(key);a,b=nodes[key[0]],nodes[key[1]]
        for gx in range(min(a['x'],b['x'])//200,max(a['x'],b['x'])//200+1):
            for gy in range(min(a['y'],b['y'])//200,max(a['y'],b['y'])//200+1):grid[key[2],gx,gy].append(key)
    component={};sizes={}
    for n in nodes:
        if n in component:continue
        todo=[n];component[n]=n;size=0
        while todo:
            k=todo.pop();size+=1
            for j in adjacency[k]:
                if j not in component:component[j]=n;todo.append(j)
        sizes[n]=size
    resolutions=json.loads((F/'address-resolutions.json').read_text());groups=defaultdict(list)
    for r in rows:groups[r['addresspointid']].append(r)
    output=[];excluded=[];attachment_counts=Counter();duplicates=[]
    for aid,rs in sorted(groups.items()):
        if len(rs)>1:
            choice=resolutions.get(aid)
            assert choice and set(choice['source_rows'])=={r['objectid'] for r in rs}, f'unreviewed duplicate {aid}'
            r=next(r for r in rs if r['objectid']==choice['selected']);duplicates.append(choice)
        else:r=rs[0]
        reason=None
        if r.get('address_status') not in (None,'','4'):reason='status:'+r['address_status']
        if not r.get('house_number') or not r.get('full_street_name') or r.get('boroughcode')!='1':reason='invalid_components'
        lon,lat=r['the_geom']['coordinates']
        if not (-74.1<lon<-73.8 and 40.65<lat<40.92):reason='invalid_coordinates'
        if reason:excluded.extend({'objectid':v['objectid'],'reason':reason} for v in rs);continue
        suffix=r.get('house_number_suffix','');end_suffix=r.get('house_number_range_suffix','')
        house=r['house_number']+(' ' if '/' in suffix else '')+suffix;end=r.get('house_number_range','')+(' ' if '/' in end_suffix else '')+end_suffix
        if end and end!=house:house+='–'+end
        street=normalize(r['full_street_name']);x,y=project(lon,lat);candidates=set()
        for gx in range(x//200-1,x//200+2):
            for gy in range(y//200-1,y//200+2):candidates.update(grid.get((street,gx,gy),[]))
        choices=[]
        for a,b,_ in candidates:
            n=min((nodes[a],nodes[b]),key=lambda n:(math.hypot(x-n['x'],y-n['y']),n['id']))
            approach=math.hypot(x-n['x'],y-n['y']);d=distance_segment(x,y,nodes[a],nodes[b])
            if approach<=200 and d<=100:choices.append((d,approach,n['id'],a,b))
        choices.sort();anchor=None;why='no_matching_street';evidence={}
        if r.get('zipcode') in ('10044','10463'):why='outside_street_coverage'
        elif choices:
            d,approach,n,a,b=choices[0]
            # Reject equally close unrelated segments; adjacent segments at an intersection are expected.
            ambiguous=any(c[0]<=d+3 and not ({a,b}&{c[3],c[4]}) and c[2]!=n for c in choices[1:])
            if ambiguous:why='ambiguous_street'
            elif sizes[component[n]]<100:why='isolated_component'
            else:anchor=n;why='approximate'
            evidence={'segment':[a,b],'segment_distance_m':round(d),'candidate_count':len(choices),'component_nodes':sizes[component[n]]}
        attachment_counts[why]+=1
        name=f'{house} {street.title()}'
        output.append({'id':'a:nyc:'+aid,'name':name,'category':'address','x':x,'y':y,'node':anchor,'approach_m':round(choices[0][1]) if anchor else 0,'attachment':why,'source_url':'https://data.cityofnewyork.us/d/uf93-f8nk','coordinate_method':'nyc_address_point','aliases':[], 'house':normalize(house.replace('–','-')),'street':street,'street_id':'street:'+r.get('b7sc_actual',street.replace(' ','_')),'bin':r['bin'] if valid_bin(r.get('bin')) else None,'zip':r.get('zipcode'),'source_rows':sorted(v['objectid'] for v in rs),'source':r,'attachment_evidence':evidence,'place_ids':[]})
    # Link only exact normalized source addresses with compatible coordinates (<=80m).
    bykey=defaultdict(list)
    for a in output:bykey[(a['house'],a['street'])].append(a)
    places=json.loads((F/'places-curated.json').read_text())['places'];byid={p['id']:p for p in places};enrichment=[]
    for file in ('osm.json','expansion.json'):
        for e in json.loads((F/'places-source'/file).read_text())['elements']:
            tags=e.get('tags',{});pid=f"p:{e['type']}:{e['id']}";p=byid.get(pid)
            if not p or not tags.get('addr:housenumber') or not tags.get('addr:street'):continue
            matches=[a for a in bykey[(normalize(tags['addr:housenumber']),normalize(tags['addr:street']))] if math.hypot(p['x']-a['x'],p['y']-a['y'])<=80]
            record={'place_id':pid,'house':tags['addr:housenumber'],'street':tags['addr:street'],'address_id':matches[0]['id'] if len(matches)==1 else None}
            enrichment.append(record)
            if len(matches)==1:matches[0]['place_ids'].append(pid)
    for a in output:a['place_ids'].sort()
    data={'schema':1,'catalog':'addresses-v1','policy':'nyc-1','source_sha256':manifest['files']['rows.json']['sha256'],'road_sha256':hashlib.sha256((F/'manhattan-drive.json').read_bytes()).hexdigest(),'addresses':output}
    (F/'addresses-v1.json').write_text(json.dumps(data,ensure_ascii=False,separators=(',',':'))+'\n')
    (F/'address-enrichment-v1.json').write_text(json.dumps(enrichment,ensure_ascii=False,indent=2)+'\n')
    quality={'raw_rows':len(rows),'accepted':len(output),'excluded_rows':len(excluded),'collapsed_duplicate_rows':sum(len(a['source_rows'])-1 for a in output),'attachments':dict(attachment_counts),'buildings':len({a['bin'] for a in output if a['bin']}),'streets':len({a['street_id'] for a in output}),'anchors':len({a['node'] for a in output if a['node']}),'linked_places':sum(len(a['place_ids']) for a in output),'duplicates':duplicates,'excluded':excluded}
    assert quality['accepted']+quality['excluded_rows']+quality['collapsed_duplicate_rows']==len(rows)
    (F/'addresses-quality.json').write_text(json.dumps(quality,indent=2)+'\n')
    print(json.dumps({k:v for k,v in quality.items() if k not in ('excluded','duplicates')},indent=2))
if __name__=='__main__':main()
