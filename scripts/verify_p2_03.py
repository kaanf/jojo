#!/usr/bin/env python3
"""P2-03 gate: host validation runner, result inspection and human candidate acceptance.

Re-runs the complete suite (Rust, boundary, Swift, GUI) on the final tree under OS network
denial, then checks:

* every Phase 1, P2-01 and P2-02 test recorded by the earlier gates still passes;
* the P2-03 inventory passes and covers every required case;
* the twelve earlier GUI scenarios and the five P2-03 GUI scenarios pass;
* the real-helper end-to-end test passes. It is the one step run outside the OS sandbox,
  because the helper inspects processes with the system `ps`, which the sandbox cannot
  execute. It is offline: fixture executables only, and its validation command passes only
  if the validator's own sandbox denies it a socket;
* launch ledger, real-CLI scan, adapter scan, links and Stage 1 inputs as in P2-02;
* files changed since the P2-02 gate are exactly the P2-03 set.

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
import verify_p2_02 as p22

ROOT = Path(__file__).resolve().parents[1]
P2_03_TESTS = {'crates/jojo-storage/tests/p2_03_validation.rs': 10,
               'crates/jojo-domain/tests/p2_03_validation_plan.rs': 3}
REAL_HELPER_TEST = 'p2_03_real_helper_runs_start_to_settlement_validation_and_acceptance_over_pipes'
SWIFT_TESTS = {'test_t14_': 27, 'test_p2_02_': 4, 'test_p2_03_': 4}
GUI = {'scripts/gui_p1_14.py': 9, 'scripts/gui_p2_02.py': 3, 'scripts/gui_p2_03.py': 5}
BOUNDARY_TESTS = 75
ALLOWED = {
    'crates/jojo-domain/src/validation.rs', 'crates/jojo-domain/tests/p2_03_validation_plan.rs',
    'crates/jojo-domain/tests/t10_outcomes.rs',
    'crates/jojo-helper/src/service.rs',
    'crates/jojo-storage/src/evidence.rs', 'crates/jojo-storage/src/lib.rs', 'crates/jojo-storage/src/validation.rs',
    'crates/jojo-storage/src/validator.rs', 'crates/jojo-storage/src/worktrees.rs',
    'crates/jojo-storage/tests/common/attempts.rs', 'crates/jojo-storage/tests/p2_02_slice.rs',
    'crates/jojo-storage/tests/p2_03_real_helper.rs', 'crates/jojo-storage/tests/p2_03_validation.rs',
    'crates/jojo-storage/tests/t10_results.rs',
    'macos/JojoApp/Sources/JojoApp/UITestDriver.swift', 'macos/JojoApp/Sources/JojoApp/Views.swift',
    'macos/JojoApp/Sources/JojoCore/Workspace.swift',
    'macos/JojoApp/Tests/JojoCoreTests/P203CandidateReviewTests.swift',
    'scripts/check_p1_01_boundaries.py', 'scripts/gui_p2_03.py', 'scripts/tests/fake_helper_p2_03.py',
    'scripts/tests/test_boundaries.py', 'scripts/verify_p1_04.py', 'scripts/verify_p2_03.py',
    'docs/architecture/STDIO_PROTOCOL.md', 'docs/verification/P2-03.md',
}
# The cases the P2-03 authorization requires, each mapped to evidence that must pass:
# Rust or Swift tests by name, GUI scenarios as `gui:<scenario>`.
COVERAGE = {
    'Valid in-scope change passes host validation and is accepted by the human': [
        'p2_03_in_scope_change_passes_host_validation_and_is_accepted_by_the_human',
        'gui:fake_review_shows_not_run_unknown_and_pass_distinctly_and_accepts_only_a_pass'],
    'Out-of-scope file modification': [
        'p2_03_out_of_scope_write_fails_scope_and_the_command_is_never_run',
        'gui:fake_negative_results_are_visible_and_can_never_be_accepted'],
    'Modification of a protected baseline test': [
        'p2_03_protected_baseline_test_modification_fails_and_the_command_is_never_run',
        'gui:fake_negative_results_are_visible_and_can_never_be_accepted'],
    'Failing validation command': [
        'p2_03_failing_validation_command_fails_closed',
        'gui:fake_negative_results_are_visible_and_can_never_be_accepted'],
    'Hung validator process': [
        'p2_03_hung_validator_is_stopped_by_its_budget_and_fails_closed',
        'gui:fake_negative_results_are_visible_and_can_never_be_accepted'],
    'Tampered sealed candidate': [
        'p2_03_tampered_sealed_candidate_is_never_validated_or_accepted',
        'test_p2_03_uninspectable_candidate_and_lost_response_stay_unknown',
        'gui:fake_negative_results_are_visible_and_can_never_be_accepted'],
    'Stale approval': [
        'p2_03_stale_approval_refuses_validation_and_acceptance',
        'gui:fake_negative_results_are_visible_and_can_never_be_accepted'],
    'Duplicate validation request': [
        'p2_03_in_scope_change_passes_host_validation_and_is_accepted_by_the_human',
        'p2_03_closing_while_validation_is_active_stops_the_validator_and_leaves_no_verdict',
        'test_p2_03_validate_and_accept_bind_the_displayed_candidate_and_receipt'],
    'Lost command response': [
        'p2_03_in_scope_change_passes_host_validation_and_is_accepted_by_the_human',
        'test_p2_03_uninspectable_candidate_and_lost_response_stay_unknown',
        'gui:fake_lost_validate_response_is_reconciled_not_resent'],
    'Candidate changes after validation': [
        'p2_03_candidate_changed_after_validation_cannot_be_accepted'],
    'App closing while validation is active': [
        'p2_03_closing_while_validation_is_active_stops_the_validator_and_leaves_no_verdict',
        'gui:fake_quit_while_validation_is_active_closes_through_the_helper_with_no_result'],
    'A provider saying "tests passed" never produces a successful validation state': [
        'p2_03_in_scope_change_passes_host_validation_and_is_accepted_by_the_human',
        'test_p2_03_provider_claim_of_passing_tests_never_produces_a_pass_or_an_accept',
        'test_p2_03_four_states_are_distinct_and_only_a_recorded_pass_is_a_pass',
        'gui:fake_review_shows_not_run_unknown_and_pass_distinctly_and_accepts_only_a_pass'],
    'Only approved command bytes run; no command, argument or path over IPC': [
        'p2_03_command_is_exact_ordered_and_part_of_the_approved_bytes',
        'p2_03_plan_without_a_command_keeps_the_v1_bytes',
        'p2_03_unsafe_or_ambiguous_commands_are_refused',
        'p2_03_validate_refuses_stale_candidates_extra_fields_old_clients_and_plans_without_a_command'],
    'Start to settlement, validation and acceptance through the real helper executable; validator network denied': [
        REAL_HELPER_TEST, 'gui:real_helper_reports_an_unsealed_attempt_as_not_run_and_sends_nothing'],
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
    (ROOT / 'verification/p2-03-gui.json').write_text(json.dumps(gui, indent=2) + '\n')
    STATE.update(swift_test_names=names, swift_passed=swift_passed, gui=gui, temp=temp, env=env, cargo=cargo)
    p21.STATE.update(temp=temp)


def real_helper():
    """The one unsandboxed step: the real helper needs the system `ps`. Offline and fixture-only."""
    p = subprocess.run([STATE['cargo'], 'test', '--offline', '--locked', '-p', 'jojo-storage', '--test',
                        'p2_03_real_helper', '--', '--ignored'], cwd=ROOT, env=STATE['env'], capture_output=True,
                       text=True, timeout=240)
    output = p.stdout + p.stderr
    (ROOT / 'verification/p2-03-real-helper.log').write_text(
        '$ Real helper end to end (outside the OS sandbox: system ps required; offline; fixture executables only)\n' + output)
    print(f'Real helper end to end: exit {p.returncode}', flush=True)
    ok = p.returncode == 0 and f'test {REAL_HELPER_TEST} ... ok' in output and 'test result: ok. 1 passed' in output
    return {'passed': ok, 'exit_code': p.returncode, 'log': 'verification/p2-03-real-helper.log',
            'sandbox': 'none for this step only (the gate sandbox cannot execute the system ps); CARGO_NET_OFFLINE; '
                       'the validation command passes only when the validator sandbox denies its socket'}


def p2_03(output, passed):
    before = json.loads((ROOT / 'verification/p2-02-result.json').read_text())
    if not before.get('passed') or before.get('p2_03') != 'GO':
        raise RuntimeError('P2-02 is not GO')
    failures = []
    ran = {t.rsplit('::', 1)[-1] for t in passed}
    phase1 = set(re.findall(r'^test (\S+) \.\.\. ok$', (ROOT / 'verification/p1-17.log').read_text(), re.M))
    p2_01 = json.loads((ROOT / 'verification/p2-01-result.json').read_text())['p2_01_test_names']
    lost = sorted((phase1 | set(p2_01)) - set(passed)) + sorted(set(before['p2_02_test_names']) - ran)
    if (len(phase1) != p22.PHASE_1_RUST_TESTS or len(p2_01) != p22.P2_01_RUST_TESTS
            or len(before['p2_02_test_names']) != before['p2_02_tests'] or lost):
        failures.append(f'Phase 1 / P2-01 / P2-02 inventory missing or changed: {lost}')
    names = {}
    for path, count in P2_03_TESTS.items():
        found = [n for n in re.findall(r'#\[test\]\s*fn (\w+)\(', (ROOT / path).read_text()) if n.startswith('p2_03_')]
        names[path] = found
        if len(found) != count or set(found) - ran:
            failures.append(f'{path}: expected {count} passing tests, missing {sorted(set(found) - ran)} of {len(found)}')
    new = [n for found in names.values() for n in found]
    expected = before['rust_tests'] + len(new)
    if len(passed) != expected:
        failures.append(f'expected {expected} Rust tests, observed {len(passed)}')
    ignored = sorted(re.findall(r'^test (\S+) \.\.\. ignored', output, re.M))
    if [i.split(',')[0] for i in ignored] != sorted(p21.IGNORED + [REAL_HELPER_TEST]):
        failures.append(f'unexpected ignored tests {ignored}')
    real = real_helper()
    if not real['passed']:
        failures.append('real helper end-to-end test failed; see verification/p2-03-real-helper.log')

    done = ran | STATE['swift_passed'] | {f'gui:{k}' for k, v in STATE['gui'].items() if v['passed']}
    if real['passed']:
        done.add(REAL_HELPER_TEST)
    coverage = {case: {t: t in done for t in tests} for case, tests in COVERAGE.items()}
    missing = sorted({t for tests in coverage.values() for t, ok in tests.items() if not ok})
    mapped = {t for tests in COVERAGE.values() for t in tests}
    uncovered = sorted(set(new) - mapped) + sorted(f'gui:{k}' for k in json.loads(
        (STATE['temp'] / 'gui_p2_03.json').read_text()) if f'gui:{k}' not in mapped)
    if missing or uncovered:
        failures.append(f'coverage: not passing {missing}; not mapped to a case {uncovered}')

    p17.LAUNCH_SITES = p17.LAUNCH_SITES + [
        (r'scripts/verify_p2_0[12]\.py', r"\['git'", 'read-only Git state check'),
        (r'scripts/gui_p2_02\.py', r'\[app,', 'built app under GUI test (helper: real jojo-helper or the protocol fixture)'),
        (r'crates/jojo-storage/tests/p2_03_real_helper\.rs', r'Command::new\(&helper\)',
         'the workspace jojo-helper under test, built next to the fixture'),
        (r'scripts/verify_p2_03\.py', r"\['git'|\[STATE\['cargo'", 'read-only Git state check; cargo test of the real-helper test'),
    ]
    static = p17.launch_ledger()
    if static['unclassified'] or static['non_fixture_provider_executables'] or static['name_based_kill_tools']:
        failures.append(f"launch ledger: {static['unclassified']} {static['non_fixture_provider_executables']} {static['name_based_kill_tools']}")
    runtime = p21.runtime_ledger()
    if runtime['non_fixture_launches'] or runtime['scenarios'] != p21.LEDGER_SCENARIOS:
        failures.append(f"runtime launch ledger: foreign={runtime['non_fixture_launches']} scenarios={runtime['scenarios']}")
    p21.SCAN_EXEMPT.update({'scripts/verify_p2_02.py', 'scripts/verify_p2_03.py'})
    names_real = p21.real_binary_scan()
    if names_real:
        failures.append(f'source names the installed Codex CLI or its state: {names_real}')
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
        failures.append(f'P2-03 changed files outside its allowlist: removed={removed} outside={outside}')
    git = subprocess.run(['git', 'status', '--short'], cwd=ROOT, capture_output=True, text=True)
    if git.returncode != 128 or 'not a git repository' not in git.stderr:
        failures.append('Git precondition changed; inspect actual state')
    schema = max(int(v) for v in re.findall(r'^\s+version: (\d+),', (ROOT / 'crates/jojo-storage/src/schema.rs').read_text(), re.M))
    if schema != before['database_migrations']:
        failures.append(f'database schema changed: {schema}')

    manifest = {
        'generated_by': 'scripts/verify_p2_03.py',
        'suite': {'rust_tests': len(passed), 'earlier_rust_tests': before['rust_tests'], 'p2_03_tests': names,
                  'ignored': ignored, 'real_helper_end_to_end': real, 'swift_tests': STATE['swift_test_names'],
                  'gui_scenarios': {k: v['passed'] for k, v in STATE['gui'].items()}},
        'schema': {'database_migrations': schema, 'stdio_protocol': '1.1 with additive P2-03 commands (1.0 clients unchanged)',
                   'validation_plan': 'jojo-validation-plan-v2 adds the command; v1 bytes unchanged'},
        'coverage': coverage,
        'process_launches': static,
        'runtime_launch_ledger': runtime,
        'real_codex_references': names_real,
        'adapters': {k: v for k, v in net.items() if k != 'packages'},
        'links': link,
        'stage1': s1,
        'files_changed_since_p2_02': changed,
        'source_sha256': hashes,
        'failures': failures,
    }
    (ROOT / 'verification/p2-03-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    if failures:
        raise RuntimeError('; '.join(failures))
    return dict(earlier_rust_tests=before['rust_tests'], p2_03_tests=len(new), p2_03_test_names=new,
                real_helper_end_to_end=real, swift_tests=sum(SWIFT_TESTS.values()),
                gui_scenarios={k: v['passed'] for k, v in STATE['gui'].items()},
                gui_evidence='verification/p2-03-gui.json', ignored_tests=ignored,
                manifest='verification/p2-03-manifest.json', coverage_cases=len(COVERAGE),
                launch_sites=len(static['sites']), database_migrations=schema, real_codex_executed=False,
                files_changed=changed, input_sha256=hashes,
                git={'exit_code': git.returncode, 'stderr': git.stderr.strip()},
                verification_report='docs/verification/P2-03.md',
                live_attempt='NOT AUTHORIZED: G-LIVE decisions (U06, Codex home, usage limit, provider network) remain open')


if __name__ == '__main__':
    try:
        shared.main(task='P2-03', extra=p2_03, setup=setup,
                    scope='P2-03 only: host validation runner, validation and candidate review commands, minimal '
                          'SwiftUI review screen; fixture executables only; no live Codex execution, credential '
                          'access, network inference, P2-04, push or merge')
    finally:
        out = ROOT / 'verification/p2-03-result.json'
        result = json.loads(out.read_text())
        if result.get('passed') and result.get('boundary_tests') != BOUNDARY_TESTS:
            result.update(passed=False, p2_04='NO-GO', error=f"boundary inventory {result.get('boundary_tests')} != {BOUNDARY_TESTS}")
            out.write_text(json.dumps(result, indent=2) + '\n')
            sys.exit(1)
