#!/usr/bin/env python3
"""M4 runtime macOS GUI scenario for repairs and reviews on Agent Runs.

Same harness as P1-14 (`gui_p1_14.Scenario`): the built JojoApp runs on WindowServer with
--ui-script. The test-only protocol fixture reports three attempts of plan steps and their
repairs, review and hold; the scenario asserts what Agent Runs lists for each and that the
app asked for them only with the one 1.5 query and sent no command.
"""
import argparse
import json
import sys
from pathlib import Path

import gui_p1_14 as g

ROOT = Path(__file__).resolve().parents[1]
FAKE = str(ROOT / 'scripts/tests/fake_helper_m4.py')


def agent_runs_shows_repairing_the_counter_the_verdict_and_the_hold(app, helper):
    s = g.Scenario(app, FAKE, 'wait:ready,sleep:500,repairs,wait:repairs,sleep:300,repairs-state,quit,sleep:8000', {})
    code = s.wait_exit()
    ev, rec = s.events(), s.received()
    shown = [e for e in ev if e.startswith('repairs-state ')]
    g.check(code == 0 and len(shown) == 1, f'exit {code}, {len(shown)} states', ev)
    rows = dict((r.split('|')[0], r.split('|')[1:]) for r in shown[0].split(' rows=', 1)[1].split(';'))
    g.check(rows.get('b2b2b2b2') == ['Repairing', 'fix · repair 1 of 2', '-', '-'],
            f'the repair attempt is Repairing with its counter: {rows}', ev)
    g.check(rows.get('a1a1a1a1') == ['Not loaded', 'fix · first attempt', 'Rejected by Claude Code', '-'],
            f'the first attempt carries the review verdict: {rows}', ev)
    g.check(rows.get('c3c3c3c3') == ['Needs human', 'never · repair 2 of 2', '-',
                                     'host validation failed; 2 of 2 repairs used'],
            f'the held step says why it waits: {rows}', ev)
    repairs = [r for r in rec if r['type'] == 'Repairs']
    g.check(len(repairs) == 1 and repairs[0]['minor'] == 5 and repairs[0]['payload'] == {},
            f'one Repairs query at 1.5: {repairs}', ev)
    commands = {'Start', 'Validate', 'CandidateDecision', 'RunPlan', 'ControlPlan', 'CancelAttempt'}
    g.check(not [r for r in rec if r['type'] in commands], 'no command was sent', ev)
    g.check(ev[-1].startswith('will-terminate outcome=clean '), 'clean quit', ev)
    return {'gui_exit': code, 'state': shown[0]}


SCENARIOS = [agent_runs_shows_repairing_the_counter_the_verdict_and_the_hold]

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--app', required=True)
    parser.add_argument('--helper', required=True)
    parser.add_argument('--out', required=True)
    parser.add_argument('--only')
    a = parser.parse_args()
    results, failed = {}, []
    for scenario in SCENARIOS:
        if a.only and scenario.__name__ != a.only:
            continue
        try:
            results[scenario.__name__] = {'passed': True, **scenario(a.app, a.helper)}
            print(f'PASS {scenario.__name__}', flush=True)
        except Exception as error:
            results[scenario.__name__] = {'passed': False, 'error': str(error)[-4000:]}
            failed.append(scenario.__name__)
            print(f'FAIL {scenario.__name__}: {error}', flush=True)
    Path(a.out).write_text(json.dumps(results, indent=2) + '\n')
    sys.exit(1 if failed else 0)
