#!/usr/bin/env python3
"""Run isolated graph profiles with per-process time/RSS ceilings and failure reports."""
import argparse
import json
import subprocess
import time
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--binary',type=Path,default=Path('target/release/island-stress'))
p.add_argument('--output',type=Path,required=True)
p.add_argument('--profiles',nargs='+',default=['synthetic-1000','synthetic-10000','synthetic-50000','synthetic-100000'])
p.add_argument('--repeats',type=int,default=3)
p.add_argument('--mode',choices=['cache','durable'],default='cache')
p.add_argument('--rss-mb',type=int,default=4096)
p.add_argument('--timeout-s',type=int,default=600)
a=p.parse_args()
if a.output.exists():p.error('output must be a fresh directory')
if a.repeats<1 or a.repeats>10 or a.rss_mb<256 or a.timeout_s<1:p.error('invalid limits')
a.output.mkdir(parents=True)
for profile in a.profiles:
    if not profile.replace('-','').isalnum():p.error('invalid profile name')
    for repeat in range(a.repeats):
        stem=f'{profile}-{repeat+1}'
        report=a.output/(stem+'.json')
        cmd=[str(a.binary.resolve()),'--profile',profile,'--mode',a.mode,'--report',str(report),'--seed','42']
        if profile in ['synthetic-50000','synthetic-100000']:cmd+=['--repetitions','20','--warmup','2']
        if a.mode=='durable':cmd+=['--db',str(a.output/(stem+'-db'))]
        start=time.monotonic();peak=0;reason=None
        with (a.output/(stem+'.log')).open('w') as log:
            proc=subprocess.Popen(cmd,stdout=log,stderr=subprocess.STDOUT)
            while proc.poll() is None:
                try:
                    status=Path(f'/proc/{proc.pid}/status').read_text()
                    rss=int(next(line.split()[1] for line in status.splitlines() if line.startswith('VmRSS:')))
                    peak=max(peak,rss)
                except (OSError,StopIteration):pass
                if peak>a.rss_mb*1024:reason='rss_limit'
                if time.monotonic()-start>a.timeout_s:reason='timeout'
                if reason:proc.kill();proc.wait();break
                time.sleep(.2)
        envelope={'command':cmd,'exit_code':proc.returncode,'reason':reason,'peak_rss_kb':peak,'elapsed_s':time.monotonic()-start,'completed_report':report.exists()}
        (a.output/(stem+'-run.json')).write_text(json.dumps(envelope,indent=2)+'\n')
        print(stem,envelope,flush=True)
