#!/usr/bin/env python3
"""P1-13 final-state gate, retaining the complete mandatory offline verification suite."""
import hashlib
import json
import re
import subprocess
from pathlib import Path
import verify_p1_04 as shared
import verify_p1_09 as p09
ROOT = Path(__file__).resolve().parents[1]

def digest(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()

def t13(output, passed):
    pre = json.loads((ROOT/'verification/p1-13-precondition-result.json').read_text())
    if not pre['passed'] or pre['rust_tests'] != 302 or pre['boundary_tests'] != 47:
        raise RuntimeError('P1-12 baseline did not pass')
    names=[]
    inventories={'crates/jojo-domain/tests/t13_protocol.rs':5,
                 'crates/jojo-helper/tests/t13_pipe.rs':5,
                 'crates/jojo-storage/tests/t13_runtime.rs':10}
    for file, count in inventories.items():
        tests=re.findall(r'#\[test\]\s*fn (\w+)\(', (ROOT/file).read_text())
        if len(tests)!=count or set(tests)-set(passed):
            raise RuntimeError(f'T13 missing tests in {file}: {set(tests)-set(passed)}')
        names+=tests
    if len(passed)!=322:
        raise RuntimeError(f'Expected 322 Rust tests, observed {len(passed)}')
    ignored=re.findall(r'^test (\S+) \.\.\. ignored',output,re.M)
    if ignored!=['child_fixture']:
        raise RuntimeError(f'Unexpected ignored tests: {ignored}')
    # Require every earlier acceptance test name to remain and pass on the final source.
    old=(ROOT/'verification/p1-13-precondition.log').read_text()
    old_names=set(re.findall(r'^test (\S+) \.\.\. ok$',old,re.M))
    if len(old_names)!=302 or old_names-set(passed):
        raise RuntimeError('Earlier acceptance inventory missing or changed')
    before=json.loads((ROOT/'verification/p1-13-before.json').read_text())
    files=[p for directory in ['crates','scripts','docs','macos','.cargo'] for p in (ROOT/directory).rglob('*')
           if p.is_file() and '__pycache__' not in p.parts and '.build' not in p.parts]
    files += [ROOT/p for p in ['Cargo.toml','Cargo.lock','rust-toolchain.toml','README.md','.gitignore','research/REPOSITORY_PINS.json']]
    hashes={str(p.relative_to(ROOT)):digest(p) for p in files}
    removed=[p for p in before if not p.startswith('_') and p not in hashes]
    if removed: raise RuntimeError(f'Removed baseline files: {removed}')
    protected=['research/REPOSITORY_PINS.json','Cargo.toml','rust-toolchain.toml',
               'docs/planning/PHASE_1_IMPLEMENTATION_BRIEF.md','docs/roadmap/IMPLEMENTATION_ROADMAP.md',
               'docs/architecture/ACCEPTED-DECISIONS-2026-11-14.md','docs/architecture/SHUTDOWN_RECOVERY.md']
    for p in protected:
        if hashes[p]!=before[p]: raise RuntimeError(f'Authoritative/protected input changed: {p}')
    git=subprocess.run(['git','status','--short'],cwd=ROOT,capture_output=True,text=True)
    if git.returncode!=128 or 'not a git repository' not in git.stderr:
        raise RuntimeError('Git precondition changed; inspect actual state')
    changed=sorted(p for p,h in hashes.items() if h!=before.get(p))
    return dict(t13_tests=len(names),t13_test_names=names,ignored_tests=ignored,
                precondition={'passed':True,'rust_tests':302,'boundary_tests':47,
                              'evidence':'verification/p1-13-precondition-result.json'},
                baseline_snapshot='verification/p1-13-before.json',
                baseline_p1_12_hash_match=True,git={'exit_code':git.returncode,'stderr':git.stderr.strip()},
                files_changed=changed,input_sha256=hashes,
                runtime_dependencies_added=[],test_dependencies_added=['jojo-storage dev-dependency on existing jojo-helper workspace crate'],
                verification_report='docs/verification/P1-13.md',invariant_changes=[],
                limitations=['Stalled workers retain helper ownership; external forced termination needs P1-11 inspection.',
                             'No reconnect to lost live stdio; durable receipt queries require released ownership.',
                             'Protocol exposes admitted launch registration internally, not arbitrary executable IPC or a scheduler loop.',
                             'Snapshot pages invalidate on concurrent commits; partial detail requires bounded detail fetch.',
                             'SwiftUI/AppKit lifecycle integration is P1-14 and has not started.'],
                token_efficiency='Used targeted source queries and reused the sandbox verifier and T11/T12 attempt fixtures. Provider token counts unavailable; no model/provider invocation.')

if __name__=='__main__':
    shared.main(task='P1-13',extra=t13,setup=p09.setup,
                scope='P1-13 only: versioned bounded stdio, receipts/snapshot/replay, worker shutdown/EOF/flush integration; no P1-14, live providers, push or merge')
