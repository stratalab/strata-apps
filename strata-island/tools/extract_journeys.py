#!/usr/bin/env python3
"""Offline pedestrian + coherent weekday train-pattern graph. Never edits old fixtures."""
import collections,csv,datetime,hashlib,io,json,math,statistics,zipfile
from pathlib import Path
from extract_places import project
F=Path(__file__).resolve().parents[1]/'fixtures'
WALK={'primary','primary_link','secondary','secondary_link','tertiary','tertiary_link','unclassified','residential','living_street','service','footway','path','pedestrian','steps','track'}
def allowed(tags):
    foot=tags.get('foot',''); hw=tags.get('highway','')
    return (hw in WALK or hw in {'cycleway','bridleway'} and foot in {'yes','designated','permissive'}) and foot not in {'no','private','use_sidepath'} and (tags.get('access') not in {'no','private','customers'} or foot in {'yes','designated','permissive'}) and tags.get('area')!='yes' and tags.get('indoor')!='yes' and tags.get('construction') is None

def seconds(s):
    h,m,s=map(int,s.split(':'));return h*3600+m*60+s

def main():
    manifest=json.loads((F/'walking-source/manifest.json').read_text());raw=(F/'walking-source/osm.json').read_bytes();assert hashlib.sha256(raw).hexdigest()==manifest['sha256']
    elements=json.loads(raw)['elements'];rawnodes={e['id']:e for e in elements if e['type']=='node'}
    boundary=json.loads((F/'places-source/boundary.json').read_text())['elements'][0]
    # Latitude buckets accelerate ray casting without an optional GIS dependency.
    bins=collections.defaultdict(list)
    for member in boundary['members']:
        if member.get('role')!='outer':continue
        points=member.get('geometry',[])
        for a,b in zip(points,points[1:]):
            for k in range(math.floor(min(a['lat'],b['lat'])*1000),math.floor(max(a['lat'],b['lat'])*1000)+1):bins[k].append((a,b))
    def inside(n):
        lat,lon=n['lat'],n['lon']
        return sum(1 for a,b in bins[math.floor(lat*1000)] if (a['lat']>lat)!=(b['lat']>lat) and lon < (b['lon']-a['lon'])*(lat-a['lat'])/(b['lat']-a['lat'])+a['lon'])%2==1
    xy={i:project(n['lat'],n['lon']) for i,n in rawnodes.items() if inside(n) and not (n.get('tags',{}).get('access') in {'no','private'} and n.get('tags',{}).get('foot') not in {'yes','designated'})}
    links={};adj=collections.defaultdict(set);incident=collections.defaultdict(set)
    for way in elements:
        if way['type']!='way' or not allowed(way.get('tags',{})):continue
        t=way['tags'];label=(t.get('name',''),t['highway']);ow=t.get('oneway:foot','no')
        for a,b in zip(way['nodes'],way['nodes'][1:]):
            if a==b or a not in xy or b not in xy:continue
            dist=max(1,round(math.dist(xy[a],xy[b])))
            for s,d in ([(a,b)] if ow in {'yes','1'} else [(b,a)] if ow=='-1' else [(a,b),(b,a)]):
                key=(s,d);v=(dist,*label)
                if key not in links or v<links[key]:links[key]=v
            adj[a].add(b);adj[b].add(a);incident[a].add(label);incident[b].add(label)
    # Assign component IDs once (also used to reject tiny attachments).
    component={};groups=[]
    for n in sorted(adj):
        if n in component:continue
        cid=len(groups);group=[n];component[n]=cid
        for a in group:
            for b in adj[a]:
                if b not in component:component[b]=cid;group.append(b)
        groups.append(group)
    valid={n for group in groups if len(group)>=100 for n in group}
    # Keep junctions, direction/name changes, and regularly spaced points for endpoint snapping.
    keep={n for n in valid if len(adj[n])!=2 or len(incident[n])!=1 or any((n,b) not in links or (b,n) not in links for b in adj[n])}
    # Long chains must retain nearby snap points, even without junctions.
    for n in sorted(valid):
        if n%7==0:keep.add(n)
    edges=[];used=set()
    for a in sorted(keep):
        for b in sorted(adj[a]):
            if (a,b) not in links:continue
            prev,cur=a,b;chain=[a,b];length=links[a,b][0];seen={a,b}
            while cur not in keep:
                opts=[n for n in adj[cur] if n!=prev and (cur,n) in links]
                if len(opts)!=1:break
                nxt=opts[0]
                if nxt in seen:break
                length+=links[cur,nxt][0];prev,cur=cur,nxt;chain.append(cur);seen.add(cur)
            if cur==a or cur not in valid:continue
            used.update([a,cur]);edges.append(dict(source=f'w:{a}',target=f'w:{cur}',kind='walk',seconds=math.ceil(length/1.35),meters=length,name=links[a,b][1] or ('Stairs' if links[a,b][2]=='steps' else 'Walkway'),points=[list(xy[n]) for n in chain]))
    nodes=[dict(id=f'w:{n}',x=xy[n][0],y=xy[n][1],kind='walk',component=component[n]) for n in sorted(used)]
    # Short geometric station approaches are explicitly approximate, never entrances.
    grid=collections.defaultdict(list)
    for n in nodes:grid[n['x']//150,n['y']//150].append(n)
    subway=json.loads((F/'subway.json').read_text());stations=subway['stations'];by_station={s['id']:s for s in stations};parents={p:s['id'] for s in stations for p in s['gtfs_stop_ids']}
    station_anchors={}
    def edge(a,b,kind,sec,meters=0,**kw):edges.append(dict(source=a,target=b,kind=kind,seconds=max(1,sec),meters=meters,**kw))
    for s in stations:
        nodes.append(dict(id=s['id'],x=s['x'],y=s['y'],kind='station',name=s['name']))
        candidates=[n for dx in [-1,0,1] for dy in [-1,0,1] for n in grid[s['x']//150+dx,s['y']//150+dy]]
        if candidates:
            n=min(candidates,key=lambda n:(math.hypot(n['x']-s['x'],n['y']-s['y']),n['id']));d=round(math.hypot(n['x']-s['x'],n['y']-s['y']))
            if d<=150:
                station_anchors[s['id']]=dict(node=n['id'],meters=d)
                for a,b in [(n['id'],s['id']),(s['id'],n['id'])]:edge(a,b,'access',math.ceil(d/1.35),d,points=[[n['x'],n['y']],[s['x'],s['y']]] if a==n['id'] else [[s['x'],s['y']],[n['x'],n['y']]])
    prov=json.loads((F/'subway-source/manifest.json').read_text());zpath=F/'subway-source/gtfs_subway.zip';assert hashlib.sha256(zpath.read_bytes()).hexdigest()==prov['sha256']['gtfs_subway.zip']
    with zipfile.ZipFile(zpath) as z:
        def table(n):return list(csv.DictReader(io.StringIO(z.read(n+'.txt').decode('utf-8-sig'))))
        reference=datetime.date(2026,9,21);date=reference.strftime('%Y%m%d');weekday=reference.strftime('%A').lower()
        active={c['service_id'] for c in table('calendar') if c['start_date']<=date<=c['end_date'] and c[weekday]=='1'}
        for c in table('calendar_dates'):
            if c['date']==date:
                if c['exception_type']=='1':active.add(c['service_id'])
                else:active.discard(c['service_id'])
        stop_parents={s['stop_id']:s['parent_station'] or s['stop_id'] for s in table('stops')}
        trips={t['trip_id']:t for t in table('trips') if t['service_id'] in active}
        sequences=collections.defaultdict(list)
        for t in table('stop_times'):
            if t['trip_id'] in trips:sequences[t['trip_id']].append(t)
        patterns=collections.defaultdict(list)
        for tid,seq in sorted(sequences.items()):
            seq.sort(key=lambda s:int(s['stop_sequence']))
            if not 10*3600<=seconds(seq[0]['departure_time'])<16*3600:continue
            chunks=[];chunk=[]
            for stop in seq:
                sid=parents.get(stop_parents.get(stop['stop_id']))
                if sid:
                    if not chunk or chunk[-1][0]!=sid:chunk.append((sid,stop))
                else:
                    if len(chunk)>1:chunks.append(chunk)
                    chunk=[]
            if len(chunk)>1:chunks.append(chunk)
            for chunk in chunks:
                key=(trips[tid]['route_id'],trips[tid]['trip_headsign'],tuple((s, t.get('pickup_type','0'),t.get('drop_off_type','0')) for s,t in chunk))
                times=[max(1,seconds(b['arrival_time'])-seconds(a['arrival_time'])) for (_,a),(_,b) in zip(chunk,chunk[1:])]
                patterns[key].append(times)
        pattern_rows=[]
        for idx,(key,samples) in enumerate(sorted(patterns.items())):
            route,headsign,stops=key;pid=f'typical:{idx}';pattern_rows.append(dict(id=pid,route=route,headsign=headsign,stations=[s[0] for s in stops],samples=len(samples)))
            for i,(sid,pickup,dropoff) in enumerate(stops):
                s=by_station[sid];nid=f't:{idx}:{i}';nodes.append(dict(id=nid,x=s['x'],y=s['y'],kind='train',station=sid,pattern=pid))
                if pickup in {'','0'}:edge(sid,nid,'board',360,route=route,headsign=headsign,pattern=pid,station=sid)
                if dropoff in {'','0'}:edge(nid,sid,'alight',60,route=route,station=sid)
                if i:
                    previous=by_station[stops[i-1][0]];distance=round(math.hypot(s['x']-previous['x'],s['y']-previous['y']))
                    edge(f't:{idx}:{i-1}',nid,'ride',round(statistics.median(t[i-1] for t in samples)),distance,route=route,headsign=headsign,pattern=pid,from_station=previous['id'],to_station=sid,points=[[previous['x'],previous['y']],[s['x'],s['y']]])
        transfers={}
        for t in table('transfers'):
            if t['transfer_type']=='3':continue
            a=parents.get(stop_parents.get(t['from_stop_id']));b=parents.get(stop_parents.get(t['to_stop_id']))
            if a and b and a!=b:transfers[a,b]=max(transfers.get((a,b),0),int(t.get('min_transfer_time') or 180),60)
        for (a,b),sec in sorted(transfers.items()):
            x,y=by_station[a],by_station[b];m=round(math.hypot(x['x']-y['x'],x['y']-y['y']))
            edge(a,b,'transfer',max(sec,math.ceil(m/1.35)),m,from_station=a,to_station=b,points=[[x['x'],x['y']],[y['x'],y['y']]])
    # Parallel OSM arcs are collapsed by endpoints; shortest deterministic one wins.
    unique={}
    for e in edges:
        k=(e['source'],e['kind'],e['target'])
        if k not in unique or (e['seconds'],e['name'] if 'name' in e else '')<(unique[k]['seconds'],unique[k].get('name','')):unique[k]=e
    result=dict(schema=1,reference_date=reference.isoformat(),window='10:00–16:00 America/New_York',semantics='Typical weekday estimates; no live arrivals. 4-minute wait + 2-minute boarding allowance; 1-minute exit allowance; walking at 1.35 m/s. Station access is approximate. Subway geometry is schematic.',walking_source=manifest,gtfs_sha256=prov['sha256']['gtfs_subway.zip'],nodes=nodes,edges=[unique[k] for k in sorted(unique)],stations=stations,routes=subway['routes'],patterns=pattern_rows,station_anchors=station_anchors)
    (F/'journeys-v1.json').write_text(json.dumps(result,separators=(',',':'),ensure_ascii=False)+'\n')
    print(json.dumps(dict(nodes=len(nodes),edges=len(unique),walking_nodes=len(used),stations=len(stations),connected_stations=len(station_anchors),patterns=len(patterns),reference_date=reference.isoformat()),indent=2))
if __name__=='__main__':main()
