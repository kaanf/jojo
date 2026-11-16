#!/usr/bin/env python3
"""P2-01 gate: Codex adapter and launch envelope, offline.

Re-runs the complete Phase 1 suite (Rust, boundary, Swift, GUI) on the final tree under OS
network denial, then the P2-01 checks:

* every Phase 1 test recorded by the P1-17 gate still passes, unmodified;
* the P2-01 acceptance and negative inventory passes and covers every acceptance clause;
* launch ledger: every process-launch site is classified, every provider executable named
  in a test is a fixture, and every executable the P2-01 tests got past the issue journal
  has the digest of the workspace's `jojo-codex-fixture`;
* no source names the installed Codex CLI, its package, a Codex home or a credential store;
* files changed since the P1-17 baseline are exactly the P2-01 set.

The installed Codex CLI is never located, read or executed by this gate or by any test.
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
import verify_p1_17 as p17

ROOT = Path(__file__).resolve().parents[1]
PHASE_1_RUST_TESTS = 339
P2_01_TESTS = {'crates/jojo-domain/tests/p2_01_codex.rs': 11, 'crates/jojo-storage/tests/p2_01_codex.rs': 9}
SWIFT_TESTS = 27
GUI_SCENARIOS = 9
BOUNDARY_TESTS = 65
IGNORED = ['child_fixture', 'p2_01_canary_child', 't16_helper_role']
# Scenarios the P2-01 tests run past the issue journal, each on the fixture executable.
LEDGER_SCENARIOS = sorted(['report', 'report', 'ok', 'ok', 'split', 'malformed', 'oversized', 'unknown-kind',
                           'auth-error-text', 'turn-failed', 'auth-error', 'error-then-completed',
                           'exit-zero-no-terminal', 'stderr-flood'])
# Everything P2-01 may add or change relative to the P1-17 baseline.
ALLOWED = {
    'crates/jojo-domain/src/codex.rs', 'crates/jojo-domain/src/lib.rs', 'crates/jojo-domain/src/provider.rs',
    'crates/jojo-domain/src/stream.rs', 'crates/jojo-domain/tests/p2_01_codex.rs',
    'crates/jojo-storage/src/codex.rs', 'crates/jojo-storage/src/digest.rs', 'crates/jojo-storage/src/lib.rs',
    'crates/jojo-storage/src/process.rs', 'crates/jojo-storage/src/bin/jojo-codex-fixture.rs',
    'crates/jojo-storage/tests/p2_01_codex.rs',
    'scripts/check_p1_01_boundaries.py', 'scripts/tests/test_boundaries.py', 'scripts/verify_p1_04.py',
    'scripts/verify_p2_01.py',
    'docs/planning/PHASE_2_VERTICAL_SLICE.md', 'docs/verification/P2-01.md', 'README.md',
}
# Acceptance clauses of P2-01 (docs/planning/PHASE_2_VERTICAL_SLICE.md) and the human's
# constraints, each mapped to tests that must pass in this run.
COVERAGE = {
    'Generated argv is byte-exact for a golden spec': [
        'p2_01_golden_argv_is_byte_exact',
        'p2_01_adapter_prepares_the_golden_launch_and_the_child_sees_exactly_it'],
    'Prohibited arguments cannot be constructed or issued': [
        'p2_01_prohibited_arguments_are_unrepresentable_and_refused_everywhere',
        'p2_01_arguments_environment_and_envelope_cannot_be_altered_at_the_launch_boundary'],
    'Symlink, script launcher, wrong digest, wrong version or missing file rejected before the issue journal': [
        'p2_01_symlink_script_wrong_digest_wrong_version_or_missing_executable_is_rejected',
        'p2_01_executable_changed_after_approval_is_refused_before_the_issue_journal'],
    'Inherited OPENAI_*/CODEX_* token or routing variables absent from the child environment': [
        'p2_01_inherited_provider_variables_never_reach_the_child',
        'p2_01_child_environment_is_exactly_one_selected_home_outside_the_run'],
    'Continuation specs rejected; one fresh developer run only': [
        'p2_01_profile_rejects_everything_one_exec_run_cannot_be_held_to'],
    'Envelope amendment scoped to the Codex profile; Claude and fixture validation unchanged; agent network denied': [
        'p2_01_envelope_amendment_is_scoped_to_the_production_codex_profile',
        't09_stale_or_forged_launch_authority_is_rejected_before_issue',
        't09_os_request_must_match_frozen_spec_before_issue'],
    'Versioned parser distinct from synthetic/1; split, malformed and oversized JSONL': [
        'p2_01_documented_stream_normalizes_to_one_successful_turn',
        'p2_01_split_records_are_identical_at_every_byte_boundary',
        'p2_01_malformed_oversized_and_unterminated_records_fail_closed',
        'p2_01_fixture_stream_and_split_records_succeed_with_raw_evidence',
        'p2_01_malformed_oversized_and_unknown_output_fail_closed'],
    'turn.failed, auth-error text and exit 0 without a terminal record are never success': [
        'p2_01_failure_error_and_unmodelled_completion_are_not_success',
        'p2_01_failed_turn_auth_error_and_missing_terminal_are_never_success'],
    'Unknown event kinds fail closed': [
        'p2_01_unknown_or_unmodelled_records_never_end_in_success'],
    'Stderr flood is bounded and cannot block the run': [
        'p2_01_stderr_flood_is_capped_drained_and_recorded'],
    'Capability record: every unverified item UNKNOWN': [
        'p2_01_every_unverified_capability_is_recorded_unknown'],
}
# Names of the real CLI's install and state locations. None may appear in Jojo source. The
# boundary guard and this gate name them only as patterns to refuse.
REAL_CODEX = r'/opt/homebrew|node_modules|@openai|codex-darwin|[\x22\x27/~]\.codex\b|which\(.codex|auth\.json'
SCAN_EXEMPT = {'scripts/verify_p2_01.py', 'scripts/check_p1_01_boundaries.py', 'scripts/tests/test_boundaries.py'}
STATE = {}


def digest(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


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
    out = run('Swift T14 tests (fake and real helper)', [swift, 'test', '--skip-build', *common])
    summary = re.findall(r'Executed (\d+) tests?, with (\d+) failures', out)
    swift_passed = set(re.findall(r"Test Case '-\[JojoCoreTests\.\w+ (\w+)\]' passed", out))
    names = [n for f in sorted((ROOT / 'macos/JojoApp/Tests/JojoCoreTests').glob('*.swift'))
             for n in re.findall(r'func (test_t14_\w+)\(', f.read_text())]
    if not summary or summary[-1] != (str(SWIFT_TESTS), '0') or len(names) != SWIFT_TESTS or set(names) - swift_passed:
        raise RuntimeError(f'Swift inventory failed: {summary[-1:]} missing={set(names) - swift_passed}')
    if re.search(r'skipped', out, re.I):
        raise RuntimeError('A Swift test was skipped; real-helper tests are mandatory')
    app = next((temp / 'swift-t14').rglob('Products/Debug/JojoApp'))
    report = temp / 'gui.json'
    run('Runtime macOS GUI scenarios', [sys.executable, 'scripts/gui_p1_14.py', '--app', app, '--helper', helper, '--out', report])
    gui = json.loads(report.read_text())
    if len(gui) != GUI_SCENARIOS or not all(r['passed'] for r in gui.values()):
        raise RuntimeError(f'GUI scenarios failed: {gui}')
    (ROOT / 'verification/p2-01-gui.json').write_text(json.dumps(gui, indent=2) + '\n')
    STATE.update(swift_test_names=names, gui=gui, temp=temp)


def real_binary_scan():
    hits = []
    for root in ['crates', 'macos/JojoApp/Sources', 'macos/JojoApp/Tests', 'scripts']:
        for p in sorted((ROOT / root).rglob('*')):
            rel = str(p.relative_to(ROOT))
            if p.suffix not in {'.rs', '.swift', '.py', '.toml'} or {'.build', '__pycache__'} & set(p.parts) or rel in SCAN_EXEMPT:
                continue
            for i, line in enumerate(p.read_text().splitlines(), 1):
                # Comments explain what is refused; only code and literals are scanned.
                code = re.sub(r'//.*$', '', line) if p.suffix in {'.rs', '.swift'} else line
                if re.search(REAL_CODEX, code):
                    hits.append(f'{rel}:{i}: {line.strip()[:100]}')
    return hits


def runtime_ledger():
    path = Path(STATE['temp']) / 'p2-01-launch-ledger.tsv'
    fixture = digest(Path(STATE['temp']) / 'target/debug/jojo-codex-fixture')
    rows = [line.split('\t') for line in path.read_text().splitlines()] if path.exists() else []
    foreign = [r for r in rows if len(r) != 2 or r[0] != fixture]
    return {'fixture_sha256': fixture, 'launches': len(rows), 'scenarios': sorted(r[-1] for r in rows),
            'non_fixture_launches': foreign}


def p2_01(output, passed):
    p1 = json.loads((ROOT / 'verification/p1-17-result.json').read_text())
    if not p1.get('passed') or p1.get('human_readiness_review') != 'GO':
        raise RuntimeError('P1-17 is not GO')
    failures = []
    old = set(re.findall(r'^test (\S+) \.\.\. ok$', (ROOT / 'verification/p1-17.log').read_text(), re.M))
    if len(old) != PHASE_1_RUST_TESTS or old - set(passed):
        failures.append(f'Phase 1 inventory missing or changed: {sorted(old - set(passed))}')
    names = {}
    for path, count in P2_01_TESTS.items():
        found = re.findall(r'#\[test\]\s*fn (\w+)\(', (ROOT / path).read_text())
        names[path] = found
        if len(found) != count or set(found) - set(passed):
            failures.append(f'{path}: expected {count} passing tests, missing {sorted(set(found) - set(passed))} of {len(found)}')
    new = [n for found in names.values() for n in found]
    if len(passed) != PHASE_1_RUST_TESTS + len(new):
        failures.append(f'expected {PHASE_1_RUST_TESTS + len(new)} Rust tests, observed {len(passed)}')
    ignored = sorted(re.findall(r'^test (\S+) \.\.\. ignored', output, re.M))
    if ignored != IGNORED:
        failures.append(f'unexpected ignored tests {ignored}')
    coverage = {clause: {t: t in passed for t in tests} for clause, tests in COVERAGE.items()}
    missing = sorted({t for tests in coverage.values() for t, ok in tests.items() if not ok})
    uncovered = sorted(set(new) - {t for tests in COVERAGE.values() for t in tests})
    if missing or uncovered:
        failures.append(f'coverage: not passing {missing}; not mapped to a clause {uncovered}')

    p17.LAUNCH_SITES = p17.LAUNCH_SITES + [
        (r'scripts/verify_p2_01\.py', r"\['git'", 'read-only Git state check'),
    ]
    static = p17.launch_ledger()
    if static['unclassified'] or static['non_fixture_provider_executables'] or static['name_based_kill_tools']:
        failures.append(f"launch ledger: {static['unclassified']} {static['non_fixture_provider_executables']} {static['name_based_kill_tools']}")
    runtime = runtime_ledger()
    if runtime['non_fixture_launches'] or runtime['scenarios'] != LEDGER_SCENARIOS:
        failures.append(f"runtime launch ledger: foreign={runtime['non_fixture_launches']} scenarios={runtime['scenarios']}")
    real = real_binary_scan()
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

    before = p1['input_sha256']
    files = [p for directory in ['crates', 'scripts', 'docs', 'macos', '.cargo'] for p in (ROOT / directory).rglob('*')
             if p.is_file() and not {'__pycache__', '.build', '.swiftpm'} & set(p.parts)]
    files += [ROOT / p for p in ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'README.md', '.gitignore', 'research/REPOSITORY_PINS.json']]
    hashes = {str(p.relative_to(ROOT)): digest(p) for p in files}
    removed = [p for p in before if p not in hashes]
    changed = sorted(p for p, h in hashes.items() if h != before.get(p))
    outside = [p for p in changed if p not in ALLOWED]
    if removed or outside:
        failures.append(f'P2-01 changed files outside its allowlist: removed={removed} outside={outside}')
    git = subprocess.run(['git', 'status', '--short'], cwd=ROOT, capture_output=True, text=True)
    if git.returncode != 128 or 'not a git repository' not in git.stderr:
        failures.append('Git precondition changed; inspect actual state')

    manifest = {
        'generated_by': 'scripts/verify_p2_01.py',
        'suite': {'rust_tests': len(passed), 'phase_1_rust_tests': len(old), 'p2_01_tests': names, 'ignored': ignored,
                  'swift_tests': SWIFT_TESTS, 'gui_scenarios': {k: v['passed'] for k, v in STATE['gui'].items()}},
        'coverage': coverage,
        'process_launches': static,
        'runtime_launch_ledger': runtime,
        'real_codex_references': real,
        'adapters': {k: v for k, v in net.items() if k != 'packages'},
        'links': link,
        'stage1': s1,
        'files_changed_since_p1_17': changed,
        'source_sha256': hashes,
        'failures': failures,
    }
    (ROOT / 'verification/p2-01-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    if failures:
        raise RuntimeError('; '.join(failures))
    return dict(phase_1_rust_tests=len(old), p2_01_tests=len(new), p2_01_test_names=new,
                swift_tests=SWIFT_TESTS, gui_scenarios={k: v['passed'] for k, v in STATE['gui'].items()},
                gui_evidence='verification/p2-01-gui.json', ignored_tests=ignored,
                manifest='verification/p2-01-manifest.json', coverage_clauses=len(COVERAGE),
                launch_sites=len(static['sites']), fixture_launches=runtime['launches'],
                fixture_sha256=runtime['fixture_sha256'], real_codex_executed=False,
                files_changed=changed, input_sha256=hashes,
                git={'exit_code': git.returncode, 'stderr': git.stderr.strip()},
                verification_report='docs/verification/P2-01.md',
                live_attempt='NOT AUTHORIZED: G-LIVE decisions (U06, Codex home, usage limit, provider network) remain open')


if __name__ == '__main__':
    try:
        shared.main(task='P2-01', extra=p2_01, setup=setup,
                    scope='P2-01 only: offline Codex adapter, launch envelope and parser; fixture executables only; '
                          'no live Codex execution, credential access, network inference, P2-02, push or merge')
    finally:
        out = ROOT / 'verification/p2-01-result.json'
        result = json.loads(out.read_text())
        if result.get('passed') and result.get('boundary_tests') != BOUNDARY_TESTS:
            result.update(passed=False, p2_02='NO-GO', error=f"boundary inventory {result.get('boundary_tests')} != {BOUNDARY_TESTS}")
            out.write_text(json.dumps(result, indent=2) + '\n')
            sys.exit(1)
