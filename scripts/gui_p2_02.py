#!/usr/bin/env python3
"""P2-02 runtime macOS GUI scenarios for the Start and recovery controls.

Same harness as P1-14 (`gui_p1_14.Scenario`): the built JojoApp runs on WindowServer with
--ui-script, whose steps call the same entry points as the Start and Recovery tabs. One
scenario uses the real jojo-helper with a disposable root and never gets as far as a
launch (admission, then approvals, refuse it). Two use the test-only protocol 1.1 fixture
to assert exactly what the controls send. Nothing here starts a provider.
"""
import argparse
import json
import subprocess
import sys
from pathlib import Path

import gui_p1_14 as g

ROOT = Path(__file__).resolve().parents[1]
FAKE = ROOT / 'scripts/tests/fake_helper_p2_02.py'
ATTEMPT = '000000000000000000000000000000a2'
SUBJECTS = [f'{n:032x}' for n in (701, 702, 703, 704, 1)]
START = 'start:' + '/'.join(SUBJECTS)
START_PAYLOAD = dict(zip(['prd', 'adr', 'plan', 'validation', 'step'], SUBJECTS))


class Slice(g.Scenario):
    """A real helper started by the app with a disposable root (slice options forwarded)."""

    def __init__(self, app, helper, script):
        self.temp = Path(g.tempfile.mkdtemp(prefix='jojo-gui-'))
        self.data, self.worktrees, self.trace = self.temp / 'data', self.temp / 'worktrees', self.temp / 'trace.jsonl'
        self.helper = helper
        (self.temp / 'disposable').mkdir(mode=0o700)
        self.proc = subprocess.Popen([app, '--helper', helper, '--data-root', self.data, '--worktree-root',
                                      self.worktrees, '--disposable-root', self.temp / 'disposable',
                                      '--trace', self.trace, '--ui-script', script],
                                     stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)


def real_start_refused_until_reviewed_and_approved(app, helper):
    s = Slice(app, helper, f'wait:ready,{START},wait:status=Start refused (AdmissionClosed),state,'
                           'recovery-inspect,wait:status=Inspection refreshed,recovery-start,wait:status=Admission open,'
                           f'wait:admission=Open,{START},wait:status=Start refused (StartRefused,state,quit,sleep:8000')
    code = s.wait_exit()
    ev = s.events()
    states = [e for e in ev if e.startswith('state ')]
    g.check(code == 0, f'exit {code}', ev)
    g.check(sum(e.startswith('client send Start ') for e in ev) == 2, 'two explicit Starts, both refused', ev)
    g.check('admission=Closed' in states[0] and 'AdmissionClosed' in states[0], 'closed admission refuses Start', ev)
    g.check('admission=Open' in states[1] and 'missing or stale approval: PRD' in states[1],
            'open admission is not approval', ev)
    g.check(all('pending=false' in st for st in states), 'refusals leave nothing outstanding', ev)
    g.check(ev[-1].startswith('will-terminate outcome=clean '), 'clean quit after refusals', ev)
    probe = s.probe()
    g.check('previous=clean' in probe, probe, ev)
    g.check(not any(s.worktrees.glob('*')) if s.worktrees.exists() else True, 'no worktree was created', ev)
    return {'gui_exit': code, 'states': states, 'probe': probe.strip().splitlines()[0]}


def fake_start_and_recovery_controls(app, helper):
    s = g.Scenario(app, str(FAKE),
                   f'wait:ready,sleep:800,recovery-resolve:{ATTEMPT}/ValidateOutput,wait:status=Recovery choice refused,'
                   f'recovery-resolve:{ATTEMPT}/Hold,wait:status=Recovery choice Hold committed,sleep:500,'
                   f'{START},wait:status=Start refused (AdmissionClosed),recovery-start,wait:status=Admission open,'
                   f'wait:admission=Open,{START},wait:status=Start accepted,state,quit,sleep:8000', {})
    code = s.wait_exit()
    ev, rec = s.events(), s.received()
    got = [r['type'] for r in rec]
    state = next(e for e in ev if e.startswith('state '))
    resolves = [r['payload'] for r in rec if r['type'] == 'RecoveryResolve']
    starts = [r for r in rec if r['type'] == 'Start']
    g.check(code == 0, f'exit {code}', ev)
    g.check(resolves == [{'digest': 'fixture-1', 'attempt': ATTEMPT, 'choice': 'ValidateOutput'},
                         {'digest': 'fixture-1', 'attempt': ATTEMPT, 'choice': 'Hold'}],
            f'choices bind the inspected digest: {resolves}', ev)
    g.check(got.index('RecoveryInspect') > [i for i, t in enumerate(got) if t == 'RecoveryResolve'][-1],
            'a committed choice is followed by a fresh inspection', ev)
    opened = next(r['payload'] for r in rec if r['type'] == 'RecoveryStart')
    g.check(opened == {'digest': 'fixture-2'}, f'reviewed start binds the fresh digest: {opened}', ev)
    g.check([r['payload'] for r in starts] == [START_PAYLOAD, START_PAYLOAD], f'Start carries identities only: {starts}', ev)
    g.check(len({r['id'] for r in starts}) == 2, 'each explicit Start is its own command', ev)
    g.check(all(r['minor'] == 1 for r in rec if 'minor' in r), 'protocol 1.1 frames', ev)
    g.check(not set(got) & {'CancelAttempt', 'HumanDecision', 'ConfirmClose', 'RegisterRepository', 'ImportBundle'},
            f'only the clicked controls sent commands: {got}', ev)
    g.check('admission=Open' in state and 'pending=false' in state and 'Start accepted for attempt' in state,
            'accepted Start is shown as accepted, not completed', ev)
    g.check(ev[-1].startswith('will-terminate outcome=clean '), 'shutdown behaviour unchanged', ev)
    return {'gui_exit': code, 'helper_commands': got, 'state': state}


def fake_lost_start_response_is_reconciled_not_resent(app, helper):
    s = g.Scenario(app, str(FAKE), f'wait:ready,{START},sleep:400,{START},{START},state,quit,wait:failed,state,'
                                   'ack-unsettled,quit,sleep:8000',
                   {'admission': 'Open', 'answer_start': False})
    code = s.wait_exit()
    ev, rec = s.events(), s.received()
    got = [r['type'] for r in rec]
    states = [e for e in ev if e.startswith('state ')]
    start = next(r for r in rec if r['type'] == 'Start')
    lookup = next(r for r in rec if r['type'] == 'Receipt')
    g.check(code == 0, f'exit {code}', ev)
    g.check(got.count('Start') == 1, f'an unanswered Start is never sent again: {got}', ev)
    g.check('pending=true' in states[0], 'the control stays disabled while the outcome is unknown', ev)
    g.check(lookup['payload'] == {'command': start['id']}, 'receipt lookup under the original identity', ev)
    g.check(got.index('Receipt') < got.index('PrepareClose'), 'reconcile before closing', ev)
    g.check(states[1].startswith('state phase=failed'), 'unconfirmed Start is visible at quit', ev)
    g.check(ev[-1].startswith('will-terminate outcome=clean-with-unconfirmed-commands'), 'no full preservation claim', ev)
    return {'gui_exit': code, 'helper_commands': got}


SCENARIOS = [real_start_refused_until_reviewed_and_approved, fake_start_and_recovery_controls,
             fake_lost_start_response_is_reconciled_not_resent]

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
