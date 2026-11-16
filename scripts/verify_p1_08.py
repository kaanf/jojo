#!/usr/bin/env python3
"""P1-08 and all prior regressions, offline and OS-network-denied."""
import hashlib
import json
import re
import platform
import tomllib
from pathlib import Path
import verify_p1_04 as shared
ROOT = Path(__file__).resolve().parents[1]

def t08(output, passed):
    pre = json.loads((ROOT / 'verification/p1-08-precondition-result.json').read_text())
    if not pre['passed'] or pre['task'] != 'P1-07' or pre['rust_tests'] != 193 or pre['t07_total'] != 39:
        raise RuntimeError('P1-07 prerequisite inventory failed')
    paths = ['crates/jojo-test-support/tests/t08_providers.rs', 'crates/jojo-storage/tests/t08_authority.rs']
    names = [n for p in paths for n in re.findall(r'#\[test\]\s*fn (\w+)\(', (ROOT / p).read_text())]
    if len(names) != 16 or set(names) - set(passed):
        raise RuntimeError(f'T08 inventory failed: {set(names) - set(passed)}')
    if len(passed) != 209:
        raise RuntimeError(f'Expected 209 Rust tests, got {len(passed)}')
    ignored = re.findall(r'^test (\S+) \.\.\. ignored', output, re.M)
    if ignored != ['child_fixture']:
        raise RuntimeError(f'Unexpected ignored tests: {ignored}')
    before = json.loads((ROOT / 'verification/p1-08-before.json').read_text())
    files = [p for d in ['crates','scripts','docs'] for p in (ROOT/d).rglob('*') if p.is_file() and p.suffix in {'.rs','.py','.md','.toml'}]
    hashes = {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in files}
    changed = sorted(p for p, digest in hashes.items() if digest != before.get(p))
    return dict(platform=platform.platform(), rust_toolchain=tomllib.loads((ROOT/'rust-toolchain.toml').read_text())['toolchain'],
                t08_total=len(names), t08_test_names=names, ignored_tests=ignored, schema_version=6,
                provider_schema_version=1, files_changed=changed, input_sha256={p: hashes[p] for p in changed},
                precondition={'task':'P1-07','passed':True,'rust_tests':193,'t07_total':39},
                verification_report='docs/verification/P1-08.md', invariant_changes=[],
                provenance='All DTOs, fake scripts and T08 fixtures authored locally; no upstream protocol fixture adapted.',
                known_limitations=['Pure in-memory synthetic adapters; no installed CLI compatibility or auth/billing certification.',
                    'LaunchSpec validation is structural; scheduler, approval, digest and repository authority remain external.',
                    'Real process identity, capture, parsing, stop/exit supervision and durable launch issuance begin in P1-09.',
                    'Result classification and independent validation remain P1-10; recovery orchestration remains P1-11.'])

if __name__ == '__main__':
    shared.main(task='P1-08', extra=t08, scope='P1-08 only; provider-neutral DTOs and deterministic synthetic adapters; no process management or live providers')
