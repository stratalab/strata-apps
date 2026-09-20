#!/usr/bin/env python3
"""Explicitly pin Manhattan OSM pedestrian inputs; never called by startup."""
import datetime, hashlib, json, urllib.request, urllib.parse
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]/'fixtures/walking-source'
QUERY='''[out:json][timeout:180];area(3608398124)->.a;way["highway"](area.a);(._;>;);out body;'''
def main():
    assert not (ROOT/'osm.json').exists(), 'Refusing to overwrite a pinned source'
    ROOT.mkdir(exist_ok=True)
    errors=[]
    for host in ['overpass-api.de','overpass.kumi.systems','overpass.private.coffee']:
        url=f'https://{host}/api/interpreter'
        try:
            req=urllib.request.Request(url, data=urllib.parse.urlencode({'data':QUERY}).encode(), headers={'User-Agent':'strata-island pedestrian graph demo'})
            with urllib.request.urlopen(req,timeout=210) as r:raw=r.read()
            data=json.loads(raw)
            assert data.get('elements') and not data.get('remark'),data.get('remark')
            (ROOT/'osm.json.tmp').write_bytes(raw)
            (ROOT/'osm.json.tmp').replace(ROOT/'osm.json')
            (ROOT/'manifest.json').write_text(json.dumps({'url':url,'query':QUERY,'retrieved_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'osm_base':data.get('osm3s',{}).get('timestamp_osm_base'),'sha256':hashlib.sha256(raw).hexdigest()},indent=2)+'\n')
            print(len(raw),'bytes',len(data['elements']),'elements');return
        except Exception as e:errors.append(str(e));print(url,e,flush=True)
    raise RuntimeError(errors)
if __name__=='__main__':main()
