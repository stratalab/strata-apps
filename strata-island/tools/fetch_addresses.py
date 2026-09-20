#!/usr/bin/env python3
"""Explicit acquisition of a pinned NYC Manhattan AddressPoint export.
Never refresh an existing pin: new source data requires a new catalog revision.
"""
import hashlib, json, urllib.request, urllib.parse, datetime, tempfile, shutil
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / 'fixtures/address-source'
BASE = 'https://data.cityofnewyork.us'
TABLE = 'uf93-f8nk'
def get(url):
    with urllib.request.urlopen(url, timeout=120) as r: return r.read()
def query(**params):
    return BASE+'/resource/'+TABLE+'.json?'+urllib.parse.urlencode(params)
def main():
    if DEST.exists(): raise SystemExit('Pinned source already exists; use a new catalog revision to refresh.')
    meta_url = f'{BASE}/api/views/{TABLE}.json'
    count_url = query(**{'$select':'count(*) as rows,count(distinct objectid) as ids', '$where':"boroughcode='1'"})
    meta = get(meta_url); before = json.loads(meta); count = json.loads(get(count_url))[0]
    assert count['rows'] == count['ids'], 'source objectid must uniquely order rows'
    url = query(**{'$where':"boroughcode='1'", '$order':'objectid', '$limit':int(count['rows'])+1})
    raw = get(url); rows = json.loads(raw)
    assert len(rows) == int(count['rows']) and len({r['objectid'] for r in rows}) == len(rows)
    assert before['rowsUpdatedAt'] == json.loads(get(meta_url))['rowsUpdatedAt']
    assert count == json.loads(get(count_url))[0], 'source changed during acquisition'
    domains_url = 'https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/AddressPoint_view/FeatureServer/0?f=pjson'
    dictionary_url = f'{BASE}/api/views/{TABLE}/files/8e7624d3-c34c-4fe8-b049-6328fee361e8?download=true&filename=AddressPoint.pdf'
    files = {'rows.json':(url,raw),'metadata.json':(meta_url,meta),'domains.json':(domains_url,get(domains_url)), 'AddressPoint.pdf':(dictionary_url,get(dictionary_url))}
    with tempfile.TemporaryDirectory(dir=ROOT/'fixtures') as temp:
        pin=Path(temp)/'pin';pin.mkdir()
        for name,(_,data) in files.items(): (pin/name).write_bytes(data)
        manifest={'dataset':TABLE,'borough':'1','retrieved_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'rows_updated_at':before['rowsUpdatedAt'],'rows':len(rows),'files':{k:{'url':u,'sha256':hashlib.sha256(b).hexdigest()} for k,(u,b) in files.items()}}
        (pin/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n');shutil.move(pin,DEST)
    print(f'Pinned {len(rows):,} Manhattan source records')
if __name__=='__main__': main()
