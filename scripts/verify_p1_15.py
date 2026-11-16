#!/usr/bin/env python3
"""P1-15 final-state gate: the complete mandatory offline suite plus the T15 scenario.

Runs everything the P1-14 gate runs (network denial, boundary guard and regression,
formatting, full Rust regression, doctests, Clippy, Swift build, Swift T14 tests and the
runtime macOS GUI scenarios) under the same temporary credential-free HOME, then checks the
T15 inventory, the unchanged earlier inventory and the changed-file allowlist.
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
EARLIER_RUST_TESTS = 322
T15_TESTS = 7
SWIFT_TESTS = 27
GUI_SCENARIOS = 9
BOUNDARY_TESTS = 55
# P1-15 adds tests, a test-only fixture scenario, its gate and reports; nothing else.
ALLOWED = {'crates/jojo-storage/tests/t15_scenario.rs', 'crates/jojo-storage/src/bin/jojo-process-fixture.rs',
           'scripts/verify_p1_15.py', 'scripts/verify_p1_04.py', 'docs/verification/P1-15.md', 'README.md'}
STATE = {}


def digest(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def setup(env, temp, run):
    p09.setup(env, temp, run)
    cargo, swift = shutil.which('cargo'), shutil.which('swift')
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
    (ROOT / 'verification/p1-15-gui.json').write_text(json.dumps(gui, indent=2) + '\n')
    STATE.update(swift_test_names=names, gui=gui)


def t15(output, passed):
    baseline = json.loads((ROOT / 'verification/p1-14-result.json').read_text())
    if not baseline.get('passed') or baseline.get('p1_15') != 'GO':
        raise RuntimeError('P1-14 is not GO')
    t15_names = re.findall(r'^fn (t15_\w+)\(', (ROOT / 'crates/jojo-storage/tests/t15_scenario.rs').read_text(), re.M)
    if len(t15_names) != T15_TESTS or set(t15_names) - set(passed):
        raise RuntimeError(f'T15 inventory failed: {sorted(set(t15_names) - set(passed))}')
    if len(passed) != EARLIER_RUST_TESTS + T15_TESTS:
        raise RuntimeError(f'Expected {EARLIER_RUST_TESTS + T15_TESTS} Rust tests, observed {len(passed)}')
    ignored = re.findall(r'^test (\S+) \.\.\. ignored', output, re.M)
    if ignored != ['child_fixture']:
        raise RuntimeError(f'Unexpected ignored tests: {ignored}')
    old = (ROOT / 'verification/p1-14.log').read_text()
    old_names = set(re.findall(r'^test (\S+) \.\.\. ok$', old, re.M))
    if len(old_names) != EARLIER_RUST_TESTS or old_names - set(passed):
        raise RuntimeError('Earlier acceptance inventory missing or changed')
    before = json.loads((ROOT / 'verification/p1-15-before.json').read_text())
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
        raise RuntimeError(f'P1-15 changed files outside its allowlist: {outside}')
    git = subprocess.run(['git', 'status', '--short'], cwd=ROOT, capture_output=True, text=True)
    if git.returncode != 128 or 'not a git repository' not in git.stderr:
        raise RuntimeError('Git precondition changed; inspect actual state')
    return dict(t15_tests=len(t15_names), t15_test_names=t15_names, earlier_rust_tests=EARLIER_RUST_TESTS,
                swift_tests=SWIFT_TESTS, swift_test_names=STATE['swift_test_names'],
                gui_scenarios={k: v['passed'] for k, v in STATE['gui'].items()},
                gui_evidence='verification/p1-15-gui.json', ignored_tests=ignored,
                precondition={'p1_14': 'GO', 'evidence': 'verification/p1-14-result.json',
                              'source_match': 'all 158 P1-14 final hashes matched before the first P1-15 edit'},
                baseline_snapshot='verification/p1-15-before.json',
                git={'exit_code': git.returncode, 'stderr': git.stderr.strip()},
                files_changed=changed,
                production_rust_sources_changed=[p for p in changed if p.startswith('crates/') and '/src/' in p
                                                 and not p.endswith('src/bin/jojo-process-fixture.rs')],
                swift_sources_changed=[p for p in changed if p.startswith('macos/')],
                input_sha256=hashes, runtime_dependencies_added=[], test_dependencies_added=[],
                verification_report='docs/verification/P1-15.md', invariant_changes=[],
                limitations=['Review/evidence UI integration is asserted at the helper protocol boundary the SwiftUI shell consumes (ReviewBundle, HumanDecision, PrepareClose/CancelClose/ConfirmClose, Recovery); SwiftUI code is unchanged and re-verified by the P1-14 Swift and GUI suites.',
                             'The independent dependency observer is a test fixture that records an observation only after a PASSED validation receipt and human acceptance exist; Phase 1 has no production observer.',
                             'Selecting a validated output for a downstream step requires a human-reviewed plan revision naming the exact candidate digest and snapshot source.',
                             'Fresh attempts after reopen are provisioned and scheduled explicitly by the test after a reviewed recovery start; protocol 1.0 still has no Start command.'],
                token_efficiency='Reused the T11/T12 attempt helpers and the P1-13 helper service; focused T15 runs during development; full gate once at the end. Provider token counts unavailable; no model/provider invocation.')


if __name__ == '__main__':
    try:
        shared.main(task='P1-15', extra=t15, setup=setup,
                    scope='P1-15 only: offline multi-agent feature scenario over the existing foundation; no P1-16, live providers, UI migration, push or merge')
    finally:
        out = ROOT / 'verification/p1-15-result.json'
        result = json.loads(out.read_text())
        if result.get('passed') and result.get('boundary_tests') != BOUNDARY_TESTS:
            result.update(passed=False, p1_16='NO-GO', error=f"boundary inventory {result.get('boundary_tests')} != {BOUNDARY_TESTS}")
            out.write_text(json.dumps(result, indent=2) + '\n')
            sys.exit(1)
