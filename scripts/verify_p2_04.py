#!/usr/bin/env python3
"""P2-04 gate: offline dress rehearsal of the whole slice with a fake Codex executable.

Re-runs the complete suite (Rust, boundary, Swift, the seventeen earlier GUI scenarios) on
the final tree under OS network denial, then runs two steps outside that sandbox, because
the real helper inspects processes with the system `ps`, which the sandbox cannot execute:

* the P2-03 real-helper end-to-end test, as a regression;
* the rehearsal (`scripts/gui_p2_04.py`): ten scenarios through the real helper protocol
  and the real app, each on its own new data root and generated sample repository under a
  disposable root outside this project. Nothing there is deleted afterwards.

Both are offline by construction: the only provider executable is a private copy of
`jojo-codex-fixture`, and the validation command runs under the validator's own network
denial. The gate then checks the required cases, the stop conditions of the readiness
packet (section 9.5) and of the Phase 2 plan (section 7), the launch ledgers and scans of
the earlier gates, and that the files changed since P2-03 are exactly the P2-04 set.

The installed Codex CLI is never located, read or executed by this gate or by any test.
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

import verify_p1_04 as shared
import verify_p1_09 as p09
import verify_p1_17 as p17
import verify_p2_01 as p21
import verify_p2_02 as p22
import verify_p2_03 as p23

ROOT = Path(__file__).resolve().parents[1]
# Read before the gate replaces HOME: the rehearsal evidence must outlive the gate's scratch.
DEFAULT_REHEARSAL_ROOT = Path.home() / 'JojoDisposable' / 'p2-04'
BOUNDARY_TESTS = 85
# Prefix of the artifacts the shared steps write; a later gate that reuses them sets its own.
ARTIFACT = 'p2-04'
SAMPLE_TESTS = 10
REHEARSAL = 'scripts/gui_p2_04.py'
ALLOWED = {
    'crates/jojo-storage/src/bin/jojo-codex-fixture.rs',
    'macos/JojoApp/Sources/JojoApp/UITestDriver.swift',
    'scripts/check_p1_01_boundaries.py', 'scripts/gui_p2_04.py', 'scripts/p2_04_sample.py',
    'scripts/samples/slugify/adr.md', 'scripts/samples/slugify/bundle.json', 'scripts/samples/slugify/prd.json',
    'scripts/samples/slugify/slugify.py', 'scripts/samples/slugify/test_slugify.py',
    'scripts/tests/test_p2_04_sample.py', 'scripts/verify_p1_04.py', 'scripts/verify_p2_04.py',
    'docs/planning/P2_05_OPERATOR_CHECKLIST.md', 'docs/planning/PHASE_2_VERTICAL_SLICE.md', 'docs/verification/P2-04.md',
}
S = {
    'successful_development': 'successful_development_is_validated_and_accepted',
    'out_of_scope_write': 'out_of_scope_write_fails_validation_and_is_rejected',
    'edited_baseline_test': 'edited_baseline_test_fails_validation_and_is_rejected',
    'codex_turn_failure': 'codex_turn_failure_is_visible_and_never_accepted',
    'authentication_error': 'authentication_error_is_visible_and_nothing_is_relayed',
    'unknown_outcome': 'unknown_outcome_is_never_treated_as_success',
    'agent_killed': 'agent_killed_during_execution_is_failed_and_nothing_remains',
    'application_quit': 'application_quit_during_execution_stops_the_agent_and_is_recoverable',
    'application_crash': 'application_crash_during_execution_stops_the_agent_and_is_recoverable',
    'helper_killed': 'helper_killed_during_execution_is_held_until_the_agent_is_gone_then_recovered',
}
INTERRUPTED = [S['application_quit'], S['application_crash'], S['helper_killed']]
# The cases the P2-04 authorization requires, each mapped to evidence that must pass.
COVERAGE = {
    'Successful development and validation, accepted by the human in the app': [f"gui:{S['successful_development']}"],
    'Out-of-scope write': [f"gui:{S['out_of_scope_write']}", f"gui:{S['edited_baseline_test']}"],
    'Codex turn failure': [f"gui:{S['codex_turn_failure']}"],
    'Authentication error': [f"gui:{S['authentication_error']}"],
    'Helper killed during execution': [f"gui:{S['helper_killed']}"],
    'Application quit during execution': [f"gui:{S['application_quit']}", f"gui:{S['application_crash']}"],
    'Agent killed during execution': [f"gui:{S['agent_killed']}"],
    'Recovery after interruption on the same data root; no new attempt is started there': [f'gui:{n}' for n in INTERRUPTED],
    'Unknown outcome': [f"gui:{S['unknown_outcome']}", f"gui:{S['helper_killed']}"],
    'No orphaned agent processes': [f'gui:{n}' for n in S.values()],
    'Deterministic sample repository generator and approved feature bundle': [
        'test_same_inputs_give_the_same_single_commit', 'test_the_repository_holds_exactly_the_sample_files_and_no_hooks',
        'test_an_existing_repository_is_never_overwritten', 'test_a_root_that_is_not_private_is_refused',
        'test_a_root_inside_a_project_is_refused', 'test_a_missing_relative_or_shallow_root_and_a_nested_name_are_refused',
        'test_validation_plan_is_the_approved_scope_and_command_in_canonical_form',
        'test_payload_has_exactly_the_import_fields_and_names_no_executable',
        'test_the_live_bundle_has_no_fixture_directive_and_the_rehearsal_bundle_has_one',
        'test_the_stub_fails_its_tests_and_the_tests_name_only_the_standard_library'],
    'P2-03 regression: start to acceptance through the real helper executable over pipes': [p23.REAL_HELPER_TEST],
}
OPEN_GAPS = [
    'After a helper crash the agent process keeps running until it exits by itself: Jojo holds everything and never '
    'signals it (no lifetime handle). In the rehearsal the fake agent ends after 25 seconds; a real agent could run, '
    'write to its owned worktree and use the provider for much longer. Ending it is a human action on the exact PID '
    'the Recovery tab shows.',
    'A failed, interrupted or rejected run cannot be started again on the same data root (P2-02 limitation 3); a new '
    'attempt needs a fresh data root. Same-root continuation is deferred by decision.',
    'A crash between Start acceptance and scheduler reservation has no resolution command (P2-02 limitation 4); not '
    'exercised here.',
    'Registration and bundle import have no app control: they are protocol frames sent by scripts/p2_04_sample.py, '
    'and the helper must be closed and reopened by the app between import stages.',
    'The fake Codex emits documented-shape JSONL. Real event shapes, sandbox behaviour and reaction to a closed pipe '
    'or SIGTERM of the real CLI remain UNKNOWN until the live attempt (P2-01 uncertainties).',
    'The rehearsal and the real-helper regression run outside the OS network sandbox (the helper needs the system '
    'ps). They are offline by construction, not by OS enforcement.',
]
STATE = {'rehearsal_root': DEFAULT_REHEARSAL_ROOT}


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
    names = {prefix: re.findall(rf'func ({prefix}\w+)\(', sources) for prefix in p23.SWIFT_TESTS}
    total = sum(p23.SWIFT_TESTS.values())
    wrong = {p: len(n) for p, n in names.items() if len(n) != p23.SWIFT_TESTS[p]}
    missing = {n for found in names.values() for n in found} - swift_passed
    if not summary or summary[-1] != (str(total), '0') or wrong or missing:
        raise RuntimeError(f'Swift inventory failed: {summary[-1:]} counts={wrong} missing={missing}')
    if re.search(r'skipped', out, re.I):
        raise RuntimeError('A Swift test was skipped; real-helper tests are mandatory')
    app = next((temp / 'swift-t14').rglob('Products/Debug/JojoApp'))
    gui = {}
    for script, count in p23.GUI.items():
        report = temp / f'{Path(script).stem}.json'
        run(f'Runtime macOS GUI scenarios ({script})', [sys.executable, script, '--app', app, '--helper', helper, '--out', report])
        results = json.loads(report.read_text())
        if len(results) != count or not all(r['passed'] for r in results.values()):
            raise RuntimeError(f'GUI scenarios failed ({script}): {results}')
        gui.update(results)
    (ROOT / f'verification/{ARTIFACT}-gui.json').write_text(json.dumps(gui, indent=2) + '\n')
    out = run('P2-04 sample generator and bundle tests', [sys.executable, '-m', 'unittest', '-v', 'scripts/tests/test_p2_04_sample.py'])
    sample_passed = set(re.findall(r'^(test_\w+) \(.*\) \.\.\. ok$', out, re.M))
    if len(sample_passed) != SAMPLE_TESTS or not re.search(rf'Ran {SAMPLE_TESTS} tests', out):
        raise RuntimeError(f'sample generator inventory failed: {sorted(sample_passed)}')
    STATE.update(swift_test_names=names, swift_passed=swift_passed, gui=gui, sample_passed=sample_passed, temp=temp, env=env,
                 cargo=cargo, app=app, helper=helper)
    p21.STATE.update(temp=temp)


def real_helper():
    """Unsandboxed regression of the P2-03 real-helper test. Offline and fixture-only."""
    p = subprocess.run([STATE['cargo'], 'test', '--offline', '--locked', '-p', 'jojo-storage', '--test',
                        'p2_03_real_helper', '--', '--ignored'], cwd=ROOT, env=STATE['env'], capture_output=True,
                       text=True, timeout=240)
    output = p.stdout + p.stderr
    (ROOT / f'verification/{ARTIFACT}-real-helper.log').write_text(
        '$ Real helper end to end (outside the OS sandbox: system ps required; offline; fixture executables only)\n' + output)
    print(f'Real helper end to end: exit {p.returncode}', flush=True)
    ok = p.returncode == 0 and f'test {p23.REAL_HELPER_TEST} ... ok' in output and 'test result: ok. 1 passed' in output
    return {'passed': ok, 'exit_code': p.returncode, 'log': f'verification/{ARTIFACT}-real-helper.log'}


def rehearsal():
    """The dress rehearsal: unsandboxed for the same reason, on a new evidence root that is kept."""
    root = Path(STATE['rehearsal_root']) / time.strftime('%Y%m%dT%H%M%SZ', time.gmtime())
    os.makedirs(root, mode=0o700)
    for directory in [root, Path(STATE['rehearsal_root'])]:
        directory.chmod(0o700)
    fixture, report = STATE['temp'] / 'target/debug/jojo-codex-fixture', STATE['temp'] / 'gui_p2_04.json'
    p = subprocess.run([sys.executable, REHEARSAL, '--app', STATE['app'], '--helper', STATE['helper'], '--fixture', fixture,
                        '--root', root, '--out', report], cwd=ROOT, env=STATE['env'], capture_output=True, text=True,
                       timeout=1500)
    (ROOT / f'verification/{ARTIFACT}-rehearsal.log').write_text(
        f'$ Offline dress rehearsal (outside the OS sandbox: system ps required; fake Codex only)\n$ evidence root: {root}\n'
        + p.stdout + p.stderr)
    print(f'Offline dress rehearsal: exit {p.returncode}', flush=True)
    results = json.loads(report.read_text()) if report.exists() else {}
    (ROOT / f'verification/{ARTIFACT}-rehearsal.json').write_text(json.dumps(results, indent=2) + '\n')
    traces = ROOT / f'verification/{ARTIFACT}-rehearsal-traces'
    shutil.rmtree(traces, ignore_errors=True)
    for scenario in sorted(p for p in root.iterdir() if (p / 'traces').is_dir()):
        shutil.copytree(scenario / 'traces', traces / scenario.name)
    return {'exit_code': p.returncode, 'evidence_root': str(root), 'fixture_sha256': p21.digest(fixture),
            'log': f'verification/{ARTIFACT}-rehearsal.log', 'results': f'verification/{ARTIFACT}-rehearsal.json',
            'traces': f'verification/{ARTIFACT}-rehearsal-traces'}, results


def stop_conditions(results, fixture_sha256, root):
    """Every stop condition of readiness 9.5 and plan section 7, as observed by the rehearsal."""
    ok = {n: r for n, r in results.items() if r.get('passed')}
    if set(ok) != set(S.values()):
        return {'all ten scenarios passed': False}
    audits = {n: r['audit'] for n, r in ok.items()}
    views = {n: a['candidate'] for n, a in audits.items()}
    killed = ok[S['helper_killed']]
    return {
        'credential read or authentication relay: the sign-in canary in the Codex home was never copied, and an '
        'authentication failure led to no second launch':
            all(a['sign_in_canary'] == {'copies_outside_codex_home': [], 'codex_home_entries': ['SIGN-IN-CANARY']}
                for a in audits.values()) and len(audits[S['authentication_error']]['launch_effects']) == 1,
        'unrequested remote effect: no remote exists, and HEAD, branch, refs, index and working tree of every source '
        'repository are unchanged':
            all(a['source_repository']['before'] == a['source_repository']['after']
                and a['source_repository']['after']['remotes'] == '' for a in audits.values()),
        'unknown outcome treated as success: only the run that succeeded and passed host validation was accepted; '
        'every failed, unknown or interrupted run is Failed or Interrupted and was rejected':
            all((v['decision'] == 'ACCEPTED') == (n == S['successful_development']) and v['decision'] != 'NONE'
                for n, v in views.items())
            and (views[S['successful_development']]['outcome'], views[S['successful_development']]['validation']['state'])
            == ('Succeeded', 'PASS')
            and all(views[S[k]]['outcome'] == 'Failed' for k in ['codex_turn_failure', 'authentication_error',
                                                                 'unknown_outcome', 'agent_killed'])
            and all(views[n]['outcome'] == 'Interrupted' for n in INTERRUPTED),
        'orphaned process: none remained after any scenario; the one orphan (helper killed) was detected, shown, '
        'held and gone before anything was settled':
            all(a['agent_processes_remaining'] == [] for a in audits.values())
            and len(killed['orphan_after_helper_kill']) == 1 and 'live orphan pid:' in killed['held'][0],
        'write outside the owned worktree: every source repository is byte-identical and clean':
            all(a['source_repository']['after']['status'] == '' for a in audits.values()),
        'second launch for the same Start: exactly one launch effect and one attempt per data root':
            all(len(a['launch_effects']) == 1 and len(a['attempts']) == 1 for a in audits.values())
            and all(r['starts_accepted'] == 1 for r in ok.values()),
        'real Codex never executed: every scenario ran a private copy of the workspace fixture inside its own root':
            all(r['fake_codex']['sha256'] == fixture_sha256 and r['fake_codex']['path'].startswith(root + '/')
                and r['root'].startswith(root + '/') for r in ok.values()),
    }


def p2_04(output, passed):
    before = json.loads((ROOT / 'verification/p2-03-result.json').read_text())
    if not before.get('passed') or before.get('p2_04') != 'GO':
        raise RuntimeError('P2-03 is not GO')
    failures = []
    ran = {t.rsplit('::', 1)[-1] for t in passed}
    phase1 = set(re.findall(r'^test (\S+) \.\.\. ok$', (ROOT / 'verification/p1-17.log').read_text(), re.M))
    p2_01 = json.loads((ROOT / 'verification/p2-01-result.json').read_text())['p2_01_test_names']
    p2_02 = json.loads((ROOT / 'verification/p2-02-result.json').read_text())['p2_02_test_names']
    lost = sorted((phase1 | set(p2_01)) - set(passed)) + sorted((set(p2_02) | set(before['p2_03_test_names'])) - ran)
    if (len(phase1) != p22.PHASE_1_RUST_TESTS or len(p2_01) != p22.P2_01_RUST_TESTS or lost
            or len(before['p2_03_test_names']) != before['p2_03_tests']):
        failures.append(f'Phase 1 / P2-01 / P2-02 / P2-03 inventory missing or changed: {lost}')
    if len(passed) != before['rust_tests']:
        failures.append(f"expected {before['rust_tests']} Rust tests (P2-04 adds none), observed {len(passed)}")
    ignored = sorted(re.findall(r'^test (\S+) \.\.\. ignored', output, re.M))
    if [i.split(',')[0] for i in ignored] != sorted(p21.IGNORED + [p23.REAL_HELPER_TEST]):
        failures.append(f'unexpected ignored tests {ignored}')
    real = real_helper()
    if not real['passed']:
        failures.append('real helper end-to-end test failed; see verification/p2-04-real-helper.log')
    run, results = rehearsal()
    scenarios = {name: bool(r.get('passed')) for name, r in results.items()}
    if run['exit_code'] != 0 or scenarios != {name: True for name in S.values()}:
        failures.append(f'rehearsal failed: exit {run["exit_code"]} {scenarios}; see verification/p2-04-rehearsal.log')
    stops = stop_conditions(results, run['fixture_sha256'], run['evidence_root'])
    if not all(stops.values()):
        failures.append(f'stop conditions not all exercised and observed: {[k for k, v in stops.items() if not v]}')

    done = ran | STATE['swift_passed'] | STATE['sample_passed'] | {f'gui:{k}' for k, v in scenarios.items() if v}
    if real['passed']:
        done.add(p23.REAL_HELPER_TEST)
    coverage = {case: {t: t in done for t in tests} for case, tests in COVERAGE.items()}
    missing = sorted({t for tests in coverage.values() for t, ok in tests.items() if not ok})
    mapped = {t for tests in COVERAGE.values() for t in tests}
    uncovered = sorted(f'gui:{k}' for k in results if f'gui:{k}' not in mapped) + sorted(STATE['sample_passed'] - mapped)
    if missing or uncovered:
        failures.append(f'coverage: not passing {missing}; not mapped to a case {uncovered}')

    p17.LAUNCH_SITES = p17.LAUNCH_SITES + [
        (r'scripts/verify_p2_0[12]\.py', r"\['git'", 'read-only Git state check'),
        (r'scripts/gui_p2_02\.py', r'\[app,', 'built app under GUI test (helper: real jojo-helper or the protocol fixture)'),
        (r'crates/jojo-storage/tests/p2_03_real_helper\.rs', r'Command::new\(&helper\)',
         'the workspace jojo-helper under test, built next to the fixture'),
        (r'scripts/verify_p2_03\.py', r"\['git'|\[STATE\['cargo'", 'read-only Git state check; cargo test of the real-helper test'),
        (r'scripts/verify_p2_04\.py', r"\['git'|\[STATE\['cargo'|\[sys\.executable, REHEARSAL",
         'read-only Git state check; cargo test of the real-helper test; the rehearsal script'),
        (r'scripts/p2_04_sample\.py', r'\[GIT,', 'sample repository setup and read-only state, /usr/bin/git inside the disposable root'),
        (r'scripts/p2_04_sample\.py', r'\[str\(executable\), .--serve.', 'the built jojo-helper over pipes (registration and import)'),
        (r'scripts/gui_p2_04\.py', r'\[r\.app,', 'built app under rehearsal; it starts the built jojo-helper'),
        (r'scripts/gui_p2_04\.py', r'\[PS,', 'read-only process listing for the orphan check (/bin/ps)'),
    ]
    static = p17.launch_ledger()
    if static['unclassified'] or static['non_fixture_provider_executables'] or static['name_based_kill_tools']:
        failures.append(f"launch ledger: {static['unclassified']} {static['non_fixture_provider_executables']} {static['name_based_kill_tools']}")
    runtime = p21.runtime_ledger()
    if runtime['non_fixture_launches'] or runtime['scenarios'] != p21.LEDGER_SCENARIOS:
        failures.append(f"runtime launch ledger: foreign={runtime['non_fixture_launches']} scenarios={runtime['scenarios']}")
    p21.SCAN_EXEMPT.update({'scripts/verify_p2_02.py', 'scripts/verify_p2_03.py', 'scripts/verify_p2_04.py'})
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
    production = [p for p in changed if re.match(r'crates/[\w-]+/src/(?!bin/jojo-codex-fixture\.rs)', p)
                  or re.match(r'macos/JojoApp/Sources/(?!JojoApp/UITestDriver\.swift)', p)]
    if removed or outside or production:
        failures.append(f'P2-04 changed files outside its allowlist: removed={removed} outside={outside} production={production}')
    git = subprocess.run(['git', 'status', '--short'], cwd=ROOT, capture_output=True, text=True)
    if git.returncode != 128 or 'not a git repository' not in git.stderr:
        failures.append('Git precondition changed; inspect actual state')
    schema = max(int(v) for v in re.findall(r'^\s+version: (\d+),', (ROOT / 'crates/jojo-storage/src/schema.rs').read_text(), re.M))
    if schema != before['database_migrations']:
        failures.append(f'database schema changed: {schema}')

    manifest = {
        'generated_by': 'scripts/verify_p2_04.py',
        'suite': {'rust_tests': len(passed), 'ignored': ignored, 'real_helper_end_to_end': real,
                  'swift_tests': STATE['swift_test_names'], 'sample_tests': sorted(STATE['sample_passed']),
                  'earlier_gui_scenarios': {k: v['passed'] for k, v in STATE['gui'].items()},
                  'rehearsal_scenarios': scenarios},
        'rehearsal': run,
        'coverage': coverage,
        'stop_conditions': stops,
        'open_gaps': OPEN_GAPS,
        'process_launches': static,
        'runtime_launch_ledger': runtime,
        'real_codex_references': names_real,
        'adapters': {k: v for k, v in net.items() if k != 'packages'},
        'links': link,
        'stage1': s1,
        'files_changed_since_p2_03': changed,
        'production_source_changed': production,
        'source_sha256': hashes,
        'failures': failures,
    }
    (ROOT / 'verification/p2-04-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    if failures:
        raise RuntimeError('; '.join(failures))
    return dict(earlier_rust_tests=before['rust_tests'], p2_04_rust_tests=0, sample_tests=SAMPLE_TESTS,
                real_helper_end_to_end=real, swift_tests=sum(p23.SWIFT_TESTS.values()),
                earlier_gui_scenarios={k: v['passed'] for k, v in STATE['gui'].items()},
                earlier_gui_evidence='verification/p2-04-gui.json',
                rehearsal_scenarios=scenarios, rehearsal=run,
                rehearsal_seconds={k: v.get('seconds') for k, v in results.items()},
                stop_conditions=stops, open_gaps=OPEN_GAPS, ignored_tests=ignored,
                manifest='verification/p2-04-manifest.json', coverage_cases=len(COVERAGE),
                launch_sites=len(static['sites']), database_migrations=schema, real_codex_executed=False,
                production_source_changed=production, files_changed=changed, input_sha256=hashes,
                git={'exit_code': git.returncode, 'stderr': git.stderr.strip()},
                verification_report='docs/verification/P2-04.md',
                operator_checklist='docs/planning/P2_05_OPERATOR_CHECKLIST.md',
                live_attempt='NOT AUTHORIZED: P2-05 and every G-LIVE decision (U06, Codex home, usage limit, provider '
                             'network, the single attempt) remain open. GO here means only that authorization may be requested.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--rehearsal-root', default=str(DEFAULT_REHEARSAL_ROOT),
                        help='existing or new private directory outside every project; one run directory is added and kept')
    STATE['rehearsal_root'] = Path(parser.parse_args().rehearsal_root).resolve()
    try:
        shared.main(task='P2-04', extra=p2_04, setup=setup,
                    scope='P2-04 only: sample repository generator and bundle, offline dress rehearsal through the real '
                          'helper and app with a fake Codex executable, operator checklist; no live Codex execution, '
                          'credential access, network inference, P2-05, push or merge')
    finally:
        out = ROOT / 'verification/p2-04-result.json'
        result = json.loads(out.read_text())
        if result.get('passed') and result.get('boundary_tests') != BOUNDARY_TESTS:
            result.update(passed=False, p2_05_authorization_request='NO-GO',
                          error=f"boundary inventory {result.get('boundary_tests')} != {BOUNDARY_TESTS}")
            out.write_text(json.dumps(result, indent=2) + '\n')
            sys.exit(1)
