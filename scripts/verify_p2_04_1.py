#!/usr/bin/env python3
"""P2-04.1 gate: an agent launched by Jojo cannot outlive the helper that supervises it.

Re-runs the complete suite (Rust, boundary, Swift, the seventeen earlier GUI scenarios) on
the final tree under OS network denial, including the OS-level guard tests
(`crates/jojo-storage/tests/p2_04_1_guard.rs`: real fixture processes, a real SIGKILL of a
disposable supervisor). Then, outside that sandbox because the real helper needs the
system `ps`:

* the P2-03 real-helper end-to-end test, now with the guard armed for agent and validator;
* the rehearsal (`scripts/gui_p2_04.py`), twelve scenarios through the real helper and the
  real app, each on its own new root under a disposable directory that is kept.

It then checks the required cases, the stop conditions, the launch ledgers and scans of the
earlier gates, and that the files changed since P2-04 are exactly the P2-04.1 set.

The installed Codex CLI is never located, read or executed by this gate or by any test.
"""
import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

import verify_p1_04 as shared
import verify_p1_17 as p17
import verify_p2_01 as p21
import verify_p2_02 as p22
import verify_p2_03 as p23
import verify_p2_04 as p24

ROOT = Path(__file__).resolve().parents[1]
ARTIFACT = 'p2-04.1'
DEFAULT_REHEARSAL_ROOT = p24.DEFAULT_REHEARSAL_ROOT.with_name('p2-04.1')
BOUNDARY_TESTS = 87
GUARD_TESTS = 'crates/jojo-storage/tests/p2_04_1_guard.rs'
HELPER_TEST = 'p2_04_1_guard_mode_takes_exactly_a_leader_and_an_absolute_inspector'
ROLE = 'p2_04_1_role'
ALLOWED = {
    'crates/jojo-helper/src/lib.rs', 'crates/jojo-helper/src/main.rs', 'crates/jojo-helper/src/service.rs',
    'crates/jojo-storage/src/launch.rs', 'crates/jojo-storage/src/process.rs', 'crates/jojo-storage/src/validator.rs',
    'crates/jojo-storage/src/bin/jojo-codex-fixture.rs', GUARD_TESTS,
    'scripts/check_p1_01_boundaries.py', 'scripts/tests/test_boundaries.py', 'scripts/gui_p2_04.py',
    'scripts/verify_p1_04.py', 'scripts/verify_p2_04.py', 'scripts/verify_p2_04_1.py',
    'docs/architecture/STDIO_PROTOCOL.md', 'docs/planning/P2_05_OPERATOR_CHECKLIST.md',
    'docs/planning/PHASE_2_VERTICAL_SLICE.md', 'docs/verification/P2-04-1.md',
}
S = dict(p24.S)
del S['helper_killed']
S.update(helper_killed='helper_killed_during_execution_stops_the_agent_and_is_recoverable',
         guard_lost='helper_killed_while_the_guard_cannot_act_is_held_until_the_agent_is_gone',
         validation_killed='helper_killed_during_validation_stops_the_validation_command')
INTERRUPTED = [S['application_quit'], S['application_crash'], S['helper_killed'], S['guard_lost']]
T = 'p2_04_1_'
# The cases the P2-04.1 authorization requires, each mapped to evidence that must pass.
COVERAGE = {
    'The P2-04 orphan reproduced with fixture agents (no guard: the agent outlives a killed supervisor)': [
        T + 'without_a_guard_the_agent_outlives_a_killed_supervisor'],
    'Normal shutdown': [T + 'a_normal_exit_releases_the_guard_and_no_signal_is_sent', f"gui:{S['application_quit']}",
                        f"gui:{S['successful_development']}"],
    'Helper SIGKILL': [T + 'a_guarded_agent_is_stopped_when_its_supervisor_is_killed',
                       T + 'an_agent_that_ignores_term_is_killed_after_the_grace_period',
                       T + 'a_guard_ended_by_a_graceful_stop_is_replaced_and_still_works',
                       f"gui:{S['helper_killed']}", f"gui:{S['validation_killed']}"],
    'GUI crash': [f"gui:{S['application_crash']}"],
    'Agent already exited': [T + 'a_normal_exit_releases_the_guard_and_no_signal_is_sent',
                             T + 'a_descendant_in_the_group_is_stopped_after_the_agent_itself_has_exited',
                             f"gui:{S['agent_killed']}"],
    'PID reuse or uncertain identity': [T + 'a_guard_that_is_not_in_the_named_group_signals_nothing',
                                        T + 'the_guard_body_fires_only_inside_its_group_and_only_without_a_release',
                                        T + 'a_launch_whose_guard_cannot_be_armed_is_killed_and_reported_uncertain',
                                        HELPER_TEST],
    'Descendant processes: inside the group stopped; outside the group not covered': [
        T + 'a_descendant_in_the_group_is_stopped_after_the_agent_itself_has_exited',
        T + 'a_descendant_that_left_the_group_is_not_stopped_by_the_guard'],
    'Recovery after helper death on the same data root': [f"gui:{S['helper_killed']}", f"gui:{S['guard_lost']}"],
    'Fail-closed rules unchanged when the guard itself is lost': [f"gui:{S['guard_lost']}"],
    'P2-04 rehearsal cases unchanged': [f'gui:{S[k]}' for k in [
        'successful_development', 'out_of_scope_write', 'edited_baseline_test', 'codex_turn_failure',
        'authentication_error', 'unknown_outcome', 'agent_killed', 'application_quit', 'application_crash']],
    'P2-03 regression: start to acceptance through the real helper executable over pipes, guard armed': [
        p23.REAL_HELPER_TEST],
}
REMAINING_RISKS = [
    'A process that leaves the agent\'s process group (its own group or session) is not reached by the guard. It is '
    'recorded if the supervisor saw it and is never signalled. Whether the real Codex CLI starts such processes is '
    'unknown.',
    'The guard is an ordinary process of the same user. Anything running as that user, including the agent, can kill '
    'or stop it. A lost guard is replaced while the helper lives; if helper and guard are both lost the P2-04 '
    'behaviour applies (agent runs on, everything held, human action on the PID shown).',
    'Between a TERM sent to the group and the replacement of the guard (one supervisor poll, about 20 ms plus one '
    'process listing) the group has no guard.',
    'The guard gives the leader up to 2 seconds after TERM before KILL. A helper that is frozen rather than dead '
    '(SIGSTOP, a hang) holds the lifeline open: the guard does not fire and the agent continues, supervised by '
    'nobody, until the helper resumes or dies.',
    'If the whole machine loses power nothing runs; after a reboot recovery treats the recorded identities as absent '
    '(different boot), as before.',
    'A lifeline write end could be inherited by a short-lived child spawned by another helper thread at the same '
    'instant (macOS has no atomic close-on-exec pipe creation). The guard would then fire when that child exits, '
    'not earlier.',
    'Jojo records nothing about what the guard did, because the process that would record it is dead. Recovery '
    'settles the run from OS evidence as Interrupted (helper_lost_recovered), as before.',
    'The fake agent is not Codex: reaction of the real CLI to TERM and to closed pipes remains unknown (P2-01).',
    'A failed, interrupted or rejected run still cannot be started again on the same data root.',
]


def stop_conditions(results, fixture_sha256, root):
    ok = {n: r for n, r in results.items() if r.get('passed')}
    if set(ok) != set(S.values()):
        return {'all twelve scenarios passed': False}
    audits = {n: r['audit'] for n, r in ok.items()}
    views = {n: a['candidate'] for n, a in audits.items()}
    killed, lost, validating = ok[S['helper_killed']], ok[S['guard_lost']], ok[S['validation_killed']]
    succeeded = [S['successful_development'], S['out_of_scope_write'], S['edited_baseline_test'], S['validation_killed']]
    return {
        'an agent does not outlive a killed helper: gone, with its guard, within 5 seconds':
            killed['agent_gone_seconds_after_helper_kill'] < 5 and killed['guard'][2] == killed['agent'][2],
        'a validation command does not outlive a killed helper: gone within 5 seconds, and no verdict exists':
            validating['validation_gone_seconds_after_helper_kill'] < 5
            and views[S['validation_killed']]['validation']['state'] == 'UNKNOWN',
        'with the guard lost as well, the orphan is detected, shown and held, and nothing is signalled by Jojo':
            len(lost['orphan_after_helper_kill']) == 1 and lost['orphan_during_held_session'] == lost['orphan_after_helper_kill']
            and 'live orphan pid:' in lost['held'][0] and 'choices: [Hold]' in lost['held'][0],
        'no agent or guard process remained after any scenario':
            all(a['agent_processes_remaining'] == [] and a['guard_processes_remaining'] == [] for a in audits.values()),
        'credential read or authentication relay: the sign-in canary was never copied':
            all(a['sign_in_canary'] == {'copies_outside_codex_home': [], 'codex_home_entries': ['SIGN-IN-CANARY']}
                for a in audits.values()),
        'unrequested remote effect or write outside the owned worktree: every source repository is unchanged':
            all(a['source_repository']['before'] == a['source_repository']['after']
                and a['source_repository']['after']['remotes'] == '' for a in audits.values()),
        'unknown outcome treated as success: only the run that succeeded and passed host validation was accepted':
            all((v['decision'] == 'ACCEPTED') == (n == S['successful_development']) and v['decision'] != 'NONE'
                for n, v in views.items())
            and all((views[n]['outcome'] == 'Succeeded') == (n in succeeded) for n in views)
            and all(views[n]['outcome'] == 'Interrupted' for n in INTERRUPTED),
        'second launch for the same Start: exactly one launch effect and one attempt per data root':
            all(len(a['launch_effects']) == 1 and len(a['attempts']) == 1 for a in audits.values())
            and all(r['starts_accepted'] == 1 for r in ok.values()),
        'real Codex never executed: every scenario ran a private copy of the workspace fixture inside its own root':
            all(r['fake_codex']['sha256'] == fixture_sha256 and r['fake_codex']['path'].startswith(root + '/')
                for r in ok.values()),
    }


def p2_04_1(output, passed):
    before = json.loads((ROOT / 'verification/p2-04-result.json').read_text())
    if not before.get('passed') or before.get('p2_05_authorization_request') != 'GO':
        raise RuntimeError('P2-04 is not GO')
    failures = []
    ran = {t.rsplit('::', 1)[-1] for t in passed}
    phase1 = set(re.findall(r'^test (\S+) \.\.\. ok$', (ROOT / 'verification/p1-17.log').read_text(), re.M))
    p2_01 = json.loads((ROOT / 'verification/p2-01-result.json').read_text())['p2_01_test_names']
    p2_02 = json.loads((ROOT / 'verification/p2-02-result.json').read_text())['p2_02_test_names']
    p2_03 = json.loads((ROOT / 'verification/p2-03-result.json').read_text())['p2_03_test_names']
    lost = sorted((phase1 | set(p2_01)) - set(passed)) + sorted((set(p2_02) | set(p2_03)) - ran)
    if len(phase1) != p22.PHASE_1_RUST_TESTS or len(p2_01) != p22.P2_01_RUST_TESTS or lost:
        failures.append(f'earlier Rust inventory missing or changed: {lost}')
    guard_tests = [n for n in re.findall(r'#\[test\]\s*fn (\w+)\(', (ROOT / GUARD_TESTS).read_text()) if n != ROLE]
    new = guard_tests + [HELPER_TEST]
    if len(guard_tests) != 10 or set(new) - ran:
        failures.append(f'P2-04.1 tests: expected 10 guard tests and the helper test, missing {sorted(set(new) - ran)}')
    if len(passed) != before['rust_tests'] + len(new):
        failures.append(f"expected {before['rust_tests'] + len(new)} Rust tests, observed {len(passed)}")
    ignored = sorted(re.findall(r'^test (\S+) \.\.\. ignored', output, re.M))
    if [i.split(',')[0] for i in ignored] != sorted(p21.IGNORED + [p23.REAL_HELPER_TEST, ROLE]):
        failures.append(f'unexpected ignored tests {ignored}')
    real = p24.real_helper()
    if not real['passed']:
        failures.append(f'real helper end-to-end test failed; see {real["log"]}')
    run, results = p24.rehearsal()
    scenarios = {name: bool(r.get('passed')) for name, r in results.items()}
    if run['exit_code'] != 0 or scenarios != {name: True for name in S.values()}:
        failures.append(f'rehearsal failed: exit {run["exit_code"]} {scenarios}; see {run["log"]}')
    stops = stop_conditions(results, run['fixture_sha256'], run['evidence_root'])
    if not all(stops.values()):
        failures.append(f'conditions not all observed: {[k for k, v in stops.items() if not v]}')

    state = p24.STATE
    done = ran | state['swift_passed'] | {f'gui:{k}' for k, v in scenarios.items() if v}
    if real['passed']:
        done.add(p23.REAL_HELPER_TEST)
    coverage = {case: {t: t in done for t in tests} for case, tests in COVERAGE.items()}
    missing = sorted({t for tests in coverage.values() for t, ok in tests.items() if not ok})
    mapped = {t for tests in COVERAGE.values() for t in tests}
    uncovered = sorted(f'gui:{k}' for k in results if f'gui:{k}' not in mapped) + sorted(set(new) - mapped)
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
        (r'scripts/verify_p2_04_1\.py', r"\['git'", 'read-only Git state check'),
        (r'scripts/p2_04_sample\.py', r'\[GIT,', 'sample repository setup and read-only state, /usr/bin/git inside the disposable root'),
        (r'scripts/p2_04_sample\.py', r'\[str\(executable\), .--serve.', 'the built jojo-helper over pipes (registration and import)'),
        (r'scripts/gui_p2_04\.py', r'\[r\.app,', 'built app under rehearsal; it starts the built jojo-helper'),
        (r'scripts/gui_p2_04\.py', r'\[PS,', 'read-only process listing for the orphan check (/bin/ps)'),
        (r'crates/jojo-storage/src/process\.rs', r'Command::new\(guard\)',
         'P2-04.1 agent guard: the helper executable in --agent-guard mode, inside the owned process group'),
    ]
    static = p17.launch_ledger()
    if static['unclassified'] or static['non_fixture_provider_executables'] or static['name_based_kill_tools']:
        failures.append(f"launch ledger: {static['unclassified']} {static['non_fixture_provider_executables']} {static['name_based_kill_tools']}")
    runtime = p21.runtime_ledger()
    if runtime['non_fixture_launches'] or runtime['scenarios'] != p21.LEDGER_SCENARIOS:
        failures.append(f"runtime launch ledger: foreign={runtime['non_fixture_launches']} scenarios={runtime['scenarios']}")
    p21.SCAN_EXEMPT.update({'scripts/verify_p2_02.py', 'scripts/verify_p2_03.py', 'scripts/verify_p2_04.py',
                            'scripts/verify_p2_04_1.py'})
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
                  or p.startswith('macos/JojoApp/Sources/')]
    if removed or outside:
        failures.append(f'P2-04.1 changed files outside its allowlist: removed={removed} outside={outside}')
    git = subprocess.run(['git', 'status', '--short'], cwd=ROOT, capture_output=True, text=True)
    if git.returncode != 128 or 'not a git repository' not in git.stderr:
        failures.append('Git precondition changed; inspect actual state')
    schema = max(int(v) for v in re.findall(r'^\s+version: (\d+),', (ROOT / 'crates/jojo-storage/src/schema.rs').read_text(), re.M))
    if schema != before['database_migrations']:
        failures.append(f'database schema changed: {schema}')

    manifest = {
        'generated_by': 'scripts/verify_p2_04_1.py',
        'suite': {'rust_tests': len(passed), 'p2_04_1_tests': new, 'ignored': ignored, 'real_helper_end_to_end': real,
                  'swift_tests': state['swift_test_names'], 'sample_tests': sorted(state['sample_passed']),
                  'earlier_gui_scenarios': {k: v['passed'] for k, v in state['gui'].items()},
                  'rehearsal_scenarios': scenarios},
        'rehearsal': run,
        'coverage': coverage,
        'conditions': stops,
        'remaining_risks': REMAINING_RISKS,
        'process_launches': static,
        'runtime_launch_ledger': runtime,
        'real_codex_references': names_real,
        'adapters': {k: v for k, v in net.items() if k != 'packages'},
        'links': link,
        'stage1': s1,
        'files_changed_since_p2_04': changed,
        'production_source_changed': production,
        'source_sha256': hashes,
        'failures': failures,
    }
    (ROOT / f'verification/{ARTIFACT}-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    if failures:
        raise RuntimeError('; '.join(failures))
    return dict(earlier_rust_tests=before['rust_tests'], p2_04_1_rust_tests=len(new), p2_04_1_test_names=new,
                sample_tests=p24.SAMPLE_TESTS, real_helper_end_to_end=real, swift_tests=sum(p23.SWIFT_TESTS.values()),
                earlier_gui_scenarios={k: v['passed'] for k, v in state['gui'].items()},
                earlier_gui_evidence=f'verification/{ARTIFACT}-gui.json',
                rehearsal_scenarios=scenarios, rehearsal=run,
                rehearsal_seconds={k: v.get('seconds') for k, v in results.items()},
                agent_gone_seconds_after_helper_kill=results[S['helper_killed']]['agent_gone_seconds_after_helper_kill'],
                validation_gone_seconds_after_helper_kill=results[S['validation_killed']]['validation_gone_seconds_after_helper_kill'],
                conditions=stops, remaining_risks=REMAINING_RISKS, ignored_tests=ignored,
                manifest=f'verification/{ARTIFACT}-manifest.json', coverage_cases=len(COVERAGE),
                launch_sites=len(static['sites']), database_migrations=schema, real_codex_executed=False,
                production_source_changed=production, files_changed=changed, input_sha256=hashes,
                git={'exit_code': git.returncode, 'stderr': git.stderr.strip()},
                verification_report='docs/verification/P2-04-1.md',
                operator_checklist='docs/planning/P2_05_OPERATOR_CHECKLIST.md',
                live_attempt='NOT AUTHORIZED: P2-05 and every G-LIVE decision remain open. GO here is a recommendation '
                             'that one supervised attempt may be requested; it starts nothing.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--rehearsal-root', default=str(DEFAULT_REHEARSAL_ROOT),
                        help='existing or new private directory outside every project; one run directory is added and kept')
    p24.STATE['rehearsal_root'] = Path(parser.parse_args().rehearsal_root).resolve()
    p24.ARTIFACT = ARTIFACT
    try:
        shared.main(task='P2-04.1', extra=p2_04_1, setup=p24.setup,
                    scope='P2-04.1 only: agent guard so that no agent or validation process outlives the helper; '
                          'fixture agents only; no live Codex execution, credential access, network inference, '
                          'P2-05, push or merge')
    finally:
        out = ROOT / f'verification/{ARTIFACT}-result.json'
        result = json.loads(out.read_text())
        if result.get('passed') and result.get('boundary_tests') != BOUNDARY_TESTS:
            result.update(passed=False, p2_05_single_supervised_attempt='NO-GO',
                          error=f"boundary inventory {result.get('boundary_tests')} != {BOUNDARY_TESTS}")
            out.write_text(json.dumps(result, indent=2) + '\n')
            sys.exit(1)
