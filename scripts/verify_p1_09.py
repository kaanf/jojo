#!/usr/bin/env python3
"""P1-09 completion gate: all regressions, credential-free and OS-network-denied."""
import hashlib
import json
import platform
import re
import shutil
import stat
import sys
import tomllib
from pathlib import Path

import verify_p1_04 as shared

ROOT = Path(__file__).resolve().parents[1]
SETUP = {}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def setup(env, temp, run):
    # macOS forbids exec of setuid /bin/ps inside sandbox-exec. Preserve network
    # denial: copy exactly the host binary, remove privilege bits, then replace
    # the platform signature with a local ad-hoc signature. No download/build,
    # privileged process, daemon, host-side inspection or fixture output shim.
    original = Path('/bin/ps')
    target = temp / 'ps-unprivileged'
    shutil.copyfile(original, target)
    assert digest(target) == digest(original)
    target.chmod(0o700)
    assert stat.S_IMODE(target.stat().st_mode) == 0o700
    run('Sign unprivileged host ps fixture', ['/usr/bin/codesign', '--force', '--sign', '-', target])
    env['JOJO_TEST_PS'] = str(target)
    run('Unprivileged inspector under OS network denial', [sys.executable, '-c',
        'import subprocess,sys; p=subprocess.run([sys.argv[1],"-axo","pid=,ppid=,pgid=,stat=,lstart="],capture_output=True,text=True); '
        'assert p.returncode == 0, p.stderr; rows=p.stdout.splitlines(); '
        'assert rows and all(len(r.split()) == 9 for r in rows); print("real OS process inspection passed")', target])
    SETUP.update(host_ps_sha256=digest(original), signed_fixture_sha256=digest(target),
                 fixture_mode='0700; no setuid/setgid; temporary; same host ps with ad-hoc signature',
                 profile='(version 1) (allow default) (deny network*)')


def t09(output, passed):
    pre = json.loads((ROOT / 'verification/p1-09-precondition-result.json').read_text())
    if not pre['passed'] or pre['task'] != 'P1-08' or pre['rust_tests'] != 209 or pre['boundary_tests'] != 30:
        raise RuntimeError('P1-08 prerequisite inventory failed')
    paths = ['crates/jojo-domain/tests/t09_streams.rs', 'crates/jojo-storage/tests/t09_process.rs']
    names = [name for path in paths for name in re.findall(r'#\[test\]\s*fn (\w+)\(', (ROOT / path).read_text())]
    sql = ['schema::tests::process_history_follows_issue_boundary_durable_bytes_and_is_immutable_in_sql']
    if len(names) != 34 or set(names + sql) - set(passed):
        raise RuntimeError(f'T09 inventory failed: {set(names + sql) - set(passed)}; total={len(names)}')
    if len(passed) != 244:
        raise RuntimeError(f'Expected 244 Rust tests, got {len(passed)}')
    ignored = re.findall(r'^test (\S+) \.\.\. ignored', output, re.M)
    if ignored != ['child_fixture']:
        raise RuntimeError(f'Unexpected ignored tests: {ignored}')
    pin = next(p for p in json.loads((ROOT / 'research/REPOSITORY_PINS.json').read_text()) if p['name'] == 'Vigla')
    if pin['commit'] != 'bbd19ae2d5a77401502756c550b5dcd4ae59bbc9':
        raise RuntimeError('Unexpected upstream pin')
    upstream = ROOT / 'research/upstream/Vigla'
    for source in pin['licenses'] + pin['p1_09_review']['sources']:
        if digest(upstream / source['path']) != source['sha256']:
            raise RuntimeError(f'Upstream content drift: {source["path"]}')
    before = json.loads((ROOT / 'verification/p1-09-before.json').read_text())
    files = [p for directory in ['crates', 'scripts', 'docs'] for p in (ROOT / directory).rglob('*')
             if p.is_file() and p.suffix in {'.rs', '.py', '.md', '.toml'}]
    files += [ROOT / p for p in ['Cargo.toml', 'Cargo.lock', 'research/REPOSITORY_PINS.json', 'README.md']]
    hashes = {str(p.relative_to(ROOT)): digest(p) for p in files}
    changed = sorted(p for p, sha in hashes.items() if sha != before.get(p))
    return dict(platform=platform.platform(), rust_toolchain=tomllib.loads((ROOT/'rust-toolchain.toml').read_text())['toolchain'],
                t09_total=len(names) + len(sql), t09_process_tests=25, t09_parser_tests=9, t09_sql_tests=1,
                t09_test_names=names + sql, ignored_tests=ignored, schema_version=7, provider_schema_version=1,
                precondition={'task':'P1-08','passed':True,'rust_tests':209,'boundary_tests':30,
                              'evidence':'verification/p1-09-precondition-result.json; preserved historical P1-08 completion'},
                baseline_note=before['_note'], files_changed=changed, input_sha256={p: hashes[p] for p in changed},
                verification_report='docs/verification/P1-09.md', invariant_changes=[], runtime_dependencies_added=[],
                upstream_pin=pin, inspector_fixture=SETUP,
                known_limitations=[
                    'macOS/Unix, trusted local processes; process groups do not contain hostile or invisible escaped descendants.',
                    'Snapshot-only PIDs and escaped descendants are never signalled; unknown stops retain claims for later recovery.',
                    'Reader threads can remain blocked on an unknown inherited pipe until its holder exits; memory and channel sizes stay bounded.',
                    'Synthetic protocol profiles only; no installed Claude/Codex compatibility or subscription certification.',
                    'Outcome finalization/independent validation are P1-10; reopen and shutdown coordination remain later milestones.',
                    'No B6-B8 CPU/RSS/latency benchmark certification; flood/stop tests are functional regression gates.'])


if __name__ == '__main__':
    shared.main(task='P1-09', extra=t09, setup=setup,
                scope='P1-09 only; local fixture processes, bounded durable raw streams, process identity/stop observations; no live provider, P1-10, remote effects, push or merge')
