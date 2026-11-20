#!/usr/bin/env python3
"""The gate (Refactor R-03): one runner, driven by a data file.

    python3 scripts/verify.py                       # scripts/gates/current.json, every stage
    python3 scripts/verify.py --skip gui            # leave a stage out; the result says so
    python3 scripts/verify.py --gate scripts/gates/<name>.json

Replaces the `verify_p*_*.py` chain for new work. Those scripts and their results stay
as historical records of their milestones and are not run again; this file imports none
of them. What a gate requires (minimum test counts, tests that must pass, GUI scenario
counts) lives in the gate file, so the runner stays valid as the tree moves.

Every command runs under an OS sandbox that denies network access, with a temporary
HOME/CARGO_HOME and an explicit environment. Nothing is installed or downloaded and no
provider CLI is launched. A stage that is skipped is recorded as skipped: the result is
then `passed` for what ran and `complete: false`.
"""
from pathlib import Path
import argparse
import hashlib
import importlib.util
import json
import os
import re
import shutil
import signal
import stat
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / 'verification'
SANDBOX = '(version 1) (allow default) (deny network*)'
STAGES = ['static', 'rust', 'swift', 'gui']
PROBE = ('import errno,socket; s=socket.socket(); r=s.connect_ex(("127.0.0.1",9)); s.close(); '
         'assert r in (errno.EPERM,errno.EACCES), r')


def evaluate(gate, seen):
    """Compares what the stages observed with what the gate file requires.

    Pure: `seen` holds only the stages that ran. Returns the list of failures.
    """
    errors = []

    def at_least(label, actual, minimum):
        if actual < minimum:
            errors.append(f'{label}: {actual}, gate requires at least {minimum}')

    if 'boundary_tests' in seen:
        at_least('boundary tests', seen['boundary_tests'], gate['boundary_tests_min'])
    if 'rust_passed' in seen:
        rust = gate['rust']
        at_least('Rust tests passed', len(seen['rust_passed']), rust['passed_min'])
        if seen['rust_failed']:
            errors.append(f"Rust tests failed: {sorted(seen['rust_failed'])}")
        missing = sorted(set(rust['required']) - set(seen['rust_passed']))
        if missing:
            errors.append(f'required Rust tests did not pass: {missing}')
        unexpected = sorted(set(seen['rust_ignored']) - set(rust['ignored']))
        if unexpected:
            errors.append(f'Rust tests ignored without being listed in the gate: {unexpected}')
    if 'swift_executed' in seen:
        at_least('Swift tests executed', seen['swift_executed'], gate['swift']['tests_min'])
        if seen['swift_failures']:
            errors.append(f"Swift test failures: {seen['swift_failures']}")
        if seen['swift_skipped']:
            errors.append('a Swift test was skipped; real-helper tests are mandatory')
    if 'gui' in seen:
        for script, minimum in gate['gui'].items():
            results = seen['gui'].get(script, {})
            at_least(f'GUI scenarios ({script})', len(results), minimum)
            failed = sorted(name for name, r in results.items() if not r.get('passed'))
            if failed:
                errors.append(f'GUI scenarios failed ({script}): {failed}')
    return errors


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def inputs():
    files = [ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', ROOT / 'rust-toolchain.toml', ROOT / '.cargo/config.toml',
             *[p for p in (ROOT / 'crates').rglob('*') if p.is_file() and p.suffix in {'.rs', '.toml'}],
             *(ROOT / 'macos').rglob('*.swift'),
             *[p for p in (ROOT / 'scripts').rglob('*') if p.is_file() and p.suffix in {'.py', '.json'}]]
    return {str(p.relative_to(ROOT)): digest(p)
            for p in sorted(files) if '.build' not in p.parts and '__pycache__' not in p.parts}


def run_gate(gate, skip, result, log):
    tools = {name: shutil.which(name) for name in ['cargo', 'rustc', 'swift', 'git', 'sandbox-exec']}
    if sys.platform != 'darwin' or not all(tools.values()):
        raise RuntimeError('the gate needs preinstalled macOS Rust, Swift, Git and sandbox-exec; nothing is installed')
    cargo, swift = tools['cargo'], tools['swift']
    seen = {}
    with tempfile.TemporaryDirectory(prefix='jojo-gate-') as scratch:
        temp = Path(scratch)
        for name in ['home', 'cargo', 'tmp', 'cache', 'clang-cache', 'swift-cache']:
            (temp / name).mkdir(mode=0o700)
        # Only registry artifacts are reused; no Cargo configuration or credentials.
        registry = Path(os.environ.get('CARGO_HOME', Path.home() / '.cargo')) / 'registry'
        if not registry.is_dir():
            raise RuntimeError('cache dependencies first: cargo fetch --locked')
        (temp / 'cargo/registry').symlink_to(registry.resolve(), target_is_directory=True)
        env = {
            'HOME': str(temp / 'home'), 'CARGO_HOME': str(temp / 'cargo'), 'CARGO_TARGET_DIR': str(temp / 'target'),
            'CARGO_NET_OFFLINE': 'true', 'RUSTUP_AUTO_INSTALL': '0', 'RUSTC': tools['rustc'],
            'PATH': os.pathsep.join(dict.fromkeys([str(Path(tools['rustc']).parent), '/usr/bin', '/bin', '/usr/sbin', '/sbin'])),
            'TMPDIR': str(temp / 'tmp'), 'XDG_CACHE_HOME': str(temp / 'cache'),
            'CLANG_MODULE_CACHE_PATH': str(temp / 'clang-cache'), 'SWIFTPM_MODULECACHE_OVERRIDE': str(temp / 'swift-cache'),
            'GIT_CONFIG_NOSYSTEM': '1', 'GIT_OPTIONAL_LOCKS': '0', 'PYTHONDONTWRITEBYTECODE': '1', 'LANG': 'en_US.UTF-8',
        }

        def run(label, argv, expected=0, timeout=900):
            done = subprocess.run([tools['sandbox-exec'], '-p', SANDBOX, *map(str, argv)], cwd=ROOT, env=env, text=True,
                                  stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)
            print(f'{label}: exit {done.returncode}', flush=True)
            log.append(f'$ {label}\n{done.stdout}{done.stderr}\n')
            result['checks'].append({'check': label, 'exit_code': done.returncode, 'expected_exit_code': expected})
            if done.returncode != expected:
                raise RuntimeError(f'{label} failed (exit {done.returncode}); see the log')
            return done.stdout + done.stderr

        run('OS network-denial probe', [sys.executable, '-c', PROBE])
        result['toolchains'] = {name: run(f'{name} identity', [tools[name], '--version']).strip().splitlines()[0]
                                for name in ['rustc', 'cargo', 'swift']}

        if 'static' not in skip:
            run('static architecture boundary', [sys.executable, 'scripts/check_p1_01_boundaries.py'])
            out = run('boundary regression tests', [sys.executable, '-m', 'unittest', 'discover', '-s', 'scripts/tests', '-v'])
            seen['boundary_tests'] = int(re.search(r'Ran (\d+) tests', out)[1])
            metadata = json.loads(subprocess.run(
                [tools['sandbox-exec'], '-p', SANDBOX, cargo, 'metadata', '--offline', '--locked', '--format-version', '1'],
                cwd=ROOT, env=env, text=True, capture_output=True, check=True, timeout=240).stdout)
            spec = importlib.util.spec_from_file_location('boundaries', ROOT / 'scripts/check_p1_01_boundaries.py')
            boundary = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(boundary)
            approved = {(p['name'], p['version'], p.get('source')) for p in boundary.APPROVED_PACKAGES}
            if {(p['name'], p['version'], p['source']) for p in metadata['packages']} != approved:
                raise RuntimeError('resolved packages differ from the approved dependency graph')
            baseline = json.loads((EVIDENCE / 'p1-01-preservation-baseline.json').read_text())
            changed = [d for d, sha in baseline['documents'].items() if digest(ROOT / d) != sha]
            if changed:
                raise RuntimeError(f'historical documents modified: {changed}')
            for clone in baseline['clones']:
                directory = ROOT / 'research/upstream' / clone['name']
                head = run(f"preserved {clone['name']} pin", [tools['git'], '-C', directory, 'rev-parse', 'HEAD']).strip()
                if head != clone['head']:
                    raise RuntimeError(f"research checkout moved: {clone['name']}")
            run('Rust formatting', [cargo, 'fmt', '--all', '--', '--check'])

        helper = temp / 'target/debug/jojo-helper'
        if not {'rust', 'swift', 'gui'} <= skip:
            # sandbox-exec refuses the setuid /bin/ps: the tests get the same binary without
            # privilege bits, ad-hoc signed. Not a shim and not a download.
            ps = temp / 'ps-unprivileged'
            shutil.copyfile('/bin/ps', ps)
            ps.chmod(0o700)
            assert stat.S_IMODE(ps.stat().st_mode) == 0o700 and digest(ps) == digest(Path('/bin/ps'))
            run('sign unprivileged host ps copy', ['/usr/bin/codesign', '--force', '--sign', '-', ps])
            env['JOJO_TEST_PS'] = str(ps)
            env['JOJO_P2_01_LEDGER'] = str(temp / 'p2-01-launch-ledger.tsv')
            # Set before the first compile: every cargo invocation below sees one environment.
            env['JOJO_HELPER_BIN'] = str(helper)
            # The same sequence the milestone gates used: test targets first, then the helper.
            run('Rust test targets compile', [cargo, 'test', '--offline', '--locked', '--workspace', '--all-targets', '--no-run'],
                timeout=1800)
            run('helper binary', [cargo, 'build', '--offline', '--locked', '-p', 'jojo-helper'])

        if 'rust' not in skip:
            out = run('Rust tests', [cargo, 'test', '--offline', '--locked', '--workspace', '--all-targets'], timeout=1800)
            seen['rust_passed'] = re.findall(r'^test (\S+) \.\.\. ok$', out, re.M)
            seen['rust_failed'] = re.findall(r'^test (\S+) \.\.\. FAILED$', out, re.M)
            seen['rust_ignored'] = re.findall(r'^test (\S+) \.\.\. ignored', out, re.M)
            for item in gate['rust'].get('unsandboxed_ignored', []):
                # The one kind of step outside the OS sandbox: a test that drives the real helper
                # needs the system `ps`. Still offline, fixture executables only.
                label = f"{item['test']} (outside the OS sandbox: system ps required)"
                done = subprocess.run([cargo, 'test', '--offline', '--locked', '-p', item['package'], '--test', item['target'],
                                       '--', '--ignored'], cwd=ROOT, env=env, capture_output=True, text=True, timeout=600)
                print(f'{label}: exit {done.returncode}', flush=True)
                log.append(f'$ {label}\n{done.stdout}{done.stderr}\n')
                result['checks'].append({'check': label, 'exit_code': done.returncode, 'expected_exit_code': 0})
                if done.returncode == 0 and f"test {item['test']} ... ok" in done.stdout:
                    seen['rust_passed'].append(item['test'])
                else:
                    seen['rust_failed'].append(item['test'])
            run('Rust documentation tests', [cargo, 'test', '--offline', '--locked', '--workspace', '--doc'])
            run('Rust Clippy', [cargo, 'clippy', '--offline', '--locked', '--workspace', '--all-targets', '--', '-D', 'warnings'])
            run('helper checks configuration only', [helper, '--check-config', '--data-root', temp / 'explicit-data',
                                                      '--worktree-root', temp / 'explicit-worktrees'])
            run('helper has no implicit runtime', [helper], expected=2)
            if (temp / 'explicit-data').exists() or (temp / 'explicit-worktrees').exists():
                raise RuntimeError('--check-config created runtime directories')

        common = ['--package-path', 'macos/JojoApp', '--scratch-path', temp / 'swift-build', '--cache-path',
                  temp / 'swiftpm-cache', '--disable-sandbox', '--disable-automatic-resolution']
        if not {'swift', 'gui'} <= skip:
            run('Swift app, core and test build', [swift, 'build', '--build-tests', *common], timeout=1800)
        if 'swift' not in skip:
            out = run('Swift tests (fake and real helper)', [swift, 'test', '--skip-build', *common], timeout=1800)
            summary = re.findall(r'Executed (\d+) tests?, with (\d+) failures', out)
            if not summary:
                raise RuntimeError('Swift test summary not found')
            seen['swift_executed'], seen['swift_failures'] = int(summary[-1][0]), int(summary[-1][1])
            seen['swift_skipped'] = bool(re.search(r'skipped', out, re.I))
        if 'gui' not in skip:
            app = next((temp / 'swift-build').rglob('Products/Debug/JojoApp'))
            seen['gui'] = {}
            for script in gate['gui']:
                report = temp / f'{Path(script).stem}.json'
                # caffeinate: a display or system sleep mid-scenario fails the harness, not the product.
                run(f'GUI scenarios ({script})', ['/usr/bin/caffeinate', '-dimsu', sys.executable, script, '--app', app,
                                                  '--helper', helper, '--out', report], timeout=1800)
                seen['gui'][script] = json.loads(report.read_text())
        # R-04: nothing a check started may outlive the gate. On 2026-11-18 three fake `gh`
        # processes of earlier gates were found still running hours later. A process running
        # from this scratch directory is stopped and fails the gate.
        leaked = left_running(temp)
        if leaked:
            raise RuntimeError(f'processes outlived the checks and were stopped: {leaked}')
    return seen


def left_running(temp):
    roots = {str(temp), str(temp.resolve())}
    listing = subprocess.run(['/bin/ps', '-axo', 'pid=,command='], capture_output=True, text=True, check=True).stdout
    leaked = []
    for line in listing.splitlines():
        pid, _, command = line.strip().partition(' ')
        if pid.isdigit() and int(pid) != os.getpid() and any(root in command for root in roots):
            leaked.append(command.strip()[:200])
            try:
                os.kill(int(pid), signal.SIGKILL)
            except ProcessLookupError:
                pass
    return leaked


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--gate', default='scripts/gates/current.json')
    parser.add_argument('--skip', action='append', default=[], choices=STAGES)
    arguments = parser.parse_args()
    gate = json.loads((ROOT / arguments.gate).read_text())
    skip = set(arguments.skip)
    stem = gate['artifact']
    target = EVIDENCE / f'{stem}-result.json'
    result = {'gate': gate['artifact'], 'gate_file': arguments.gate, 'passed': False, 'status': 'running', 'checks': []}
    target.write_text(json.dumps(result) + '\n')
    log = []
    try:
        seen = run_gate(gate, skip, result, log)
        errors = evaluate(gate, seen)
        result.update(
            status='finished', passed=not errors, complete=not skip, skipped=sorted(skip), errors=errors,
            scope=gate['scope'], not_covered=gate.get('not_covered', []),
            network='OS sandbox denied; loopback probe confirmed', credentials='none inherited; temporary HOME/CARGO_HOME',
            counts={'boundary_tests': seen.get('boundary_tests'), 'rust_passed': len(seen.get('rust_passed', [])),
                    'rust_ignored': seen.get('rust_ignored'), 'swift_executed': seen.get('swift_executed'),
                    'gui_scenarios': {s: len(r) for s, r in seen.get('gui', {}).items()}},
            gui=seen.get('gui'), input_sha256=inputs())
    except (RuntimeError, AssertionError, subprocess.SubprocessError, StopIteration) as error:
        result.update(status='failed', passed=False, errors=[str(error)])
    finally:
        (EVIDENCE / f'{stem}.log').write_text('\n'.join(log))
        target.write_text(json.dumps(result, indent=2) + '\n')
    for error in result['errors']:
        print(f'FAIL: {error}', file=sys.stderr)
    if result['passed']:
        print(f"{stem}: PASS{'' if result['complete'] else ' (incomplete: skipped ' + ', '.join(sorted(skip)) + ')'}")
    return 0 if result['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
