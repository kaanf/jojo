#!/usr/bin/env python3
"""P1-12 completion gate: all regressions, credential-free and OS-network-denied."""
import hashlib
import json
import platform
import re
import tomllib
from pathlib import Path

import verify_p1_04 as shared
import verify_p1_09 as p09

ROOT = Path(__file__).resolve().parents[1]
EXPECTED_RUST = 302


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def t12(output, passed):
    pre = json.loads((ROOT / 'verification/p1-12-precondition-result.json').read_text())
    if not pre['passed'] or pre['task'] != 'P1-11' or pre['rust_tests'] != 287 or pre['boundary_tests'] != 44:
        raise RuntimeError('P1-11 prerequisite inventory failed')
    integration = re.findall(r'#\[test\]\s*fn (\w+)\(', (ROOT / 'crates/jojo-storage/tests/t12_shutdown.rs').read_text())
    domain = ['shutdown::tests::' + n for n in
              re.findall(r'#\[test\]\s*fn (\w+)\(', (ROOT / 'crates/jojo-domain/src/shutdown.rs').read_text())]
    unit = ['schema::tests::close_tickets_gate_admission_and_clean_receipts_are_guarded_in_sql']
    names = integration + domain + unit
    if len(integration) != 10 or len(domain) != 4 or set(names) - set(passed):
        raise RuntimeError(f'T12 inventory failed: {set(names) - set(passed)}; total={len(names)}')
    t11 = re.findall(r'#\[test\]\s*fn (\w+)\(', (ROOT / 'crates/jojo-storage/tests/t11_recovery.rs').read_text())
    if len(t11) != 8 or set(t11) - set(passed):
        raise RuntimeError('T11 regression inventory changed after moving its fixture')
    if len(passed) != EXPECTED_RUST:
        raise RuntimeError(f'Expected {EXPECTED_RUST} Rust tests, got {len(passed)}')
    ignored = re.findall(r'^test (\S+) \.\.\. ignored', output, re.M)
    if ignored != ['child_fixture']:
        raise RuntimeError(f'Unexpected ignored tests: {ignored}')
    before = json.loads((ROOT / 'verification/p1-12-before.json').read_text())
    files = [p for directory in ['crates', 'scripts', 'docs'] for p in (ROOT / directory).rglob('*')
             if p.is_file() and p.suffix in {'.rs', '.py', '.md', '.toml'} and '__pycache__' not in p.parts]
    files += [ROOT / p for p in ['Cargo.toml', 'Cargo.lock', 'research/REPOSITORY_PINS.json', 'README.md']]
    hashes = {str(p.relative_to(ROOT)): digest(p) for p in files}
    changed = sorted(p for p, sha in hashes.items() if sha != before.get(p))
    removed = sorted(p for p in before if not p.startswith('_') and p not in hashes)
    if 'Cargo.lock' in changed or any(p.endswith('Cargo.toml') for p in changed):
        raise RuntimeError('P1-12 must not change manifests or the lockfile')
    if 'research/REPOSITORY_PINS.json' in changed or removed:
        raise RuntimeError(f'P1-12 must not change upstream pins or remove files: {removed}')
    return dict(platform=platform.platform(), rust_toolchain=tomllib.loads((ROOT/'rust-toolchain.toml').read_text())['toolchain'],
                t12_total=len(names), t12_integration_tests=len(integration), t12_domain_tests=len(domain),
                t12_unit_tests=len(unit), t12_test_names=names, ignored_tests=ignored, schema_version=10,
                provider_schema_version=1,
                precondition={'task': 'P1-11', 'passed': True, 'rust_tests': 287, 'boundary_tests': 44,
                              'evidence': 'verification/p1-12-precondition-result.json; preserved P1-11 completion result'},
                baseline_note=before['_note'], files_changed=changed, input_sha256={p: hashes[p] for p in changed},
                verification_report='docs/verification/P1-12.md', invariant_changes=[], runtime_dependencies_added=[],
                inspector_fixture=p09.SETUP,
                known_limitations=[
                    'The coordinator is a storage-level library driven by the caller; the helper worker pool, stdio EOF detection and ShutdownReady transport/exit handshake belong to P1-13/P1-14.',
                    'Stop escalation is performed by each P1-09 runner on its own lifetime handle; a target whose runner already reported STOP_UNCONFIRMED (or whose runner died) cannot be re-stopped by retry and stays failed until reopen inspection (P1-11) proves absence.',
                    'Any live claim blocks READY, including QUARANTINED claims of earlier-epoch work kept on Hold; such a close commits FAILED by the unchanged P1-02 clean-shutdown rule.',
                    'An earlier-epoch live orphan is never signalled (no lifetime handle); it fails the close rather than being stopped.',
                    'A disconnect during a user-confirmed close is recorded only by the EMERGENCY receipt (finish_close(disconnected=true)), not by a separate ticket.',
                    'Reserved-but-unissued work retired by shutdown keeps its queue item with intent HOLD (existing P1-06 retirement rule); reopening requires a human resume, never automatic dispatch.',
                    'Process groups are not hostile-code containment; descendants that escaped unrecorded are reported as unconfirmed only when they hold a stream open.',
                    'Immutability is application policy plus SQL triggers; a same-user hostile process can still alter files or the database.'])


if __name__ == '__main__':
    shared.main(task='P1-12', extra=t12, setup=p09.setup,
                scope='P1-12 only; close tickets, layered close gate over admission, confirm/cancel/emergency stop, shutdown target freezing, receipt-based READY/FAILED/EMERGENCY finalization; no IPC, GUI, live provider, remote effects, push or merge')
