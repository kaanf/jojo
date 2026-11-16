#!/usr/bin/env python3
"""P1-14 final-state gate: the complete mandatory offline suite plus native shell checks.

Adds to the P1-13 gate: Swift app/core/test build, T14 Swift model tests (fake helper and
real jojo-helper), and runtime macOS GUI scenarios on WindowServer, all under the same
OS network denial and temporary credential-free HOME.
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
SWIFT_TESTS = 27
GUI_SCENARIOS = 9
BOUNDARY_TESTS = 55
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
    (ROOT / 'verification/p1-14-gui.json').write_text(json.dumps(gui, indent=2) + '\n')
    STATE.update(swift_test_names=names, gui=gui)


def t14(output, passed):
    pre = json.loads((ROOT / 'verification/p1-14-precondition-result.json').read_text())
    if not pre['passed'] or pre['rust_tests'] != 322 or pre['boundary_tests'] != 50:
        raise RuntimeError('P1-13 baseline did not pass')
    if len(passed) != 322:
        raise RuntimeError(f'Expected 322 Rust tests, observed {len(passed)}')
    ignored = re.findall(r'^test (\S+) \.\.\. ignored', output, re.M)
    if ignored != ['child_fixture']:
        raise RuntimeError(f'Unexpected ignored tests: {ignored}')
    old = (ROOT / 'verification/p1-14-precondition.log').read_text()
    old_names = set(re.findall(r'^test (\S+) \.\.\. ok$', old, re.M))
    if len(old_names) != 322 or old_names - set(passed):
        raise RuntimeError('Earlier acceptance inventory missing or changed')
    before = json.loads((ROOT / 'verification/p1-14-before.json').read_text())
    files = [p for directory in ['crates', 'scripts', 'docs', 'macos', '.cargo'] for p in (ROOT / directory).rglob('*')
             if p.is_file() and not {'__pycache__', '.build', '.swiftpm'} & set(p.parts)]
    files += [ROOT / p for p in ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'README.md', '.gitignore', 'research/REPOSITORY_PINS.json']]
    hashes = {str(p.relative_to(ROOT)): digest(p) for p in files}
    removed = [p for p in before if not p.startswith('_') and p not in hashes]
    if removed:
        raise RuntimeError(f'Removed baseline files: {removed}')
    protected = ['research/REPOSITORY_PINS.json', 'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml',
                 'docs/planning/PHASE_1_IMPLEMENTATION_BRIEF.md', 'docs/roadmap/IMPLEMENTATION_ROADMAP.md',
                 'docs/architecture/ACCEPTED-DECISIONS-2026-11-14.md', 'docs/architecture/SHUTDOWN_RECOVERY.md',
                 'docs/architecture/STDIO_PROTOCOL.md', 'macos/JojoApp/Sources/JojoAppBoundary/JojoSceneBoundary.swift']
    for p in protected:
        if hashes[p] != before[p]:
            raise RuntimeError(f'Authoritative/protected input changed: {p}')
    changed = sorted(p for p, h in hashes.items() if h != before.get(p))
    rust = [p for p in changed if p.startswith(('crates/', '.cargo/'))]
    if rust:
        raise RuntimeError(f'P1-14 must not change Rust runner/helper sources: {rust}')
    git = subprocess.run(['git', 'status', '--short'], cwd=ROOT, capture_output=True, text=True)
    if git.returncode != 128 or 'not a git repository' not in git.stderr:
        raise RuntimeError('Git precondition changed; inspect actual state')
    return dict(swift_tests=SWIFT_TESTS, swift_test_names=STATE['swift_test_names'],
                gui_scenarios={k: v['passed'] for k, v in STATE['gui'].items()},
                gui_evidence='verification/p1-14-gui.json', ignored_tests=ignored,
                precondition={'passed': True, 'rust_tests': 322, 'boundary_tests': 50,
                              'evidence': 'verification/p1-14-precondition-result.json'},
                baseline_snapshot='verification/p1-14-before.json', baseline_p1_13_hash_match=True,
                git={'exit_code': git.returncode, 'stderr': git.stderr.strip()},
                files_changed=changed, rust_sources_changed=[], input_sha256=hashes,
                runtime_dependencies_added=[], test_dependencies_added=[],
                verification_report='docs/verification/P1-14.md', invariant_changes=[],
                limitations=['Protocol 1.0 has no Start or recovery-resolution command: active-work GUI scenarios use a test-only protocol fixture; recovery choices are displayed, not executable.',
                             'Runtime GUI steps invoke the real menu key equivalent, quit Apple event, performClose and button handlers in-process; no Accessibility-driven mouse clicks.',
                             'SHUTDOWN_FAILED leaves only Stay or an explicit unclean Disconnect and Quit; retry/escalation is not exposed by the helper.',
                             'Unbundled SwiftPM executable; no signed .app packaging or helper embedding.'],
                token_efficiency='Targeted reads of contracts and helper response code; one fake helper reused by unit and GUI tests; development runs used focused Swift/GUI subsets before the full gate. Provider token counts unavailable; no model/provider invocation.')


if __name__ == '__main__':
    try:
        shared.main(task='P1-14', extra=t14, setup=setup,
                    scope='P1-14 only: native SwiftUI/AppKit review and lifecycle shell over the P1-13 helper; no P1-15, live providers, push or merge')
    finally:
        # The shared runner cannot see the boundary inventory in `extra`; enforce it here.
        out = ROOT / 'verification/p1-14-result.json'
        result = json.loads(out.read_text())
        if result.get('passed') and result.get('boundary_tests') != BOUNDARY_TESTS:
            result.update(passed=False, p1_15='NO-GO', error=f"boundary inventory {result.get('boundary_tests')} != {BOUNDARY_TESTS}")
            out.write_text(json.dumps(result, indent=2) + '\n')
            sys.exit(1)
