#!/usr/bin/env python3
"""Offline P1-04 regression. No upstream audit, providers or remote operations."""
from pathlib import Path
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'verification'

def main(task="P1-04", extra=None, scope=None, setup=None):
    checks = []
    log = []
    stem = task.lower()
    recommendation = {'P2-04.1':'p2_05_single_supervised_attempt', 'P2-04':'p2_05_authorization_request', 'P2-03':'p2_04', 'P2-02':'p2_03', 'P2-01':'p2_02', 'P1-17':'human_readiness_review', 'P1-16':'p1_17', 'P1-15':'p1_16', 'P1-14':'p1_15', 'P1-13':'p1_14', 'P1-12':'p1_13', 'P1-11':'p1_12', 'P1-10':'p1_11', 'P1-09':'p1_10', 'P1-08':'p1_09', 'P1-07':'p1_08', 'P1-06':'p1_07', 'P1-05':'p1_06'}.get(task,'p1_05')
    result = {'task': task, 'passed': False, recommendation: 'NO-GO'}
    (OUT / f'{stem}-result.json').write_text(json.dumps(result) + '\n')
    try:
        with tempfile.TemporaryDirectory(prefix='jojo-t04-') as scratch:
            temp = Path(scratch)
            for name in ['home', 'cargo', 'tmp', 'cache']:
                (temp / name).mkdir(mode=0o700)
            registry = Path(os.environ.get('CARGO_HOME', Path.home() / '.cargo')) / 'registry'
            (temp / 'cargo/registry').symlink_to(registry.resolve(), target_is_directory=True)
            cargo, rustc, swift = [shutil.which(x) for x in ['cargo', 'rustc', 'swift']]
            env = {'HOME': str(temp/'home'), 'CARGO_HOME': str(temp/'cargo'),
                   'CARGO_TARGET_DIR': str(temp/'target'), 'CARGO_NET_OFFLINE': 'true',
                   'RUSTUP_AUTO_INSTALL': '0', 'RUSTC': rustc,
                   'PATH': str(Path(rustc).parent)+':/usr/bin:/bin:/usr/sbin:/sbin',
                   'TMPDIR': str(temp/'tmp'), 'XDG_CACHE_HOME': str(temp/'cache'),
                   'CLANG_MODULE_CACHE_PATH': str(temp/'clang-cache'),
                   'SWIFTPM_MODULECACHE_OVERRIDE': str(temp/'swift-cache'),
                   'GIT_CONFIG_NOSYSTEM': '1', 'PYTHONDONTWRITEBYTECODE': '1'}
            def run(label, args):
                p = subprocess.run(['/usr/bin/sandbox-exec', '-p', '(version 1) (allow default) (deny network*)', *map(str,args)], cwd=ROOT, env=env, capture_output=True, text=True, timeout=240)
                output = p.stdout+p.stderr
                log.append(f'$ {label}\n{output}')
                checks.append({'check':label,'exit_code':p.returncode})
                print(f'{label}: exit {p.returncode}',flush=True)
                if p.returncode: raise RuntimeError(f'{label} failed; see {stem}.log')
                return output
            run('OS network denial', [sys.executable,'-c','import socket,errno; s=socket.socket(); assert s.connect_ex(("127.0.0.1",9)) in (errno.EPERM,errno.EACCES); s.close()'])
            if setup:
                setup(env, temp, run)
            run('Offline boundary', [sys.executable,'scripts/check_p1_01_boundaries.py'])
            boundary = run('Boundary regression', [sys.executable,'-m','unittest','discover','-s','scripts/tests','-v'])
            run('Formatting',[cargo,'fmt','--all','--','--check'])
            tests = run('Full Rust regression',[cargo,'test','--offline','--locked','--workspace','--all-targets'])
            run('Rust doctests',[cargo,'test','--offline','--locked','--workspace','--doc'])
            run('Clippy',[cargo,'clippy','--offline','--locked','--workspace','--all-targets','--','-D','warnings'])
            run('Swift boundary build',[swift,'build','--package-path','macos/JojoApp','--scratch-path',temp/'swift-build','--cache-path',temp/'swift-cache','--disable-sandbox','--disable-automatic-resolution'])
            passed = re.findall(r'^test (\S+) \.\.\. ok$',tests,re.M)
            t04_names = re.findall(r'^fn (\w+)\(', (ROOT/'crates/jojo-storage/tests/t04_approvals.rs').read_text(), re.M)
            t04_count = len(set(t04_names) & set(passed))
            if t04_count != 16: raise RuntimeError('Expected all 16 T04 tests')
            additions = extra(tests, passed) if extra else {}
            result.update(passed=True,rust_tests=len(passed),t04_tests=t04_count,
                          boundary_tests=int(re.search(r'Ran (\d+) tests',boundary)[1]),
                          rust_failures=0,network='OS denied; offline locked Cargo',
                          credentials='temporary HOME/CARGO_HOME; environment allowlist',
                          scope=scope or f'{task} only; '+('scheduling only; ' if task=='P1-06' else 'no scheduler; ')+'no provider execution, remote operations, upstream audits or mutation campaign', **additions)
            result[recommendation] = 'GO'
    except Exception as error:
        result['error']=str(error)
        raise
    finally:
        result['checks']=checks
        (OUT/f'{stem}.log').write_text('\n'.join(log))
        (OUT/f'{stem}-result.json').write_text(json.dumps(result,indent=2)+'\n')

if __name__=='__main__':
    main()
