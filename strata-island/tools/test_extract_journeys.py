#!/usr/bin/env python3
import json,unittest
from pathlib import Path
from extract_journeys import allowed
class WalkingPolicy(unittest.TestCase):
 def test_pedestrian_access_is_independent_of_car_oneway(self):
  self.assertTrue(allowed({'highway':'residential','oneway':'yes'}))
  self.assertTrue(allowed({'highway':'footway'}))
  self.assertTrue(allowed({'highway':'steps'}))
  for t in [{'highway':'motorway'}, {'highway':'trunk'}, {'highway':'residential','foot':'no'}, {'highway':'footway','access':'private'}, {'highway':'footway','indoor':'yes'}, {'highway':'cycleway'}]: self.assertFalse(allowed(t),t)
  self.assertTrue(allowed({'highway':'cycleway','foot':'yes'}))
 def test_pinned_accounting_and_train_continuity(self):
  d=json.loads((Path(__file__).resolve().parents[1]/'fixtures/journeys-v1.json').read_text());nodes={n['id']:n for n in d['nodes']};patterns={p['id']:p for p in d['patterns']};edges=d['edges'];keys=set()
  for e in edges:
   key=(e['source'],e['kind'],e['target']);self.assertNotIn(key,keys);keys.add(key)
   self.assertIn(e['source'],nodes);self.assertIn(e['target'],nodes)
   if e['kind']=='ride':
    p=patterns[e['pattern']];pairs=list(zip(p['stations'],p['stations'][1:]));self.assertIn((e['from_station'],e['to_station']),pairs);self.assertEqual(nodes[e['source']]['pattern'],nodes[e['target']]['pattern'])
  self.assertEqual(len(d['stations']),151)
  self.assertEqual(len(d['station_anchors']),151)
if __name__=='__main__':unittest.main()
