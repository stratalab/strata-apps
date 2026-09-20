import unittest
from extract_subway import ride_pairs


class SubwayExtractionTest(unittest.TestCase):
    def test_excluded_borough_breaks_trip(self):
        parents = {'m1N': 'm1', 'q1N': 'q1', 'm2N': 'm2', 'm3N': 'm3'}
        stations = {'m1': 'first', 'm2': 'second', 'm3': 'third'}
        self.assertEqual(list(ride_pairs(['m1N', 'q1N', 'm2N', 'm3N'], parents, stations)), [('second', 'third')])

    def test_multiple_gtfs_ids_share_one_station(self):
        self.assertEqual(list(ride_pairs(['A12', 'D13', 'D14'], {}, {'A12': '151', 'D13': '151', 'D14': '152'})), [('151', '152')])


if __name__ == '__main__':
    unittest.main()
