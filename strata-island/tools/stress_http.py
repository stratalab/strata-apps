#!/usr/bin/env python3
"""Read-only concurrency load against an explicitly selected running island server."""
import argparse
import concurrent.futures
import json
import platform
import statistics
import time
import urllib.error
import urllib.request
from pathlib import Path

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--url',required=True)
p.add_argument('--clients',type=int,choices=[1,4,8,16],default=4)
p.add_argument('--requests',type=int,default=200)
p.add_argument('--report',type=Path,required=True)
a=p.parse_args()
if not 1<=a.requests<=10000:p.error('requests must be 1..10000')
payload=json.dumps({'origin':'poi:port-authority','max_m':5000}).encode()
def query(i):
    start=time.perf_counter()
    req=urllib.request.Request(a.url.rstrip('/')+'/api/discover',data=payload,headers={'Content-Type':'application/json'})
    try:
        with urllib.request.urlopen(req,timeout=30) as r:status=r.status;body=json.load(r)
        distances=sorted((x['place']['id'],x['distance_m']) for x in body['results'])
        return {'status':status,'ms':(time.perf_counter()-start)*1000,'distances':distances,'version':body['version']}
    except urllib.error.HTTPError as e:return {'status':e.code,'ms':(time.perf_counter()-start)*1000}
    except Exception as e:return {'status':'error','error':str(e),'ms':(time.perf_counter()-start)*1000}
started=time.perf_counter()
with concurrent.futures.ThreadPoolExecutor(max_workers=a.clients) as pool:results=list(pool.map(query,range(a.requests)))
elapsed=time.perf_counter()-started
success=[r for r in results if r['status']==200]
assert success,'no successful requests'
assert all(r['distances']==success[0]['distances'] and r['version']==success[0]['version'] for r in success),'inconsistent immutable snapshot results'
values=sorted(r['ms'] for r in results)
report={'url':a.url,'clients':a.clients,'requests':a.requests,'platform':platform.platform(),'elapsed_s':elapsed,'throughput_rps':a.requests/elapsed,'p50_ms':statistics.median(values),'p95_ms':values[round((len(values)-1)*.95)],'p99_ms':values[round((len(values)-1)*.99)] if len(values)>=100 else None,'statuses':{str(s):sum(r['status']==s for r in results) for s in set(r['status'] for r in results)},'successful_requests':len(success),'successful_p50_ms':statistics.median(r['ms'] for r in success),'successful_p95_ms':sorted(r['ms'] for r in success)[round((len(success)-1)*.95)],'consistent_distances':True,'note':'End-to-end HTTP times include serialization and queueing. Read-only SSSP against a cached road snapshot.'}
a.report.parent.mkdir(parents=True,exist_ok=True)
a.report.write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
