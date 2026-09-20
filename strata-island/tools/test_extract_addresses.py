#!/usr/bin/env python3
import unittest,json,math,hashlib
from extract_addresses import normalize,valid_bin,F
class Addresses(unittest.TestCase):
 def test_normalization_and_identity(self):
  self.assertEqual(normalize('230 W 55th St'), '230 west 55 street')
  self.assertEqual(normalize('350 Fifth Ave'), '350 5 avenue')
  self.assertEqual(normalize('94½ Greenwich Street'), '94 1/2 greenwich street')
  self.assertFalse(valid_bin('1000000'));self.assertTrue(valid_bin('1091925'))
 def test_fixture_accounting_and_attachments(self):
  data=json.loads((F/'addresses-v1.json').read_text());rs=data['addresses'];q=json.loads((F/'addresses-quality.json').read_text())
  self.assertEqual(q['raw_rows'],len(rs)+q['excluded_rows']+q['collapsed_duplicate_rows'])
  self.assertEqual(len(rs),len({r['id'] for r in rs}));self.assertEqual(len(rs),63103)
  roads=json.loads((F/'manhattan-drive.json').read_text());nodes={n['id']:n for n in roads['nodes']}
  named={(e['src'],normalize(e.get('name') or '')) for e in roads['edges']}|{(e['dst'],normalize(e.get('name') or '')) for e in roads['edges']}
  for r in rs:
   self.assertIn(r['source'].get('address_status'),(None,'','4'))
   self.assertEqual(r['house'],normalize(r['name'].removesuffix(' '+r['street'].title())))
   if r['node']:
    n=nodes[r['node']];self.assertLessEqual(math.hypot(r['x']-n['x'],r['y']-n['y']),200)
    self.assertIn((r['node'],r['street']),named);self.assertNotIn(r['zip'],('10044','10463'))
   if r['bin']:self.assertTrue(valid_bin(r['bin']))
  duplicate=next(r for r in rs if r['id']=='a:nyc:5217738');self.assertEqual(duplicate['bin'],'1091925');self.assertEqual(len(duplicate['source_rows']),2)
  self.assertEqual(data['road_sha256'],hashlib.sha256((F/'manhattan-drive.json').read_bytes()).hexdigest())
if __name__=='__main__':unittest.main()
