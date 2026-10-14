"""Pure parser tests; do NOT run the hardware collector here."""
import importlib.util
from pathlib import Path
import unittest
spec = importlib.util.spec_from_file_location('collector', Path(__file__).with_name('collect_hardware.py'))
c = importlib.util.module_from_spec(spec); spec.loader.exec_module(c)

class ParserTests(unittest.TestCase):
    def test_stat_handles_parentheses_and_spaces(self):
        fields = ['S'] + ['0']*30
        fields[11]='20'; fields[12]='3'; fields[19]='500'; fields[21]='7'
        result = c.parse_stat('123 (test ) name) '+' '.join(fields))
        self.assertEqual(result['comm'], 'test ) name')
        self.assertEqual((result['pid'], result['ticks'], result['start_ticks']), (123,23,500))
    def test_stat_short_is_error(self):
        with self.assertRaises(ValueError): c.parse_stat('1 (x) S 1')
    def test_stat_invalid_is_error(self):
        with self.assertRaises(ValueError): c.parse_stat('not a process')
    def test_unit_pids(self):
        self.assertEqual(c.service_pids('MainPID=12\nMainPID=0\nId=a.service\nMainPID=no\nMainPID=12\n'), {12})

if __name__ == '__main__': unittest.main()
