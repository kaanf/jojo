#!/usr/bin/env python3
"""P1-11 completion gate: all regressions, credential-free and OS-network-denied."""
import hashlib
import json
import platform
import re
import tomllib
from pathlib import Path

import verify_p1_04 as shared
import verify_p1_09 as p09

ROOT = Path(__file__).resolve().parents[1]
EXPECTED_RUST = 287


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def t11(output, passed):
    pre = json.loads((ROOT / 'verification/p1-11-precondition-result.json').read_text())
    if not pre['passed'] or pre['task'] != 'P1-10' or pre['rust_tests'] != 272 or pre['boundary_tests'] != 42:
        raise RuntimeError('P1-10 prerequisite inventory failed')
    integration = re.findall(r'#\[test\]\s*fn (\w+)\(', (ROOT / 'crates/jojo-storage/tests/t11_recovery.rs').read_text())
    domain = ['recovery::tests::' + n for n in
              re.findall(r'#\[test\]\s*fn (\w+)\(', (ROOT / 'crates/jojo-domain/src/recovery.rs').read_text())]
    unit = ['schema::tests::recovery_proofs_resolutions_and_continuation_plans_are_guarded_in_sql',
            'recovery::tests::only_exact_round_tripping_session_payloads_parse']
    names = integration + domain + unit
    if len(integration) != 8 or len(domain) != 5 or set(names) - set(passed):
        raise RuntimeError(f'T11 inventory failed: {set(names) - set(passed)}; total={len(names)}')
    if len(passed) != EXPECTED_RUST:
        raise RuntimeError(f'Expected {EXPECTED_RUST} Rust tests, got {len(passed)}')
    ignored = re.findall(r'^test (\S+) \.\.\. ignored', output, re.M)
    if ignored != ['child_fixture']:
        raise RuntimeError(f'Unexpected ignored tests: {ignored}')
    before = json.loads((ROOT / 'verification/p1-11-before.json').read_text())
    files = [p for directory in ['crates', 'scripts', 'docs'] for p in (ROOT / directory).rglob('*')
             if p.is_file() and p.suffix in {'.rs', '.py', '.md', '.toml'} and '__pycache__' not in p.parts]
    files += [ROOT / p for p in ['Cargo.toml', 'Cargo.lock', 'research/REPOSITORY_PINS.json', 'README.md']]
    hashes = {str(p.relative_to(ROOT)): digest(p) for p in files}
    changed = sorted(p for p, sha in hashes.items() if sha != before.get(p))
    if 'Cargo.lock' in changed or any(p.endswith('Cargo.toml') for p in changed):
        raise RuntimeError('P1-11 must not change manifests or the lockfile')
    if 'research/REPOSITORY_PINS.json' in changed:
        raise RuntimeError('P1-11 must not change upstream pins')
    return dict(platform=platform.platform(), rust_toolchain=tomllib.loads((ROOT/'rust-toolchain.toml').read_text())['toolchain'],
                t11_total=len(names), t11_integration_tests=len(integration), t11_domain_tests=len(domain),
                t11_unit_tests=len(unit), t11_test_names=names, ignored_tests=ignored, schema_version=9,
                provider_schema_version=1,
                precondition={'task': 'P1-10', 'passed': True, 'rust_tests': 272, 'boundary_tests': 42,
                              'evidence': 'verification/p1-11-precondition-result.json; preserved P1-10 completion result'},
                baseline_note=before['_note'], files_changed=changed, input_sha256={p: hashes[p] for p in changed},
                verification_report='docs/verification/P1-11.md', invariant_changes=[], runtime_dependencies_added=[],
                inspector_fixture=p09.SETUP,
                known_limitations=[
                    'Helper death is simulated by dropping the engine without a clean shutdown at durable barriers; no T11 test SIGKILLs a real helper process (T02 covers OS-level helper kill).',
                    'A live orphan stays quarantined: the engine never signals a process without a retained lifetime handle, so stopping it before continuation needs OS evidence of its exit (or a later approved stop mechanism).',
                    'A launch issued without a registered process identity stays blocked: no recorded boot/process identity exists to prove absence, and a human confirmation is not accepted.',
                    'A reused PID blocks as specified even though the recorded root is gone; it resolves only after a later inspection finds the identity absent (for example after reboot).',
                    'Process-group inspection cannot see descendants that escaped into another group or session without being recorded.',
                    'Validate-output can produce a passed receipt for a recovered candidate, but acceptance still requires a SUCCEEDED invocation; using an interrupted candidate goes through continuation.',
                    'Continuation is limited to the same step revision and repository; materially changed remaining work still needs a new plan approval, which is not detected automatically.',
                    'Immutability is application policy plus SQL triggers and digests; a same-user hostile process can still alter files or the database.'])


if __name__ == '__main__':
    shared.main(task='P1-11', extra=t11, setup=p09.setup,
                scope='P1-11 only; paused reopen, read-only recovery inspection, quarantine, recovery resolutions, absence proofs, reviewed continuation plans and continuation launch binding; no live provider, P1-12 shutdown coordinator, remote effects, push or merge')
