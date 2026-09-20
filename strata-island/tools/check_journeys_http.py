#!/usr/bin/env python3
"""Read-only route oracle: independent Dijkstra on the published fixture."""
import json,heapq,os,time,urllib.request,urllib.error
from pathlib import Path
BASE=os.environ.get('ISLAND_URL','http://127.0.0.1:7454')
def api(body):
 request=urllib.request.Request(BASE+'/api/route',data=json.dumps(body).encode(),headers={'Content-Type':'application/json'})
 with urllib.request.urlopen(request,timeout=30) as r:return json.load(r)
def main():
 data=json.loads((Path(__file__).resolve().parents[1]/'fixtures/journeys-v1.json').read_text());adj={n['id']:[] for n in data['nodes']}
 for e in data['edges']:adj[e['source']].append((e['target'],e['seconds']))
 def dijkstra(src):
  d={src:0};heap=[(0,src)]
  while heap:
   cost,node=heapq.heappop(heap)
   if cost!=d[node]:continue
   for target,weight in adj[node]:
    next=cost+weight
    if next<d.get(target,float('inf')):d[target]=next;heapq.heappush(heap,(next,target))
  return d
 results=[]
 for start,end in [('330','310'),('330','222'),('222','330'),('310','396'),('296','330'),('330','330')]:
  src,dst='p:mta:station:'+start,'p:mta:station:'+end
  t=time.perf_counter();r=api({'from':src,'to':dst,'mode':'transit'});elapsed=(time.perf_counter()-t)*1000
  assert r['duration_s']==dijkstra(src)[dst],(start,end,r)
  assert sum(l['seconds'] for l in r['legs'])==r['duration_s']
  assert sum(l['meters'] for l in r['legs'] if l['mode']=='walk')==r['walking_m']
  assert r['boardings']==sum(l['mode']=='subway' for l in r['legs'])
  child=api({'from':src,'to':dst,'mode':'transit','branch':'desk-0001'})
  assert child['duration_s']==r['duration_s'] and child['legs']==r['legs']
  results.append({'from':src,'to':dst,'duration_s':r['duration_s'],'walking_m':r['walking_m'],'boardings':r['boardings'],'http_ms':elapsed,'native_sssp_ms':r['algorithm_ms']})
 car=api({'from':'poi:port-authority','to':'poi:grand-central','mode':'car'});legacy=api({'from':'poi:port-authority','to':'poi:grand-central'})
 assert car['nodes']==legacy['nodes'] and car['length_m']==legacy['length_m']
 for body in [{'from':'poi:battery','to':'poi:lincoln','mode':'airplane'},{'from':'p:mta:station:330','to':'p:mta:station:310','mode':'transit','version':1}]:
  try:api(body);raise AssertionError('invalid request accepted')
  except urllib.error.HTTPError as e:assert e.code==(412 if 'version' in body else 400),e.code
 print(json.dumps({'profile':'journeys-http','routes':results,'independent_dijkstra_matches':True,'car_default_preserved':True,'car_closure_leaves_transit_unchanged':True},indent=2))
if __name__=='__main__':main()
