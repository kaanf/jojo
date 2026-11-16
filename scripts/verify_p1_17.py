#!/usr/bin/env python3
"""P1-17 readiness gate: re-runs the complete offline suite on the final tree, then the
deterministic T17 provenance checks, and writes the machine-readable readiness manifest.

T17: local links; fixture/tool/source digests and manifest completeness; Stage 1 report
hashes and clean, pinned upstream checkouts with unchanged license files; an enumerated and
classified process-launch ledger (only reviewed fixture executables act as providers);
absence of remote/inference adapters; dependency licenses; and an invariant-coverage map
whose every named test must have passed in this run. No implementation is added.
"""
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
from pathlib import Path
import verify_p1_04 as shared
import verify_p1_09 as p09
ROOT = Path(__file__).resolve().parents[1]
RUST_TESTS = 339
SWIFT_TESTS = 27
GUI_SCENARIOS = 9
BOUNDARY_TESTS = 55
IGNORED = ['child_fixture', 't16_helper_role']
# P1-17 adds only its gate, the readiness packet and reports; no implementation.
ALLOWED = {'scripts/verify_p1_17.py', 'scripts/verify_p1_04.py', 'docs/verification/P1-17.md',
           'docs/verification/PHASE_1_READINESS.md', 'README.md'}
# Brief definition-of-done rows and the standing safety invariants, each mapped to tests that
# must pass in this run. Swift/GUI and boundary evidence are checked by their own counts.
COVERAGE = {
    'Human PRD/ADR/plan approval bound to exact displayed bytes': [
        't15_end_to_end_blocks_before_every_approval_and_binds_displayed_bytes',
        't13_review_decision_binds_exact_display_and_replays_original_receipt',
        'missing_revoked_or_changed_artifact_approvals_block_and_stop',
        'plan_revision_changes_require_new_human_review_and_preserve_old_graphs'],
    'Multiple Claude/Codex processes; cap one by default': [
        't15_cap_one_runs_mixed_providers_strictly_serially',
        't15_cap_two_overlaps_claude_and_codex_in_independent_repositories',
        't06_shipped_defaults_are_one_and_zero_is_a_durable_pause',
        't06_cap_two_mixed_providers_independent_repositories_overlap',
        't09_mixed_provider_attempts_run_concurrently_with_independent_streams'],
    'Independent worktrees and per-repository resources (Git worktree isolation)': [
        't15_same_repository_preprovisioned_worktrees_overlap_only_under_elevated_policy',
        't07_preprovisioned_same_repository_cap_two_separate_edits_and_outputs',
        't07_linked_and_symlink_aliases_share_gate',
        't07_creation_does_not_modify_user_checkout_or_remote_config',
        't07_symlink_attempt_directory_cannot_escape_owned_root'],
    'Explicit typed dependencies; PR-open never completes': [
        't15_pr_open_never_unlocks_and_validated_output_satisfies_the_exact_predicate',
        'opened_pr_failed_lookup_and_mismatched_deployment_never_reserve'],
    'Immutable execution evidence and deterministic attempt settlement': [
        't10_success_seals_one_immutable_result_with_verified_evidence_and_releases_claims',
        't10_concurrent_finalizers_have_exactly_one_winner',
        't10_cancel_before_finalization_wins_and_cancel_after_finalization_changes_nothing',
        't10_missing_or_corrupt_referenced_blob_blocks_and_is_never_reconstructed',
        't16_fault_boundary_matrix_leaves_a_valid_result_or_an_explicit_hold',
        't16_seeded_concurrent_schedules_keep_capacity_ownership_and_receipts'],
    'Safe shutdown coordination, cancel-close and verified receipts': [
        't12_active_count_changes_while_open_and_cancel_close_sends_nothing',
        't12_confirm_stops_two_provider_trees_and_a_test_worker_without_task_cancel',
        't12_unconfirmed_descendant_stop_fails_the_close_and_keeps_claims_and_gate',
        't15_cancel_close_leaves_processes_and_confirmed_close_reopen_preserves_evidence',
        't16_disk_failures_on_log_result_and_stop_flush_fail_the_clean_close'],
    'Recovery admission restrictions; no silent background execution': [
        't11_reopen_stays_paused_until_a_fresh_reviewed_inspection_starts_it',
        't15_cancel_close_leaves_processes_and_confirmed_close_reopen_preserves_evidence',
        't16_real_sigkill_of_a_disposable_helper_quarantines_and_never_kills_by_name'],
    'Quarantined resource claims; fake reboot/clock jump never frees ownership': [
        't06_unknown_live_claims_survive_heartbeat_expiry_and_restart',
        't16_fake_reboot_and_clock_jumps_never_free_live_ownership'],
    'Exact continuation authorization; completed effects never repeated': [
        't11_continuation_is_a_new_attempt_and_a_completed_effect_is_never_repeated',
        't10_effect_committed_but_ack_lost_stays_issued_unknown_and_is_never_replayed'],
    'Preserved work/evidence; validated resume; restore integrity': [
        't11_moved_replaced_head_changed_or_dirty_worktree_blocks_recovery',
        't11_unchanged_settled_worktree_recovers_its_real_result_and_validates_output',
        't16_restored_data_root_keeps_integrity_and_dead_candidates_stay_inspectable'],
    'Fail-closed treatment of unknown outcomes': [
        't10_process_unknown_cannot_finalize_and_keeps_claims',
        't09_inspection_failure_returns_unknown_within_stop_budget_and_keeps_claims',
        't16_unknown_process_or_repository_inspection_blocks'],
    'Only one supervised helper; Rust owns state; channel boundary': [
        't13_real_pipe_singleton_blocks_replacement_and_crash_has_no_ready',
        't13_child_does_not_inherit_helper_control_fds',
        't13_worker_output_flood_does_not_starve_control_or_forge_ready',
        't16_malicious_output_and_forged_approval_commands_are_denied_by_the_channel',
        't16_permission_boundaries_refuse_shared_or_substituted_roots'],
}
# Every process-launch site, classified. Anything unmatched fails the gate.
LAUNCH_SITES = [
    (r'crates/jojo-helper/tests/\w+\.rs', r'env!\("CARGO_BIN_EXE_jojo-helper"', 'helper under test'),
    (r'crates/jojo-storage/src/bin/jojo-process-fixture\.rs', r'me\(\)|"/bin/sh"', 'fixture-owned descendants'),
    (r'crates/jojo-storage/src/process\.rs', r'Command::new\(program\)', 'pinned system tools /bin/ps, /bin/kill (exact pid/pgid), /usr/sbin/sysctl'),
    (r'crates/jojo-storage/src/process\.rs', r'Command::new\(&launch\.executable\)', 'P1-09 supervised launch of a validated, issued LaunchSpec'),
    (r'crates/jojo-storage/src/worktrees\.rs', r'"/usr/bin/git"', 'local Git boundary'),
    (r'crates/jojo-storage/tests/[\w/]+\.rs', r'"/usr/bin/git"', 'test repository setup'),
    (r'crates/jojo-storage/tests/\w+\.rs', r'std::env::current_exe\(\)', 'this test binary as a disposable child'),
    (r'macos/JojoApp/Sources/JojoCore/HelperProcess\.swift', r'Process\(\)|executableURL = configuration\.executable', 'the configured jojo-helper only'),
    (r'scripts/gui_p1_14\.py', r'\[app,|\[self\.helper,', 'built app and helper under GUI test'),
    (r'scripts/verify_p1_0[19]\.py', r'sandbox-exec|sys\.argv\[1', 'gate tooling'),
    (r'scripts/verify_p1_04\.py', r'/usr/bin/sandbox-exec', 'network-denied gate runner'),
    (r'scripts/verify_p1_1[3-7]\.py', r"\['git'|\['/usr/bin/git'", 'read-only Git state checks'),
    (r'scripts/verify_p1_17\.py', r'subprocess\.run\(args', 'read-only tool version capture (sw_vers, rustc, cargo, swift, git, sqlite3)'),
]
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
    (ROOT / 'verification/p1-17-gui.json').write_text(json.dumps(gui, indent=2) + '\n')
    STATE.update(swift_test_names=names, gui=gui)


def tool(args):
    p = subprocess.run(args, capture_output=True, text=True)
    return (p.stdout or p.stderr).strip().splitlines()[0] if p.returncode == 0 else f'unavailable ({p.returncode})'


def links():
    files = [ROOT / 'README.md'] + sorted((ROOT / 'docs').rglob('*.md'))
    checked, broken = 0, []
    for f in files:
        for m in re.finditer(r'\[[^\]]*\]\(([^)\s]+)\)', f.read_text()):
            target = m.group(1)
            if re.match(r'[a-z]+:', target) or target.startswith('#'):
                continue
            checked += 1
            if not (f.parent / target.split('#')[0]).resolve().exists():
                broken.append(f'{f.relative_to(ROOT)}: {target}')
    return {'files': len(files), 'local_links': checked, 'broken': broken}


def stage1():
    base = json.loads((ROOT / 'verification/p1-01-preservation-baseline.json').read_text())
    docs = {p: digest(ROOT / p) == h for p, h in base['documents'].items()}
    env = dict(os.environ, GIT_OPTIONAL_LOCKS='0')
    clones = []
    for pin in json.loads((ROOT / 'research/REPOSITORY_PINS.json').read_text()):
        d = ROOT / 'research/upstream' / pin['name']
        git = lambda *a: subprocess.run(['/usr/bin/git', '-C', str(d), *a], capture_output=True, text=True, env=env)
        clones.append({'name': pin['name'], 'pinned': git('rev-parse', 'HEAD').stdout.strip() == pin['commit'],
                       'clean': git('status', '--porcelain').stdout == '',
                       'licenses_unchanged': all(digest(d / l['path']) == l['sha256'] for l in pin['licenses'])})
    return {'documents': docs, 'clones': clones}


def launch_ledger():
    patterns = {'.rs': r'Command::new\(([^)]*)\)', '.swift': r'Process\(\)|executableURL\s*=\s*[^\n]+',
                '.py': r'subprocess\.(?:run|Popen|check_output|call)\(\s*(?:\[[^\]]{0,80}|[A-Za-z_][\w.]*)'}
    ledger, unknown = [], []
    for root in ['crates', 'macos/JojoApp/Sources', 'macos/JojoApp/Tests', 'scripts']:
        for p in sorted((ROOT / root).rglob('*')):
            if p.suffix not in patterns or {'.build', '__pycache__'} & set(p.parts):
                continue
            rel = str(p.relative_to(ROOT))
            for i, line in enumerate(p.read_text().splitlines(), 1):
                for m in re.finditer(patterns[p.suffix], line):
                    site = m.group(0)[:100]
                    purpose = next((why for f, e, why in LAUNCH_SITES if re.fullmatch(f, rel) and re.search(e, site)), None)
                    (ledger if purpose else unknown).append({'site': f'{rel}:{i}', 'launch': site, 'purpose': purpose})
    providers = []
    for p in sorted((ROOT / 'crates').rglob('*.rs')):
        if '/tests/' in str(p):
            # LaunchSpec/fingerprint fields only: code lines of the form `executable: <expr>,`.
            providers += [f'{p.relative_to(ROOT)}: {m}' for m in re.findall(r'^\s*executable:\s*([^,\n]+),\s*$', p.read_text(), re.M)]
    bad_providers = [x for x in providers if not re.search(r'FIXTURE|fixture', x)]
    named_kills = [str(p.relative_to(ROOT)) for root in ['crates', 'macos/JojoApp/Sources', 'scripts']
                   for p in (ROOT / root).rglob('*') if p.is_file() and p.suffix in {'.rs', '.swift', '.py'}
                   and p.name != 'verify_p1_17.py' and re.search(r'\b(pkill|killall)\b', p.read_text())]
    return {'sites': ledger, 'unclassified': unknown, 'provider_executables_in_tests': providers,
            'non_fixture_provider_executables': bad_providers, 'name_based_kill_tools': named_kills}


def adapters():
    lock = (ROOT / 'Cargo.lock').read_text()
    packages = sorted(set(re.findall(r'^name = "([^"]+)"', lock, re.M)))
    forbidden_crates = sorted(set(packages) & {'reqwest', 'hyper', 'ureq', 'tokio', 'openssl', 'native-tls', 'rustls',
                                               'h2', 'curl', 'isahc', 'surf', 'tungstenite', 'async-openai'})
    hits = []
    for root in ['crates', 'macos/JojoApp/Sources', 'macos/JojoApp/Tests']:
        for p in (ROOT / root).rglob('*'):
            if p.suffix in {'.rs', '.swift'} and '.build' not in p.parts:
                for i, line in enumerate(p.read_text().splitlines(), 1):
                    if re.search(r'std::net|TcpStream|UdpSocket|URLSession|NWConnection|https?://', line):
                        hits.append(f'{p.relative_to(ROOT)}:{i}: {line.strip()[:100]}')
    reviewed = [h for h in hits if 'https://example.invalid/not-contacted' in h]
    return {'packages': packages, 'forbidden_crates': forbidden_crates, 'network_tokens': hits,
            'reviewed_non_contacted': reviewed, 'unreviewed_network_tokens': [h for h in hits if h not in reviewed]}


def licenses(packages):
    home = Path(os.environ.get('CARGO_HOME', Path.home() / '.cargo')) / 'registry/src'
    lock = (ROOT / 'Cargo.lock').read_text()
    out = {}
    for name, version in re.findall(r'^name = "([^"]+)"\nversion = "([^"]+)"\nsource = "registry', lock, re.M):
        manifest = next(home.glob(f'*/{name}-{version}/Cargo.toml'), None)
        found = manifest and re.search(r'^license\s*=\s*"([^"]+)"', manifest.read_text(), re.M)
        out[f'{name}@{version}'] = found.group(1) if found else None
    return out


def milestones():
    rows = []
    for i in range(1, 17):
        result = ROOT / f'verification/p1-{i:02d}-result.json'
        report = ROOT / f'docs/verification/P1-{i:02d}.md'
        d = json.loads(result.read_text())
        rows.append({'task': f'P1-{i:02d}', 'passed': d.get('passed') is True,
                     'next': {k: v for k, v in d.items() if re.fullmatch(r'p1_\d\d', k)},
                     'result': str(result.relative_to(ROOT)), 'result_sha256': digest(result),
                     'report': str(report.relative_to(ROOT)), 'report_sha256': digest(report),
                     'files_changed': d.get('files_changed')})
    return rows


def t17(output, passed):
    p16 = json.loads((ROOT / 'verification/p1-16-result.json').read_text())
    if not p16.get('passed') or p16.get('p1_17') != 'GO':
        raise RuntimeError('P1-16 is not GO')
    failures = []
    if len(passed) != RUST_TESTS:
        failures.append(f'expected {RUST_TESTS} Rust tests, observed {len(passed)}')
    old = set(re.findall(r'^test (\S+) \.\.\. ok$', (ROOT / 'verification/p1-16.log').read_text(), re.M))
    if len(old) != RUST_TESTS or old - set(passed):
        failures.append('P1-16 acceptance inventory missing or changed')
    ignored = sorted(re.findall(r'^test (\S+) \.\.\. ignored', output, re.M))
    if ignored != IGNORED:
        failures.append(f'unexpected ignored tests {ignored}')
    coverage = {req: {t: t in passed for t in tests} for req, tests in COVERAGE.items()}
    missing = sorted({t for tests in coverage.values() for t, ok in tests.items() if not ok})
    if missing:
        failures.append(f'coverage tests not passing: {missing}')
    link = links()
    if link['broken']:
        failures.append(f"broken local links: {link['broken']}")
    s1 = stage1()
    if not all(s1['documents'].values()) or not all(c['pinned'] and c['clean'] and c['licenses_unchanged'] for c in s1['clones']):
        failures.append('Stage 1 inputs or upstream checkouts changed')
    ledger = launch_ledger()
    if ledger['unclassified'] or ledger['non_fixture_provider_executables'] or ledger['name_based_kill_tools']:
        failures.append(f"launch ledger: {ledger['unclassified']} {ledger['non_fixture_provider_executables']} {ledger['name_based_kill_tools']}")
    net = adapters()
    if net['forbidden_crates'] or net['unreviewed_network_tokens']:
        failures.append(f"adapter scan: {net['forbidden_crates']} {net['unreviewed_network_tokens']}")
    deps = licenses(net['packages'])
    if any(v is None for v in deps.values()):
        failures.append(f'dependency license metadata missing: {[k for k, v in deps.items() if v is None]}')
    ms = milestones()
    if not all(m['passed'] for m in ms):
        failures.append('a milestone result is not passed')
    before = json.loads((ROOT / 'verification/p1-16-result.json').read_text())['input_sha256']
    files = [p for directory in ['crates', 'scripts', 'docs', 'macos', '.cargo'] for p in (ROOT / directory).rglob('*')
             if p.is_file() and not {'__pycache__', '.build', '.swiftpm'} & set(p.parts)]
    files += [ROOT / p for p in ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'README.md', '.gitignore', 'research/REPOSITORY_PINS.json']]
    hashes = {str(p.relative_to(ROOT)): digest(p) for p in files}
    removed = [p for p in before if p not in hashes]
    changed = sorted(p for p, h in hashes.items() if h != before.get(p))
    outside = [p for p in changed if p not in ALLOWED]
    if removed or outside:
        failures.append(f'P1-17 changed files outside its allowlist: removed={removed} outside={outside}')
    fixtures = {p: hashes[p] for p in sorted(hashes) if re.search(
        r'jojo-process-fixture\.rs|jojo-test-support/src/|tests/common/|scripts/tests/fake_helper|gui_p1_14\.py', p)}
    git = subprocess.run(['git', 'status', '--short'], cwd=ROOT, capture_output=True, text=True)
    if git.returncode != 128 or 'not a git repository' not in git.stderr:
        failures.append('Git precondition changed; inspect actual state')
    schema = max(int(v) for v in re.findall(r'^\s+version: (\d+),', (ROOT / 'crates/jojo-storage/src/schema.rs').read_text(), re.M))
    manifest = {
        'generated_by': 'scripts/verify_p1_17.py',
        'platform': {'os': tool(['/usr/bin/sw_vers', '-productVersion']), 'build': tool(['/usr/bin/sw_vers', '-buildVersion']),
                     'arch': platform.machine()},
        'tools': {'rustc': tool([shutil.which('rustc'), '-V']), 'cargo': tool([shutil.which('cargo'), '-V']),
                  'swift': tool([shutil.which('swift'), '--version']), 'git': tool(['/usr/bin/git', '--version']),
                  'system_sqlite3': tool(['/usr/bin/sqlite3', '--version']), 'python': platform.python_version(),
                  'toolchain_pin': (ROOT / 'rust-toolchain.toml').read_text().strip()},
        'schema': {'database_migrations': schema, 'provider_schema': 1, 'dependency_schema': 1,
                   'stdio_protocol': '1.0'},
        'configuration': {'shipped_defaults': 'global, per-provider and per-repository slots one (accepted U01-U05)',
                          'test_environment': 'OS network denial; temporary HOME/CARGO_HOME; offline locked Cargo'},
        'suite': {'rust_tests': len(passed), 'ignored': ignored, 'swift_tests': SWIFT_TESTS,
                  'gui_scenarios': {k: v['passed'] for k, v in STATE['gui'].items()}},
        'milestones': ms,
        'coverage': coverage,
        'links': link,
        'stage1': s1,
        'process_launches': ledger,
        'adapters': {k: v for k, v in net.items() if k != 'packages'},
        'dependency_licenses': deps,
        'reused_upstream_content': 'none: Phase 1 sources declare original fixtures; P1-03/P1-08/P1-09/P1-10 reports record that no upstream code, fixtures or template text were copied or adapted',
        'fixture_sha256': fixtures,
        'source_sha256': hashes,
        'files_changed_by_p1_17': changed,
        'drafts_accepted_automatically': False,
        'failures': failures,
    }
    (ROOT / 'verification/p1-17-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    if failures:
        raise RuntimeError('; '.join(failures))
    return dict(swift_tests=SWIFT_TESTS, swift_test_names=STATE['swift_test_names'],
                gui_scenarios={k: v['passed'] for k, v in STATE['gui'].items()},
                gui_evidence='verification/p1-17-gui.json', ignored_tests=ignored,
                manifest='verification/p1-17-manifest.json', coverage_requirements=len(COVERAGE),
                coverage_tests=len({t for tests in COVERAGE.values() for t in tests}),
                local_links=link['local_links'], stage1_documents=len(s1['documents']), upstream_clones=len(s1['clones']),
                launch_sites=len(ledger['sites']), files_changed=changed, input_sha256=hashes,
                git={'exit_code': git.returncode, 'stderr': git.stderr.strip()},
                verification_report='docs/verification/P1-17.md', readiness_packet='docs/verification/PHASE_1_READINESS.md',
                phase_2='NOT APPROVED: requires a separate explicit human decision',
                token_efficiency='Provenance checks reuse recorded results and hashes; one full gate. Provider token counts unavailable; no model/provider invocation.')


if __name__ == '__main__':
    try:
        shared.main(task='P1-17', extra=t17, setup=setup,
                    scope='P1-17 only: evidence, provenance and human readiness review; no implementation, Phase 2, live providers, UI migration, push or merge')
    finally:
        out = ROOT / 'verification/p1-17-result.json'
        result = json.loads(out.read_text())
        if result.get('passed') and result.get('boundary_tests') != BOUNDARY_TESTS:
            result.update(passed=False, human_readiness_review='NO-GO', error=f"boundary inventory {result.get('boundary_tests')} != {BOUNDARY_TESTS}")
            out.write_text(json.dumps(result, indent=2) + '\n')
            sys.exit(1)
