"""R-03: the gate fails for each class of check it evaluates, and passes only a complete record."""
import copy
import importlib.util
import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('verify', ROOT / 'scripts/verify.py')
VERIFY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFY)

GATE = {
    'boundary_tests_min': 3,
    'rust': {'passed_min': 2, 'required': ['needed'], 'ignored': ['role']},
    'swift': {'tests_min': 2},
    'gui': {'scripts/gui_a.py': 2},
}
GOOD = {
    'boundary_tests': 3,
    'rust_passed': ['needed', 'other'], 'rust_failed': [], 'rust_ignored': ['role'],
    'swift_executed': 2, 'swift_failures': 0, 'swift_skipped': False,
    'gui': {'scripts/gui_a.py': {'one': {'passed': True}, 'two': {'passed': True}}},
}


class GateEvaluation(unittest.TestCase):
    def broken(self, edit):
        seen = copy.deepcopy(GOOD)
        edit(seen)
        return VERIFY.evaluate(GATE, seen)

    def test_a_complete_record_passes(self):
        self.assertEqual(VERIFY.evaluate(GATE, GOOD), [])

    def test_fewer_boundary_tests_fail(self):
        self.assertTrue(self.broken(lambda s: s.update(boundary_tests=2)))

    def test_a_failed_rust_test_fails(self):
        self.assertTrue(self.broken(lambda s: s['rust_failed'].append('other')))

    def test_fewer_rust_tests_fail(self):
        self.assertTrue(self.broken(lambda s: s.update(rust_passed=['needed'])))

    def test_a_required_rust_test_that_did_not_pass_fails(self):
        errors = self.broken(lambda s: s.update(rust_passed=['other', 'third']))
        self.assertIn("required Rust tests did not pass: ['needed']", errors)

    def test_an_unlisted_ignored_rust_test_fails(self):
        self.assertTrue(self.broken(lambda s: s['rust_ignored'].append('quietly_disabled')))

    def test_swift_failure_shortfall_or_skip_fails(self):
        self.assertTrue(self.broken(lambda s: s.update(swift_failures=1)))
        self.assertTrue(self.broken(lambda s: s.update(swift_executed=1)))
        self.assertTrue(self.broken(lambda s: s.update(swift_skipped=True)))

    def test_a_failed_or_missing_gui_scenario_fails(self):
        self.assertTrue(self.broken(lambda s: s['gui']['scripts/gui_a.py']['two'].update(passed=False)))
        self.assertTrue(self.broken(lambda s: s['gui']['scripts/gui_a.py'].pop('two')))
        self.assertTrue(self.broken(lambda s: s['gui'].clear()))

    def test_a_skipped_stage_is_not_evaluated_and_not_invented(self):
        seen = {k: v for k, v in GOOD.items() if not k.startswith('swift') and k != 'gui'}
        self.assertEqual(VERIFY.evaluate(GATE, seen), [])

    def test_the_current_gate_file_names_only_known_keys_and_real_scripts(self):
        gate = json.loads((ROOT / 'scripts/gates/current.json').read_text())
        self.assertLessEqual(set(gate), {'artifact', 'scope', 'boundary_tests_min', 'rust', 'swift', 'gui', 'not_covered'})
        for script in gate['gui']:
            self.assertTrue((ROOT / script).is_file(), script)
        self.assertLessEqual({i['test'] for i in gate['rust']['unsandboxed_ignored']}, set(gate['rust']['required']))


if __name__ == '__main__':
    unittest.main()
