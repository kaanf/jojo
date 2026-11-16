#!/usr/bin/env python3
"""P1-16 final-state gate: the complete mandatory offline suite plus the T16 campaign.

Runs everything the P1-15 gate runs (network denial, boundary guard and regression,
formatting, full Rust regression including T15, doctests, Clippy, Swift build, Swift T14
tests and the runtime macOS GUI scenarios) under the same temporary credential-free HOME,
then checks the T16 inventory, the unchanged earlier inventory and the changed-file
allowlist. Test targets are compiled in their own step so execution has the full budget.
"""
import hashlib
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path
import verify_p1_04 as shared
import verify_p1_09 as p09
ROOT = Path(__file__).resolve().parents[1]
EARLIER_RUST_TESTS = 329
T16_TESTS = 10
IGNORED = ['child_fixture', 't16_helper_role']
SWIFT_TESTS = 27
GUI_SCENARIOS = 9
BOUNDARY_TESTS = 55
# P1-16 adds the campaign, the shared scenario fixture (moved out of T15), a test-only
# hostile-output fixture scenario, its gate and reports; nothing else.
ALLOWED = {'crates/jojo-storage/tests/t16_campaign.rs', 'crates/jojo-storage/tests/common/scenario.rs',
           'crates/jojo-storage/tests/common/mod.rs', 'crates/jojo-storage/tests/t15_scenario.rs',
           'crates/jojo-storage/src/bin/jojo-process-fixture.rs', 'scripts/verify_p1_16.py',
           'docs/verification/P1-16.md', 'README.md'}
STATE = {}


def digest(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def setup(env, temp, run):
    p09.setup(env, temp, run)
    cargo, swift = shutil.which('cargo'), shutil.which('swift')
    run('Build all Rust test targets', [cargo, 'test', '--offline', '--locked', '--workspace', '--all-targets', '--no-run'])
    run('Helper binary for native tests', [cargo, 'build', '--offline', '--locked', '-p', 'jojo-helper'])
    helper = temp / 'target/debug/jojo-helper'
    env['JOJO_HELPER_BIN'] = str(helper)
    common = ['--package-path', 'macos/JojoApp', '--scratch-path', temp / 'swift-t14', '--cache-path', temp / 'swift-cache',
              '--disable-sandbox', '--disable-automatic-resolution']
    run('Swift app, core and test build', [swift, 'build', '--build-tests', *common])
    out = run('Swift T14 tests (fake and real helper)', [swift, 'test', '--skip-build', *common])
    summary = re.findall(r'Executed (\d+) tests?, with (\d+) failures', out)
    passed = set(re.findall(r"Test Case '-\[JojoCoreTests\.\w+ (\w+)\]' passed", out))
    names = [n for f in sorted((ROOT / 'macos/JojoApp/Tests/JojoCoreTests').glob('*.swift'))
             for n in re.findall(r'func (test_t14_\w+)\(', f.read_text())]
    if not summary or summary[-1] != (str(SWIFT_TESTS), '0') or len(names) != SWIFT_TESTS or set(names) - passed:
        raise RuntimeError(f'Swift T14 inventory failed: {summary[-1:]} missing={set(names) - passed}')
    if re.search(r'skipped', out, re.I):
        raise RuntimeError('A Swift T14 test was skipped; real-helper tests are mandatory')
    app = next((temp / 'swift-t14').rglob('Products/Debug/JojoApp'))
    report = temp / 'gui.json'
    run('Runtime macOS GUI scenarios', [sys.executable, 'scripts/gui_p1_14.py', '--app', app, '--helper', helper, '--out', report])
    gui = json.loads(report.read_text())
    if len(gui) != GUI_SCENARIOS or not all(r['passed'] for r in gui.values()):
        raise RuntimeError(f'GUI scenarios failed: {gui}')
    (ROOT / 'verification/p1-16-gui.json').write_text(json.dumps(gui, indent=2) + '\n')
    STATE.update(swift_test_names=names, gui=gui)


def t16(output, passed):
    baseline = json.loads((ROOT / 'verification/p1-15-result.json').read_text())
    if not baseline.get('passed') or baseline.get('p1_16') != 'GO':
        raise RuntimeError('P1-15 is not GO')
    t16_names = re.findall(r'^fn (t16_\w+)\(', (ROOT / 'crates/jojo-storage/tests/t16_campaign.rs').read_text(), re.M)
    t16_names = [n for n in t16_names if n != 't16_helper_role']
    if len(t16_names) != T16_TESTS or set(t16_names) - set(passed):
        raise RuntimeError(f'T16 inventory failed: {sorted(set(t16_names) - set(passed))}')
    if len(passed) != EARLIER_RUST_TESTS + T16_TESTS:
        raise RuntimeError(f'Expected {EARLIER_RUST_TESTS + T16_TESTS} Rust tests, observed {len(passed)}')
    ignored = sorted(re.findall(r'^test (\S+) \.\.\. ignored', output, re.M))
    if ignored != IGNORED:
        raise RuntimeError(f'Unexpected ignored tests: {ignored}')
    old = (ROOT / 'verification/p1-15.log').read_text()
    old_names = set(re.findall(r'^test (\S+) \.\.\. ok$', old, re.M))
    if len(old_names) != EARLIER_RUST_TESTS or old_names - set(passed):
        raise RuntimeError('Earlier acceptance inventory (including T15) missing or changed')
    before = json.loads((ROOT / 'verification/p1-16-before.json').read_text())
    files = [p for directory in ['crates', 'scripts', 'docs', 'macos', '.cargo'] for p in (ROOT / directory).rglob('*')
             if p.is_file() and not {'__pycache__', '.build', '.swiftpm'} & set(p.parts)]
    files += [ROOT / p for p in ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'README.md', '.gitignore', 'research/REPOSITORY_PINS.json']]
    hashes = {str(p.relative_to(ROOT)): digest(p) for p in files}
    removed = [p for p in before if not p.startswith('_') and p not in hashes]
    if removed:
        raise RuntimeError(f'Removed baseline files: {removed}')
    changed = sorted(p for p, h in hashes.items() if h != before.get(p))
    outside = [p for p in changed if p not in ALLOWED]
    if outside:
        raise RuntimeError(f'P1-16 changed files outside its allowlist: {outside}')
    git = subprocess.run(['git', 'status', '--short'], cwd=ROOT, capture_output=True, text=True)
    if git.returncode != 128 or 'not a git repository' not in git.stderr:
        raise RuntimeError('Git precondition changed; inspect actual state')
    return dict(t16_tests=len(t16_names), t16_test_names=t16_names, earlier_rust_tests=EARLIER_RUST_TESTS,
                swift_tests=SWIFT_TESTS, swift_test_names=STATE['swift_test_names'],
                gui_scenarios={k: v['passed'] for k, v in STATE['gui'].items()},
                gui_evidence='verification/p1-16-gui.json', ignored_tests=ignored,
                precondition={'p1_15': 'GO', 'evidence': 'verification/p1-15-result.json'},
                baseline_snapshot='verification/p1-16-before.json',
                git={'exit_code': git.returncode, 'stderr': git.stderr.strip()},
                files_changed=changed,
                production_rust_sources_changed=[p for p in changed if p.startswith('crates/') and '/src/' in p
                                                 and not p.endswith('src/bin/jojo-process-fixture.rs')],
                swift_sources_changed=[p for p in changed if p.startswith('macos/')],
                input_sha256=hashes, runtime_dependencies_added=[], test_dependencies_added=[],
                verification_report='docs/verification/P1-16.md', invariant_changes=[],
                limitations=['Disk failures are injected (spool write limits, read-only evidence directories, writer fail points); no real ENOSPC volume is filled.',
                             'Sleep/wake and reboot are simulated by clock jumps, a new clock epoch and scripted identity results; no physical reboot or sleep occurs.',
                             'Backup/restore is a cold copy of a stopped helper data root (database files plus evidence store); Phase 1 has no online backup command.',
                             'The database file inherits the umask; its privacy rests on the 0700 data root that prepare_data_root enforces.',
                             'Same-user hostile-code isolation is explicitly unsupported; the channel test proves provider output cannot act as a control command, not process containment.'],
                token_efficiency='Reused the T15 scenario fixture (moved to tests/common) and existing fault hooks; per-test runs under a hard alarm during development; full gate once at the end. Provider token counts unavailable; no model/provider invocation.')


if __name__ == '__main__':
    try:
        shared.main(task='P1-16', extra=t16, setup=setup,
                    scope='P1-16 only: crash, resource, durability and permission campaign over the integrated foundation; no P1-17, live providers, UI migration, push or merge')
    finally:
        out = ROOT / 'verification/p1-16-result.json'
        result = json.loads(out.read_text())
        if result.get('passed') and result.get('boundary_tests') != BOUNDARY_TESTS:
            result.update(passed=False, p1_17='NO-GO', error=f"boundary inventory {result.get('boundary_tests')} != {BOUNDARY_TESTS}")
            out.write_text(json.dumps(result, indent=2) + '\n')
            sys.exit(1)
