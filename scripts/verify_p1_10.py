#!/usr/bin/env python3
"""P1-10 completion gate: all regressions, credential-free and OS-network-denied."""
import hashlib
import json
import platform
import re
import tomllib
from pathlib import Path

import verify_p1_04 as shared
import verify_p1_09 as p09

ROOT = Path(__file__).resolve().parents[1]
EXPECTED_RUST = 272


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def t10(output, passed):
    pre = json.loads((ROOT / 'verification/p1-10-precondition-result.json').read_text())
    if not pre['passed'] or pre['task'] != 'P1-09' or pre['rust_tests'] != 244 or pre['boundary_tests'] != 38:
        raise RuntimeError('P1-09 prerequisite inventory failed')
    paths = ['crates/jojo-domain/tests/t10_outcomes.rs', 'crates/jojo-storage/tests/t10_results.rs']
    names = [name for path in paths for name in re.findall(r'#\[test\]\s*fn (\w+)\(', (ROOT / path).read_text())]
    unit = ['schema::tests::results_evidence_effects_and_validation_are_guarded_and_immutable_in_sql',
            'outcomes::tests::stored_terminal_and_exit_text_parse_exactly_and_unknown_shapes_never_succeed']
    if len(names) != 26 or set(names + unit) - set(passed):
        raise RuntimeError(f'T10 inventory failed: {set(names + unit) - set(passed)}; total={len(names)}')
    if len(passed) != EXPECTED_RUST:
        raise RuntimeError(f'Expected {EXPECTED_RUST} Rust tests, got {len(passed)}')
    ignored = re.findall(r'^test (\S+) \.\.\. ignored', output, re.M)
    if ignored != ['child_fixture']:
        raise RuntimeError(f'Unexpected ignored tests: {ignored}')
    pin = next(p for p in json.loads((ROOT / 'research/REPOSITORY_PINS.json').read_text()) if p['name'] == 'Vigla')
    if pin['commit'] != 'bbd19ae2d5a77401502756c550b5dcd4ae59bbc9':
        raise RuntimeError('Unexpected upstream pin')
    upstream = ROOT / 'research/upstream/Vigla'
    for source in pin['licenses'] + pin['p1_09_review']['sources'] + pin['p1_10_review']['sources']:
        if digest(upstream / source['path']) != source['sha256']:
            raise RuntimeError(f'Upstream content drift: {source["path"]}')
    before = json.loads((ROOT / 'verification/p1-10-before.json').read_text())
    files = [p for directory in ['crates', 'scripts', 'docs'] for p in (ROOT / directory).rglob('*')
             if p.is_file() and p.suffix in {'.rs', '.py', '.md', '.toml'}]
    files += [ROOT / p for p in ['Cargo.toml', 'Cargo.lock', 'research/REPOSITORY_PINS.json', 'README.md']]
    hashes = {str(p.relative_to(ROOT)): digest(p) for p in files}
    changed = sorted(p for p, sha in hashes.items() if sha != before.get(p))
    if 'Cargo.lock' in changed or any(p.endswith('Cargo.toml') for p in changed):
        raise RuntimeError('P1-10 must not change manifests or the lockfile')
    return dict(platform=platform.platform(), rust_toolchain=tomllib.loads((ROOT/'rust-toolchain.toml').read_text())['toolchain'],
                t10_total=len(names) + len(unit), t10_domain_tests=9, t10_integration_tests=17, t10_unit_tests=len(unit),
                t10_test_names=names + unit, ignored_tests=ignored, schema_version=8, provider_schema_version=1,
                precondition={'task': 'P1-09', 'passed': True, 'rust_tests': 244, 'boundary_tests': 38,
                              'evidence': 'verification/p1-10-precondition-result.json; preserved P1-09 completion result'},
                baseline_note=before['_note'], files_changed=changed, input_sha256={p: hashes[p] for p in changed},
                verification_report='docs/verification/P1-10.md', invariant_changes=[], runtime_dependencies_added=[],
                upstream_pin=pin, inspector_fixture=p09.SETUP,
                known_limitations=[
                    'Evidence/result immutability is application policy, SQL triggers and digests; a same-user hostile process can still alter files or the database.',
                    'Blob verification precedes the sealing transaction; a same-user change between verification and commit is not prevented.',
                    'Changed-path scope covers repository content only; writes outside the worktree, reads, and effects outside the effect journal are not observed.',
                    'Validator reports enter through a typed trusted host entry point; running the validator process itself is P1-15 orchestration.',
                    'Remote observations are fakes only; no remote mutation adapter exists.',
                    'Recovery holds (lost coverage, unknown effects, unconfirmed stops, old-epoch attempts) are recorded but resolved only by P1-11.',
                    'Acceptance requires SUCCEEDED invocation plus a fresh passing receipt; feeding accepted outputs into dependency predicates is later integration work.'])


if __name__ == '__main__':
    shared.main(task='P1-10', extra=t10, setup=p09.setup,
                scope='P1-10 only; ordered settlement into immutable results, content-addressed evidence, attempt effect journal, independent validation receipts and separate acceptance; no live provider, P1-11 recovery, remote effects, push or merge')
