#!/usr/bin/env python3
"""T01–T06 through the credential-free, OS-network-denied offline runner."""
import hashlib
import json
import re
from pathlib import Path
import verify_p1_04 as shared

ROOT = Path(__file__).resolve().parents[1]

def t06(output, passed):
    pre = json.loads((ROOT/'verification/p1-06-precondition-result.json').read_text())
    if not pre['passed'] or pre['task'] != 'P1-05' or pre['rust_tests'] != 123 or pre['t05_total'] != 29 or pre['boundary_tests'] != 26:
        raise RuntimeError('P1-05 prerequisite did not pass the expected inventory')
    names = re.findall(r'#\[test\]\s*fn (\w+)\(', (ROOT/'crates/jojo-storage/tests/t06_scheduler.rs').read_text())
    if len(names) != 29 or set(names)-set(passed):
        raise RuntimeError(f'T06 missing or failed: {set(names)-set(passed)}; inventory={len(names)}')
    sql = ['schema::tests::scheduler_configuration_and_claim_quantities_are_durable_and_versioned',
           'schema::tests::single_unsealed_step_constraint_also_covers_legacy_dependency_attempts']
    if set(sql)-set(passed): raise RuntimeError('T06 SQL constraints did not pass')
    for crate in ['jojo-domain','jojo-storage']:
        old = re.findall(r'#\[test\]\s*fn (\w+)\(', (ROOT/f'crates/{crate}/tests/t05_dependencies.rs').read_text())
        if len(old) != 14 or set(old)-set(passed): raise RuntimeError(f'T05 regression failed: {crate}')
    ignored = re.findall(r'^test (\S+) \.\.\. ignored', output, re.M)
    if ignored != ['child_fixture']: raise RuntimeError(f'Unexpected ignored tests: {ignored}')
    files = [p for directory in ['crates','scripts','docs'] for p in (ROOT/directory).rglob('*') if p.is_file() and p.suffix in {'.rs','.py','.md','.toml'}]
    before = json.loads((ROOT/'verification/p1-06-before.json').read_text())
    hashes = {str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in files}
    changed = sorted(p for p,digest in hashes.items() if digest != before.get(p))
    return {'t06_integration_tests':len(names), 't06_sql_tests':len(sql), 't06_total':len(names)+len(sql),
            't06_test_names':names, 't05_total':29, 'ignored_tests':ignored, 'schema_version':5,
            'execution_plan_schema_versions':[1,2], 'files_changed':changed,
            'precondition':{'task':'P1-05','passed':True,'rust_tests':123,'t05_total':29,'boundary_tests':26},
            'input_sha256':{p:hashes[p] for p in changed},
            'verification_report':'docs/verification/P1-06.md',
            'known_limitations':[
                'No dispatcher, provider execution, process inspector or worktree implementation.',
                'Repository identities and source observations are controlled facts; canonical Git identity validation is P1-07.',
                'Schema-1 plans remain readable; admission requires reviewed schema-2 resource declarations.',
                'An admission envelope currently names one source repository per step; multi-repository execution envelopes fail closed.',
                'Queue entries preserve a step revision and its plan binding; material rebinding uses a new reviewed step revision.',
                'Issued/unknown and prior-epoch reservations retain claims; their resolution is future lifecycle/recovery work.',
                'Fairness assumes finite incumbents or explicit resolution of unknown work; no preemption or unbounded-arrival guarantee.'
            ]}

if __name__ == '__main__':
    shared.main(task='P1-06', extra=t06)
