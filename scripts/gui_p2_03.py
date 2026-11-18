#!/usr/bin/env python3
"""P2-03 runtime macOS GUI scenarios for candidate review, validation and acceptance.

Same harness as P1-14 (`gui_p1_14.Scenario`): the built JojoApp runs on WindowServer with
--ui-script, whose steps call the same entry points as the Candidate tab. Four scenarios
use the test-only protocol fixture to assert what the tab shows for each validation state
and exactly what its controls send; one uses the real jojo-helper. Nothing here starts a
provider or a validation command.
"""
import argparse
import json
import sys
from pathlib import Path

import gui_p1_14 as g
import gui_p2_02 as p202

# The frames the app sends above minor 1 on every launch: the sidebar counter's documents (1.6;
# at 1.10 since M5, where they list the external requests that wait)
# and what the Dashboard reads while it is shown (1.7).
OVERVIEW_MINOR = {'Documents': 10, 'Features': 7, 'Attempts': 7, 'Events': 11, 'Executables': 4}

ROOT = Path(__file__).resolve().parents[1]
FAKE = str(ROOT / 'scripts/tests/fake_helper_p2_03.py')
ATTEMPT = '000000000000000000000000000000a2'
CANDIDATE = 'ab' * 32
SUBJECT = {'attempt': ATTEMPT, 'candidate': CANDIDATE}
LOAD = f'wait:ready,candidate:{ATTEMPT}'
PASS = {'state': 'PASS', 'receipt': 31, 'detail': 'validation command exited with code 0'}
RUNNING = {'state': 'UNKNOWN', 'detail': 'validation is running'}
CLAIM = 'All tests passed! 12 passed, 0 failed (says the agent)'


def shown(events):
    return [e for e in events if e.startswith('candidate-state ')]


def sent(received, kind):
    return [r for r in received if r['type'] == kind]


def fake_review_shows_not_run_unknown_and_pass_distinctly_and_accepts_only_a_pass(app, helper):
    s = g.Scenario(app, FAKE,
                   f'{LOAD},wait:validation=NOT RUN,sleep:500,candidate-state,candidate-accept,sleep:200,'
                   'validate,wait:validation=UNKNOWN,sleep:300,candidate-state,candidate-accept,sleep:200,'
                   f'candidate:{ATTEMPT},wait:validation=PASS,wait:scope=PASS,sleep:500,candidate-state,'
                   'candidate-accept,wait:status=Candidate accepted,sleep:500,candidate-state,quit,sleep:8000',
                   {'views': [{'state': 'NOT_RUN'}, RUNNING, PASS], 'log': CLAIM})
    code = s.wait_exit()
    ev, rec = s.events(), s.received()
    states = shown(ev)
    got = [r['type'] for r in rec]
    g.check(code == 0 and len(states) == 4, f'exit {code}, {len(states)} states', ev)
    not_run, running, passed, accepted = states
    g.check('validation=NOT RUN ' in not_run and 'can-accept=false' in not_run, 'NOT RUN is shown as NOT RUN', ev)
    g.check(CLAIM in not_run and 'provider=turn.completed' in not_run,
            'the provider claim is displayed as agent output only', ev)
    g.check('validation=UNKNOWN ' in running and 'validation is running' in running and 'can-accept=false' in running,
            'a running validation is UNKNOWN', ev)
    g.check('validation=PASS scope=PASS decision=NONE can-accept=true files=ALLOWED:src/feature.rs' in passed,
            'PASS is shown only for the recorded receipt', ev)
    g.check('diff-bytes=0 ' not in passed and 'report-bytes=0 ' not in passed, 'diff and report are shown', ev)
    g.check('decision=ACCEPTED can-accept=false' in accepted, 'the decision is shown and final', ev)
    g.check(len({s.split(' ')[1] for s in states[:3]}) == 3, 'three distinct validation labels', ev)
    g.check([r['payload'] for r in sent(rec, 'Validate')] == [SUBJECT], 'Validate names the sealed candidate only', ev)
    decisions = [r['payload'] for r in sent(rec, 'CandidateDecision')]
    g.check(decisions == [{**SUBJECT, 'receipt': 31, 'action': 'Accept'}],
            f'one Accept, bound to digest and receipt, only after PASS: {decisions}', ev)
    g.check(got.index('CandidateDecision') > got.index('Validate'), 'no decision before validation', ev)
    # 1.6: the documents query behind the sidebar counter is read at 1.6 with every snapshot.
    # 1.7: the Dashboard, the screen the app opens on, reads its queries at their own minor.
    g.check(all(r['minor'] in (1, OVERVIEW_MINOR.get(r['type'], 1)) for r in rec if 'minor' in r),
            'protocol 1.1 frames', ev)
    g.check(ev[-1].startswith('will-terminate outcome=clean '), 'shutdown behaviour unchanged', ev)
    return {'gui_exit': code, 'states': states, 'helper_commands': sorted(set(got))}


NEGATIVE = {
    'out_of_scope_write': (
        {'views': [{'state': 'FAIL', 'receipt': 31, 'reasons': ['out-of-scope change "README.md"'],
                    'detail': 'validation command was not run: the change is outside the approved scope'}],
         'changes': {'scope': 'FAIL', 'rows': [{'path': 'README.md', 'kind': 'Added', 'scope': 'OUT_OF_SCOPE'}]}},
        'FAIL', ['validation=FAIL scope=FAIL', 'OUT_OF_SCOPE:README.md', 'out-of-scope change "README.md"']),
    'protected_baseline_test_modified': (
        {'views': [{'state': 'FAIL', 'receipt': 31, 'reasons': ['baseline modified "tests/check.rs"']}],
         'changes': {'scope': 'FAIL', 'rows': [{'path': 'tests/check.rs', 'kind': 'Modified', 'scope': 'BASELINE'}]}},
        'FAIL', ['validation=FAIL scope=FAIL', 'BASELINE:tests/check.rs', 'baseline modified "tests/check.rs"']),
    'failing_validation_command': (
        {'views': [{'state': 'FAIL', 'receipt': 31, 'reasons': ['test "unit::check" Failed'],
                    'detail': 'validation command exited with code 1'}], 'log': CLAIM},
        'FAIL', ['validation=FAIL scope=PASS', 'exited with code 1', 'test "unit::check" Failed', CLAIM]),
    'hung_validator': (
        {'views': [{'state': 'FAIL', 'receipt': 31, 'reasons': ['test "unit::check" Failed'],
                    'detail': 'validation command exceeded its 300 ms budget and was stopped'}]},
        'FAIL', ['validation=FAIL', 'exceeded its 300 ms budget and was stopped']),
    'tampered_sealed_candidate': (
        {'views': [{'state': 'UNKNOWN', 'detail': 'sealed candidate packet rejected: snapshot digest/size mismatch'}],
         'changes': {'error': 'CandidateUnavailable: sealed candidate packet rejected: snapshot digest/size mismatch'}},
        'UNKNOWN', ['validation=UNKNOWN scope=UNKNOWN', 'scope-detail=CandidateUnavailable: sealed candidate packet']),
}


def fake_negative_results_are_visible_and_can_never_be_accepted(app, helper):
    out = {}
    for name, (scenario, label, visible) in NEGATIVE.items():
        s = g.Scenario(app, FAKE, f'{LOAD},wait:validation={label},sleep:600,candidate-state,candidate-accept,sleep:300,'
                                  'candidate-state,quit,sleep:8000', scenario)
        code = s.wait_exit()
        ev, rec = s.events(), s.received()
        states = shown(ev)
        g.check(code == 0 and len(states) == 2, f'{name}: exit {code}', ev)
        for text in visible:
            g.check(text in states[0], f'{name}: {text!r} is visible', ev)
        g.check('can-accept=false' in states[0], f'{name}: Accept is not offered', ev)
        g.check('Accept is unavailable: validation is ' + label in states[1], f'{name}: Accept explains itself', ev)
        g.check(not sent(rec, 'CandidateDecision') and not sent(rec, 'Validate'), f'{name}: nothing was sent', ev)
        out[name] = states[0]
    # A stale approval is the helper's refusal of a real Accept; it is shown, not retried.
    s = g.Scenario(app, FAKE, f'{LOAD},wait:validation=PASS,wait:scope=PASS,candidate-accept,wait:status=Accept refused,'
                              'sleep:300,candidate-state,candidate-reject,wait:status=Candidate rejected,sleep:400,'
                              'candidate-state,quit,sleep:8000',
                   {'views': [PASS], 'refuse_accept': 'validation is stale against current approvals'})
    code = s.wait_exit()
    ev, rec = s.events(), s.received()
    states = shown(ev)
    decisions = [r['payload'] for r in sent(rec, 'CandidateDecision')]
    g.check(code == 0, f'stale approval: exit {code}', ev)
    g.check('Accept refused (validation is stale against current approvals)' in states[0] and 'decision=NONE' in states[0],
            'stale approval refusal is visible and nothing is accepted', ev)
    g.check(decisions == [{**SUBJECT, 'receipt': 31, 'action': 'Accept'}, {**SUBJECT, 'receipt': None, 'action': 'Reject'}],
            f'one refused Accept, then an explicit Reject: {decisions}', ev)
    g.check('decision=REJECTED can-accept=false' in states[1], 'rejection is shown', ev)
    out['stale_approval'] = states[0]
    return {'cases': out}


def fake_lost_validate_response_is_reconciled_not_resent(app, helper):
    s = g.Scenario(app, FAKE, f'{LOAD},wait:validation=NOT RUN,validate,sleep:400,validate,validate,candidate-reject,'
                              'candidate-state,quit,wait:failed,state,ack-unsettled,quit,sleep:8000',
                   {'views': [{'state': 'NOT_RUN'}], 'answer_validate': False})
    code = s.wait_exit()
    ev, rec = s.events(), s.received()
    got = [r['type'] for r in rec]
    validate = sent(rec, 'Validate')
    g.check(code == 0, f'exit {code}', ev)
    g.check(len(validate) == 1, f'an unanswered Validate is never sent again: {got}', ev)
    g.check(not sent(rec, 'CandidateDecision'), 'no other command while the outcome is unknown', ev)
    state = shown(ev)[0]
    g.check('validation=NOT RUN ' in state and 'pending=true' in state, 'unknown outcome is not shown as a result', ev)
    lookup = sent(rec, 'Receipt')[0]
    g.check(lookup['payload'] == {'command': validate[0]['id']}, 'receipt lookup under the original identity', ev)
    g.check(got.index('Receipt') < got.index('PrepareClose'), 'reconcile before closing', ev)
    g.check(ev[-1].startswith('will-terminate outcome=clean-with-unconfirmed-commands'), 'no full preservation claim', ev)
    return {'gui_exit': code, 'helper_commands': got}


def fake_quit_while_validation_is_active_closes_through_the_helper_with_no_result(app, helper):
    s = g.Scenario(app, FAKE, f'{LOAD},wait:validation=NOT RUN,validate,wait:validation=UNKNOWN,sleep:300,candidate-state,'
                              'quit,sleep:8000', {'views': [{'state': 'NOT_RUN'}, RUNNING, RUNNING]})
    code = s.wait_exit()
    ev, rec = s.events(), s.received()
    got = [r['type'] for r in rec]
    state = shown(ev)[0]
    g.check(code == 0, f'exit {code}', ev)
    g.check('validation=UNKNOWN ' in state and 'can-accept=false' in state, 'active validation is UNKNOWN at quit', ev)
    g.check(got.index('PrepareClose') > got.index('Validate'), 'the close goes through the helper', ev)
    g.check(not sent(rec, 'CandidateDecision'), 'quitting decides nothing', ev)
    g.check(ev[-1].startswith('will-terminate outcome=clean '), 'quit only after the helper shutdown receipt', ev)
    return {'gui_exit': code, 'state': state}


def real_helper_reports_an_unsealed_attempt_as_not_run_and_sends_nothing(app, helper):
    s = p202.Slice(app, helper, f'{LOAD},wait:validation=NOT RUN,sleep:300,candidate-state,validate,'
                                'wait:status=Nothing to validate,candidate-accept,candidate-reject,sleep:300,state,quit,sleep:8000')
    code = s.wait_exit()
    ev = s.events()
    state = shown(ev)[0]
    g.check(code == 0, f'exit {code}', ev)
    g.check(any(e.startswith('client send Candidate ') for e in ev), 'the real helper answered the Candidate query', ev)
    g.check('validation=NOT RUN ' in state and 'can-accept=false' in state and 'decision=UNKNOWN' in state,
            'no sealed result: NOT RUN, nothing to accept', ev)
    g.check(not any(e.startswith(('client send Validate ', 'client send CandidateDecision ')) for e in ev),
            'no command was sent', ev)
    g.check(ev[-1].startswith('will-terminate outcome=clean '), 'clean quit', ev)
    return {'gui_exit': code, 'state': state}


SCENARIOS = [fake_review_shows_not_run_unknown_and_pass_distinctly_and_accepts_only_a_pass,
             fake_negative_results_are_visible_and_can_never_be_accepted,
             fake_lost_validate_response_is_reconciled_not_resent,
             fake_quit_while_validation_is_active_closes_through_the_helper_with_no_result,
             real_helper_reports_an_unsealed_attempt_as_not_run_and_sends_nothing]

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
