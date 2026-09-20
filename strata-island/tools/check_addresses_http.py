#!/usr/bin/env python3
"""Disposable HTTP address oracle and bounded concurrent search benchmark."""
import os,json,urllib.request,urllib.parse,time,heapq,concurrent.futures,statistics,platform
from pathlib import Path
BASE=os.environ.get('ISLAND_URL','http://127.0.0.1:7453')
def api(path,body=None):
 r=urllib.request.Request(BASE+'/api/'+path,data=None if body is None else json.dumps(body).encode(),headers={'Content-Type':'application/json'})
 with urllib.request.urlopen(r,timeout=180) as response:return json.load(response)
def dist(g,origin):
 ids=[n['id'] for n in g['nodes']];adj=[[] for _ in ids]
 for e in g['edges']:adj[e['s']].append((e['d'],e['m']))
 d={ids.index(origin):0};todo=[(0,ids.index(origin))]
 while todo:
  cost,n=heapq.heappop(todo)
  if cost!=d[n]:continue
  for j,w in adj[n]:
   if cost+w<d.get(j,float('inf')):d[j]=cost+w;heapq.heappush(todo,(cost+w,j))
 return {ids[i]:v for i,v in d.items()}
def main():
 meta=api('meta');assert not meta['durable'],'mutating oracle requires disposable --cache server'
 rows=json.loads((Path(__file__).resolve().parents[1]/'fixtures/addresses-v1.json').read_text())['addresses'];assert meta['address_count']==len(rows)
 for q,want in [('350 fif','a:nyc:1019347'),('230 W 55th St','a:nyc:1023622'),('270 E 2 St','a:nyc:5217738'),('94 1/2 Greenwich Street','a:nyc:1000711')]:
  d=api('search?'+urllib.parse.urlencode({'q':q}));assert want in [p['id'] for p in d['places']],(q,d)
 old=api('search?q=230%20west%2055');version=old['version'];origin='a:nyc:1023622';anchor=next(r['node'] for r in rows if r['id']==origin)
 roads=api('city');before=dist(roads,anchor)
 stations=api('addresses/'+origin+'/nearest-stations',{});catalog=api('subway')['stations'];places=[];cursor=None
 while True:
  d=api('places?limit=100'+('&cursor='+urllib.parse.quote(cursor) if cursor else ''));places+=d['places'];cursor=d['cursor']
  if not cursor:break
 expected=sorted((before[p['node']],p['id']) for p in places if p.get('subway') and p.get('node') in before)[:5]
 assert [(r['network_distance_m'],r['place']['id']) for r in stations['results']]==expected
 nearby=api('address-discover',{'origin':origin,'max_m':1000});assert nearby['total']==sum(r['node'] in before and before[r['node']]<=1000 for r in rows)
 request={'from':'city','request_id':'address-http-oracle-v1'};started=time.perf_counter();created=api('close',request);fork_ms=(time.perf_counter()-started)*1000;desk=created['desk']
 try:
  assert api('close',request)['desk']==desk,'retry forked twice'
  child=api('city?branch='+desk);after=dist(child,anchor);counts={};changed={}
  for r in rows:
   x=before.get(r['node']);y=after.get(r['node'])
   status='unconnected' if not r['node'] else 'already_unreachable' if x is None and y is None else 'newly_reachable' if x is None else 'newly_unreachable' if y is None else 'farther' if y>x else 'closer' if y<x else 'unchanged'
   counts[status]=counts.get(status,0)+1
   if status in ['farther','closer','newly_unreachable','newly_reachable']:changed[r['id']]=(status,x,y)
  cursor=None;seen={};impact_ms=[]
  while True:
   t=time.perf_counter();d=api('scenarios/'+desk+'/address-impact',{'origin':origin,'status':'affected','limit':100,'cursor':cursor});impact_ms.append((time.perf_counter()-t)*1000)
   assert d['counts']==counts
   for r in d['results']:seen[r['place']['id']]=(r['status'],r['before_m'],r['after_m'])
   cursor=d['cursor']
   if not cursor:break
  assert seen==changed,(len(seen),len(changed))
  history=api('scenarios/'+desk+'/history');api('scenarios/'+desk+'/operations',{'id':'address-oracle-reopen','closed':False,'expected_version':history['current_version']})
  d=api('scenarios/'+desk+'/address-impact',{'origin':origin,'status':'affected'});assert d['total']==0
  assert api('audit',{})['ok']
 finally:api('archive',{'desk':desk})
 queries=[r['name'] for r in rows[::max(1,len(rows)//80)]][:80]
 def query(q):
  t=time.perf_counter();d=api('search?'+urllib.parse.urlencode({'q':q}));assert d['places'],(q,d);return (time.perf_counter()-t)*1000
 with concurrent.futures.ThreadPoolExecutor(max_workers=4) as ex:times=sorted(ex.map(query,queries))
 result={'profile':'address-http-oracle','host':platform.node(),'clients':4,'search_reps':len(times),'search_p50_ms':statistics.median(times),'search_p95_ms':times[int(.95*len(times))],'scenario_create_ms':fork_ms,'impact_pages':len(impact_ms),'impact_first_page_ms':impact_ms[0],'affected_addresses':len(changed),'counts':counts,'all_affected_rows_match_independent_dijkstra':True,'idempotent_retry':True,'reopen_restores_access':True}
 print(json.dumps(result,indent=2))
if __name__=='__main__':main()
