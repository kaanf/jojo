#!/usr/bin/env python3
"""P2-02 gate: production orchestration over helper protocol 1.1, offline.

Re-runs the complete suite (Rust, boundary, Swift, GUI) on the final tree under OS network
denial, then checks:

* every Phase 1 test recorded by the P1-17 gate and every P2-01 test still passes;
* the P2-02 inventory passes and covers every required case;
* the nine P1-14 GUI scenarios and the three P2-02 GUI scenarios pass;
* launch ledger: every process-launch site is classified, every provider executable named
  in a test is a fixture, and the runtime ledger of the P2-01 launches is unchanged;
* no source names the installed Codex CLI, its package, a Codex home or a credential store;
* files changed since the P2-01 gate are exactly the P2-02 set.

The installed Codex CLI is never located, read or executed by this gate or by any test.
"""
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path

import verify_p1_04 as shared
import verify_p1_09 as p09
import verify_p1_17 as p17
import verify_p2_01 as p21

ROOT = Path(__file__).resolve().parents[1]
PHASE_1_RUST_TESTS = 339
P2_01_RUST_TESTS = 20
P2_02_TESTS = {'crates/jojo-storage/tests/p2_02_slice.rs': 7, 'crates/jojo-helper/tests/p2_02_pipe.rs': 3,
               'crates/jojo-helper/src/lib.rs': 1}
SWIFT_TESTS = {'test_t14_': 27, 'test_p2_02_': 4}
GUI = {'scripts/gui_p1_14.py': 9, 'scripts/gui_p2_02.py': 3}
BOUNDARY_TESTS = 71
ALLOWED = {
    'crates/jojo-domain/src/ipc.rs',
    'crates/jojo-helper/src/lib.rs', 'crates/jojo-helper/src/main.rs', 'crates/jojo-helper/src/service.rs',
    'crates/jojo-helper/tests/p2_02_pipe.rs', 'crates/jojo-helper/tests/t02_helper_ownership.rs',
    'crates/jojo-helper/tests/t13_pipe.rs',
    'crates/jojo-storage/src/bin/jojo-codex-fixture.rs', 'crates/jojo-storage/src/codex.rs',
    'crates/jojo-storage/src/lib.rs', 'crates/jojo-storage/src/slice.rs', 'crates/jojo-storage/src/worktrees.rs',
    'crates/jojo-storage/tests/p2_02_slice.rs', 'crates/jojo-storage/tests/t02_crash.rs',
    'macos/JojoApp/Sources/JojoApp/AppDelegate.swift', 'macos/JojoApp/Sources/JojoApp/UITestDriver.swift',
    'macos/JojoApp/Sources/JojoApp/Views.swift', 'macos/JojoApp/Sources/JojoCore/Envelope.swift',
    'macos/JojoApp/Sources/JojoCore/HelperProcess.swift', 'macos/JojoApp/Sources/JojoCore/Workspace.swift',
    'macos/JojoApp/Tests/JojoCoreTests/P202StartRecoveryTests.swift',
    'scripts/check_p1_01_boundaries.py', 'scripts/gui_p2_02.py', 'scripts/tests/fake_helper_p2_02.py',
    'scripts/tests/test_boundaries.py', 'scripts/verify_p1_04.py', 'scripts/verify_p2_01.py',
    'scripts/verify_p2_02.py',
    'docs/architecture/STDIO_PROTOCOL.md', 'docs/planning/PHASE_2_VERTICAL_SLICE.md',
    'docs/verification/P2-02.md', 'README.md',
}
# The cases the P2-02 authorization requires, each mapped to tests that must pass.
COVERAGE = {
    'Slice driven by protocol commands only: register, import, approve, Start, supervised run, settlement': [
        'p2_02_protocol_only_slice_registers_imports_approves_starts_and_settles'],
    'Admission closed': [
        'p2_02_start_is_refused_without_admission_approval_configuration_or_exact_payload',
        'p2_02_real_pipe_refuses_start_until_reviewed_and_never_launches'],
    'Missing and stale approval; import never approves': [
        'p2_02_protocol_only_slice_registers_imports_approves_starts_and_settles',
        'p2_02_start_is_refused_without_admission_approval_configuration_or_exact_payload'],
    'Repository outside disposable root': [
        'p2_02_repository_outside_the_disposable_root_is_rejected',
        'p2_02_real_pipe_refuses_start_until_reviewed_and_never_launches'],
    'Duplicate Start, replayed command, lost Start response, capacity one': [
        'p2_02_duplicate_replayed_and_lost_start_never_launch_twice_and_capacity_stays_one',
        't13_receipt_after_dropped_response_duplicate_conflict_and_epoch_fences'],
    'Crash between command issue and spawn': [
        'p2_02_crash_between_issue_and_spawn_is_held_and_never_relaunched'],
    'Recovery after forced interruption; unknown stays Hold-only; unsafe continuation refused': [
        'p2_02_recovery_after_forced_interruption_needs_os_evidence_before_it_settles',
        't11_process_inspector_distinguishes_live_absent_reused_and_unknown'],
    'Incorrect provider adapter; no arbitrary executable or argv over IPC': [
        'p2_02_only_the_configured_codex_adapter_can_launch',
        'p2_02_slice_options_are_explicit_complete_and_only_for_serve'],
    'Protocol 1.1 is additive; 1.0 clients unchanged; no slice without configuration': [
        'p2_02_real_pipe_negotiates_1_1_and_keeps_1_0_clients_on_1_0',
        'p2_02_real_pipe_without_slice_options_offers_no_slice'],
    'Crash-test race fixed without weakening the assertion': [
        'a_helper_killed_inside_a_transaction_leaves_only_whole_committed_commands',
        'second_helper_is_denied_and_a_replacement_after_sigkill_starts_a_new_closed_epoch',
        't13_real_pipe_singleton_blocks_replacement_and_crash_has_no_ready'],
}
STATE = {}


def setup(env, temp, run):
    p09.setup(env, temp, run)
    cargo, swift = shutil.which('cargo'), shutil.which('swift')
    env['JOJO_P2_01_LEDGER'] = str(temp / 'p2-01-launch-ledger.tsv')
    run('Build all Rust test targets', [cargo, 'test', '--offline', '--locked', '--workspace', '--all-targets', '--no-run'])
    run('Helper binary for native tests', [cargo, 'build', '--offline', '--locked', '-p', 'jojo-helper'])
    helper = temp / 'target/debug/jojo-helper'
    env['JOJO_HELPER_BIN'] = str(helper)
    common = ['--package-path', 'macos/JojoApp', '--scratch-path', temp / 'swift-t14', '--cache-path', temp / 'swift-cache',
              '--disable-sandbox', '--disable-automatic-resolution']
    run('Swift app, core and test build', [swift, 'build', '--build-tests', *common])
    out = run('Swift tests (fake and real helper)', [swift, 'test', '--skip-build', *common])
    summary = re.findall(r'Executed (\d+) tests?, with (\d+) failures', out)
    swift_passed = set(re.findall(r"Test Case '-\[JojoCoreTests\.\w+ (\w+)\]' passed", out))
    sources = ''.join(f.read_text() for f in sorted((ROOT / 'macos/JojoApp/Tests/JojoCoreTests').glob('*.swift')))
    names = {prefix: re.findall(rf'func ({prefix}\w+)\(', sources) for prefix in SWIFT_TESTS}
    total = sum(SWIFT_TESTS.values())
    wrong = {p: len(n) for p, n in names.items() if len(n) != SWIFT_TESTS[p]}
    missing = {n for found in names.values() for n in found} - swift_passed
    if not summary or summary[-1] != (str(total), '0') or wrong or missing:
        raise RuntimeError(f'Swift inventory failed: {summary[-1:]} counts={wrong} missing={missing}')
    if re.search(r'skipped', out, re.I):
        raise RuntimeError('A Swift test was skipped; real-helper tests are mandatory')
    app = next((temp / 'swift-t14').rglob('Products/Debug/JojoApp'))
    gui = {}
    for script, count in GUI.items():
        report = temp / f'{Path(script).stem}.json'
        run(f'Runtime macOS GUI scenarios ({script})', [sys.executable, script, '--app', app, '--helper', helper, '--out', report])
        results = json.loads(report.read_text())
        if len(results) != count or not all(r['passed'] for r in results.values()):
            raise RuntimeError(f'GUI scenarios failed ({script}): {results}')
        gui.update(results)
    (ROOT / 'verification/p2-02-gui.json').write_text(json.dumps(gui, indent=2) + '\n')
    STATE.update(swift_test_names=names, gui=gui, temp=temp)
    p21.STATE.update(temp=temp)


def p2_02(output, passed):
    before = json.loads((ROOT / 'verification/p2-01-result.json').read_text())
    if not before.get('passed') or before.get('p2_02') != 'GO':
        raise RuntimeError('P2-01 is not GO')
    failures = []
    phase1 = set(re.findall(r'^test (\S+) \.\.\. ok$', (ROOT / 'verification/p1-17.log').read_text(), re.M))
    earlier = phase1 | set(before['p2_01_test_names'])
    if len(phase1) != PHASE_1_RUST_TESTS or len(before['p2_01_test_names']) != P2_01_RUST_TESTS or earlier - set(passed):
        failures.append(f'Phase 1 / P2-01 inventory missing or changed: {sorted(earlier - set(passed))}')
    names = {}
    for path, count in P2_02_TESTS.items():
        found = [n for n in re.findall(r'#\[test\]\s*fn (\w+)\(', (ROOT / path).read_text()) if n.startswith('p2_02_')]
        names[path] = found
        ran = {t.rsplit('::', 1)[-1] for t in passed}
        if len(found) != count or set(found) - ran:
            failures.append(f'{path}: expected {count} passing tests, missing {sorted(set(found) - ran)} of {len(found)}')
    new = [n for found in names.values() for n in found]
    expected = PHASE_1_RUST_TESTS + P2_01_RUST_TESTS + len(new)
    if len(passed) != expected:
        failures.append(f'expected {expected} Rust tests, observed {len(passed)}')
    ignored = sorted(re.findall(r'^test (\S+) \.\.\. ignored', output, re.M))
    if ignored != p21.IGNORED:
        failures.append(f'unexpected ignored tests {ignored}')
    ran = {t.rsplit('::', 1)[-1] for t in passed}
    coverage = {case: {t: t in ran for t in tests} for case, tests in COVERAGE.items()}
    missing = sorted({t for tests in coverage.values() for t, ok in tests.items() if not ok})
    uncovered = sorted(set(new) - {t for tests in COVERAGE.values() for t in tests})
    if missing or uncovered:
        failures.append(f'coverage: not passing {missing}; not mapped to a case {uncovered}')

    p17.LAUNCH_SITES = p17.LAUNCH_SITES + [
        (r'scripts/verify_p2_0[12]\.py', r"\['git'", 'read-only Git state check'),
        (r'scripts/gui_p2_02\.py', r'\[app,', 'built app under GUI test (helper: real jojo-helper or the protocol fixture)'),
    ]
    static = p17.launch_ledger()
    if static['unclassified'] or static['non_fixture_provider_executables'] or static['name_based_kill_tools']:
        failures.append(f"launch ledger: {static['unclassified']} {static['non_fixture_provider_executables']} {static['name_based_kill_tools']}")
    runtime = p21.runtime_ledger()
    if runtime['non_fixture_launches'] or runtime['scenarios'] != p21.LEDGER_SCENARIOS:
        failures.append(f"runtime launch ledger: foreign={runtime['non_fixture_launches']} scenarios={runtime['scenarios']}")
    p21.SCAN_EXEMPT.add('scripts/verify_p2_02.py')
    real = p21.real_binary_scan()
    if real:
        failures.append(f'source names the installed Codex CLI or its state: {real}')
    net = p17.adapters()
    if net['forbidden_crates'] or net['unreviewed_network_tokens']:
        failures.append(f"adapter scan: {net['forbidden_crates']} {net['unreviewed_network_tokens']}")
    link = p17.links()
    if link['broken']:
        failures.append(f"broken local links: {link['broken']}")
    s1 = p17.stage1()
    if not all(s1['documents'].values()) or not all(c['pinned'] and c['clean'] and c['licenses_unchanged'] for c in s1['clones']):
        failures.append('Stage 1 inputs or upstream checkouts changed')

    old = before['input_sha256']
    files = [p for directory in ['crates', 'scripts', 'docs', 'macos', '.cargo'] for p in (ROOT / directory).rglob('*')
             if p.is_file() and not {'__pycache__', '.build', '.swiftpm'} & set(p.parts)]
    files += [ROOT / p for p in ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'README.md', '.gitignore', 'research/REPOSITORY_PINS.json']]
    hashes = {str(p.relative_to(ROOT)): p21.digest(p) for p in files}
    removed = [p for p in old if p not in hashes]
    changed = sorted(p for p, h in hashes.items() if h != old.get(p))
    outside = [p for p in changed if p not in ALLOWED]
    if removed or outside:
        failures.append(f'P2-02 changed files outside its allowlist: removed={removed} outside={outside}')
    git = subprocess.run(['git', 'status', '--short'], cwd=ROOT, capture_output=True, text=True)
    if git.returncode != 128 or 'not a git repository' not in git.stderr:
        failures.append('Git precondition changed; inspect actual state')
    schema = max(int(v) for v in re.findall(r'^\s+version: (\d+),', (ROOT / 'crates/jojo-storage/src/schema.rs').read_text(), re.M))

    manifest = {
        'generated_by': 'scripts/verify_p2_02.py',
        'suite': {'rust_tests': len(passed), 'phase_1_rust_tests': len(phase1), 'p2_01_rust_tests': P2_01_RUST_TESTS,
                  'p2_02_tests': names, 'ignored': ignored, 'swift_tests': STATE['swift_test_names'],
                  'gui_scenarios': {k: v['passed'] for k, v in STATE['gui'].items()}},
        'schema': {'database_migrations': schema, 'stdio_protocol': '1.1 (1.0 clients unchanged)'},
        'coverage': coverage,
        'process_launches': static,
        'runtime_launch_ledger': runtime,
        'real_codex_references': real,
        'adapters': {k: v for k, v in net.items() if k != 'packages'},
        'links': link,
        'stage1': s1,
        'files_changed_since_p2_01': changed,
        'source_sha256': hashes,
        'failures': failures,
    }
    (ROOT / 'verification/p2-02-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    if failures:
        raise RuntimeError('; '.join(failures))
    return dict(phase_1_rust_tests=len(phase1), p2_01_rust_tests=P2_01_RUST_TESTS, p2_02_tests=len(new),
                p2_02_test_names=new, swift_tests=sum(SWIFT_TESTS.values()),
                gui_scenarios={k: v['passed'] for k, v in STATE['gui'].items()},
                gui_evidence='verification/p2-02-gui.json', ignored_tests=ignored,
                manifest='verification/p2-02-manifest.json', coverage_cases=len(COVERAGE),
                launch_sites=len(static['sites']), database_migrations=schema, real_codex_executed=False,
                files_changed=changed, input_sha256=hashes,
                git={'exit_code': git.returncode, 'stderr': git.stderr.strip()},
                verification_report='docs/verification/P2-02.md',
                live_attempt='NOT AUTHORIZED: G-LIVE decisions (U06, Codex home, usage limit, provider network) remain open')


if __name__ == '__main__':
    try:
        shared.main(task='P2-02', extra=p2_02, setup=setup,
                    scope='P2-02 only: protocol 1.1 registration, bundle import, Start and recovery resolution, '
                          'minimal SwiftUI controls; fixture executables only; no live Codex execution, credential '
                          'access, network inference, P2-03, push or merge')
    finally:
        out = ROOT / 'verification/p2-02-result.json'
        result = json.loads(out.read_text())
        if result.get('passed') and result.get('boundary_tests') != BOUNDARY_TESTS:
            result.update(passed=False, p2_03='NO-GO', error=f"boundary inventory {result.get('boundary_tests')} != {BOUNDARY_TESTS}")
            out.write_text(json.dumps(result, indent=2) + '\n')
            sys.exit(1)
