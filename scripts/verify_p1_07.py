#!/usr/bin/env python3
"""T01–T07 under the existing credential-free, OS-network-denied offline runner."""
import hashlib
import json
import re
from pathlib import Path
import verify_p1_04 as shared

ROOT = Path(__file__).resolve().parents[1]


def t07(output, passed):
    pre = json.loads((ROOT / 'verification/p1-07-precondition-result.json').read_text())
    if not pre['passed'] or pre['task'] != 'P1-06' or pre['rust_tests'] != 154 or pre['t06_total'] != 31 or pre['boundary_tests'] != 26:
        raise RuntimeError('P1-06 executable prerequisite inventory failed')
    names = re.findall(r'#\[test\]\s*fn (\w+)\(', (ROOT / 'crates/jojo-storage/tests/t07_worktrees.rs').read_text())
    sql = ['schema::tests::worktree_ownership_lifecycle_and_candidates_are_immutable_in_sql']
    if len(names) != 38 or (set(names + sql) - set(passed)):
        raise RuntimeError(f'T07 missing/failed: {set(names + sql) - set(passed)}; inventory={len(names)}')
    regression = re.findall(r'#\[test\]\s*fn (\w+)\(', (ROOT / 'crates/jojo-storage/tests/t06_scheduler.rs').read_text())
    if len(regression) != 29 or set(regression) - set(passed):
        raise RuntimeError('T06 scheduler regression failed')
    if len(passed) != 193:
        raise RuntimeError(f'Expected 193 Rust tests, got {len(passed)}')
    ignored = re.findall(r'^test (\S+) \.\.\. ignored', output, re.M)
    if ignored != ['child_fixture']:
        raise RuntimeError(f'Unexpected ignored tests: {ignored}')
    files = [p for directory in ['crates', 'scripts', 'docs'] for p in (ROOT / directory).rglob('*') if p.is_file() and p.suffix in {'.rs', '.py', '.md', '.toml'}]
    before = json.loads((ROOT / 'verification/p1-07-before.json').read_text())
    hashes = {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in files}
    changed = sorted(p for p, digest in hashes.items() if digest != before.get(p))
    return {
        't07_integration_tests': len(names), 't07_sql_tests': len(sql), 't07_total': len(names) + len(sql),
        't07_test_names': names, 't06_total': 31, 'ignored_tests': ignored, 'schema_version': 6,
        'files_changed': changed, 'input_sha256': {p: hashes[p] for p in changed},
        'precondition': {'task': 'P1-06', 'passed': True, 'rust_tests': 154, 't06_total': 31, 'boundary_tests': 26},
        'verification_report': 'docs/verification/P1-07.md',
        'invariant_changes': [],
        'known_limitations': [
            'Cooperative local coordination, not an OS sandbox against hostile same-user filesystem mutation.',
            'Partial creation and old-epoch bindings remain blocked/retained; no automatic adoption, pruning or recovery resolution.',
            'Snapshots are Git file-state packets, bounded to 16 MiB, not general filesystem backups; ignored files, submodules, special files, unsupported index flags and external symlinks block capture.',
            'No provider execution, dispatcher, process supervisor, remote operations or SwiftUI changes.',
            'Snapshot collection requires absence of private writer claims; process-death evidence and reviewed recovery/continuation orchestration remain later milestones.',
            'Uses /usr/bin/git on the macOS/Unix target; Git process deadlines and streaming belong to process-management work.',
        ],
    }


if __name__ == '__main__':
    shared.main(task='P1-07', extra=t07,
                scope='P1-07 only; local Git/worktree ownership, candidate packets and materialization; no provider execution or remote operations')
