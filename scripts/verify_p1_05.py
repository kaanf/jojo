#!/usr/bin/env python3
"""T01–T05 through the existing clean-home, network-denied validation runner."""
import hashlib
import json
import re
from pathlib import Path
import verify_p1_04 as shared

ROOT = Path(__file__).resolve().parents[1]

def t05(output, passed):
    baseline = json.loads((ROOT/'verification/p1-05-precondition-result.json').read_text())
    if not baseline['passed'] or baseline['rust_tests'] != 94 or baseline['t04_tests'] != 16 or baseline['boundary_tests'] != 26:
        raise RuntimeError('P1-04 prerequisite differs from the verified baseline')
    counts = {}
    for crate in ['jojo-domain', 'jojo-storage']:
        source = ROOT/f'crates/{crate}/tests/t05_dependencies.rs'
        names = re.findall(r'#\[test\]\s*fn (\w+)\(', source.read_text())
        missing = set(names) - set(passed)
        if missing: raise RuntimeError(f'T05 tests missing or failed: {sorted(missing)}')
        counts[crate] = len(names)
    if counts != {'jojo-domain': 14, 'jojo-storage': 14}:
        raise RuntimeError(f'T05 inventory changed: {counts}')
    sql = 'schema::tests::dependency_history_is_immutable_and_graph_foreign_keys_refuse_unknown_endpoints'
    if sql not in passed: raise RuntimeError('T05 database immutability/FK test did not pass')
    before = json.loads((ROOT/'verification/p1-05-before.json').read_text())
    files = [p for directory in ['crates','scripts','docs/verification'] for p in (ROOT/directory).rglob('*') if p.is_file() and p.suffix in {'.rs','.py','.md','.toml'}]
    changed = sorted(str(p.relative_to(ROOT)) for p in files if hashlib.sha256(p.read_bytes()).hexdigest() != before.get(str(p.relative_to(ROOT))))
    ignored = re.findall(r'^test (\S+) \.\.\. ignored', output, re.M)
    if ignored != ['child_fixture']: raise RuntimeError(f'Unexpected ignored tests: {ignored}')
    return {'t05_integration_tests': counts, 't05_sql_tests': 1, 't05_total': sum(counts.values())+1,
            'ignored_tests': ignored, 'files_changed': changed, 'schema_version': 4,
            'precondition': {'passed': True, 'rust_tests':94, 'boundary_tests':26},
            'input_sha256': {str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in files if p.suffix != '.md'}}

if __name__ == '__main__':
    shared.main(task='P1-05', extra=t05)
