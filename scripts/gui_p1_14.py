#!/usr/bin/env python3
"""P1-14 runtime macOS GUI scenarios: the real SwiftUI/AppKit app on WindowServer.

Each scenario launches the built JojoApp executable with --ui-script (steps call
NSApp.terminate, NSWindow.performClose and the Cancel/Confirm handlers) and a JSONL trace.
Idle quit, window policy, GUI crash and helper crash use the real jojo-helper; active-work
scenarios use the test-only protocol fixture because protocol 1.0 has no Start command.
The harness (not the app) SIGKILLs a GUI/helper only to simulate a crash.
"""
import argparse
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FAKE = ROOT / 'scripts/tests/fake_helper_p1_14.py'


class Scenario:
    def __init__(self, app, helper, script, scenario=None):
        self.temp = Path(tempfile.mkdtemp(prefix='jojo-gui-'))
        self.data, self.worktrees, self.trace = self.temp / 'data', self.temp / 'worktrees', self.temp / 'trace.jsonl'
        self.helper = helper
        if scenario is not None:
            self.data.mkdir(mode=0o700)
            (self.data / 'scenario.json').write_text(json.dumps(scenario))
        self.proc = subprocess.Popen([app, '--helper', helper, '--data-root', self.data, '--worktree-root',
                                      self.worktrees, '--trace', self.trace, '--ui-script', script],
                                     stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)

    def events(self):
        try:
            return [json.loads(l)['event'] for l in self.trace.read_text().splitlines() if l.strip()]
        except (FileNotFoundError, json.JSONDecodeError):
            return []

    def wait_event(self, prefix, timeout=30):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if any(e.startswith(prefix) for e in self.events()):
                return
            if self.proc.poll() is not None:
                break
            time.sleep(0.05)
        raise AssertionError(f'event {prefix!r} not observed; trace={self.events()[-8:]}')

    def wait_exit(self, timeout=45):
        try:
            return self.proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait()
            raise AssertionError(f'GUI did not exit; trace tail={self.events()[-10:]}')

    def helper_pid(self):
        return next(int(e.split()[1]) for e in self.events() if e.startswith('helper-pid '))

    def received(self):
        path = self.data / 'received.jsonl'
        return [json.loads(l) for l in path.read_text().splitlines()] if path.exists() else []

    def probe(self):
        """Second real helper on the same data root: previous-session marker and admission."""
        p = subprocess.run([self.helper, '--engine-probe', '--data-root', self.data, '--worktree-root', self.worktrees],
                           stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=30)
        assert p.returncode == 0, p.stderr
        return p.stderr


def wait_gone(pid, timeout=30):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return
        time.sleep(0.05)
    raise AssertionError(f'helper {pid} still alive after GUI loss')


def check(cond, message, events):
    if not cond:
        raise AssertionError(f'{message}; trace={events}')


def real_idle_duplicate_quit(app, helper):
    # A real idle close finishes within one script step; duplicates during a pending
    # deferral are exercised in the active-dialog scenario, where the window is long.
    s = Scenario(app, helper, 'wait:ready,state,quit,quit,sleep:8000,state')
    code = s.wait_exit()
    ev = s.events()
    decisions = [e for e in ev if e.startswith('terminate-decision')]
    check(code == 0, f'exit {code}', ev)
    check('cmd-q handled=true' in ev, 'real Cmd-Q main-menu key equivalent', ev)
    check(decisions == ['terminate-decision quitCommand later'], 'one deferred AppKit decision', ev)
    check(sum(e.startswith('client send PrepareClose') for e in ev) == 1, 'one PrepareClose', ev)
    check(not any('confirmRequired' in e for e in ev), 'idle close shows no active-work confirmation', ev)
    check(ev[-1].startswith('will-terminate outcome=clean '), 'clean only after READY + exit', ev)
    check(ev.index(next(e for e in ev if e.startswith('quit phase awaitingExit'))) <
          ev.index(next(e for e in ev if e.startswith('connection ended(status: 0)'))), 'READY precedes exit', ev)
    probe = s.probe()
    check('previous=clean' in probe and 'admission=closed' in probe, probe, ev)
    return {'gui_exit': code, 'terminate_decisions': decisions, 'probe': probe.strip().splitlines()[0]}


def real_apple_event_quit(app, helper):
    s = Scenario(app, helper, 'wait:ready,quit-ae,sleep:8000')
    code = s.wait_exit()
    ev = s.events()
    check(code == 0 and 'quit-ae sent=true' in ev and 'quit-apple-event' in ev, f'exit {code}', ev)
    check([e for e in ev if e.startswith('terminate-decision')] == ['terminate-decision quitCommand later'], 'deferred', ev)
    check(ev[-1].startswith('will-terminate outcome=clean '), 'clean', ev)
    check('previous=clean' in s.probe(), 'clean marker', ev)
    return {'gui_exit': code}


def real_window_policy(app, helper):
    s = Scenario(app, helper, 'wait:ready,open-aux,sleep:300,close-aux,sleep:500,state,minimize-primary,sleep:500,state,'
                              'close-primary,sleep:8000,state')
    code = s.wait_exit()
    ev = s.events()
    aux = ev.index('window-close auxiliary')
    first_decision = next(i for i, e in enumerate(ev) if e.startswith('terminate-decision'))
    check(code == 0, f'exit {code}', ev)
    check(aux < first_decision, 'auxiliary close does not quit', ev)
    states = [e for e in ev if e.startswith('state ')]
    check(states[0].startswith('state phase=idle') and 'aux=false' in states[0], 'app alive after auxiliary close', ev)
    check(states[1].startswith('state phase=idle'), 'minimize is not closure', ev)
    check(ev[first_decision] == 'terminate-decision lastPrimaryWindow later', 'last primary window routes to quit', ev)
    check(ev[-1].startswith('will-terminate outcome=clean '), 'clean quit', ev)
    check('previous=clean' in s.probe(), 'clean marker', ev)
    return {'gui_exit': code, 'states': states, 'decision': ev[first_decision]}


def real_gui_crash_emergency(app, helper):
    s = Scenario(app, helper, 'wait:ready,state')
    s.wait_event('script-done')
    pid = s.helper_pid()
    s.proc.send_signal(signal.SIGKILL)  # simulated GUI crash; helper sees stdin EOF
    s.proc.wait()
    wait_gone(pid)
    probe = s.probe()
    ev = s.events()
    check(not any(e.startswith('will-terminate') for e in ev), 'crash, not a quit', ev)
    check('previous=unclean' in probe and 'admission=closed' in probe, f'emergency leaves unclean session: {probe}', ev)
    return {'helper_exited_after_gui_loss': True, 'probe': probe.strip().splitlines()[0]}


def real_helper_crash_unknown(app, helper):
    s = Scenario(app, helper, 'wait:ready,wait:ended,state,quit')
    s.wait_event('script wait:ready ok')
    os.kill(s.helper_pid(), signal.SIGKILL)  # simulated helper crash
    code = s.wait_exit()
    ev = s.events()
    state = next(e for e in ev if e.startswith('state '))
    check('ended(status: -9)' in state, 'GUI shows helper ended/unknown', ev)
    check(ev[-1].startswith('will-terminate outcome=unverified-helper-gone'), 'never claimed clean', ev)
    check('previous=unclean' in s.probe(), 'unclean marker', ev)
    return {'gui_exit': code, 'state': state, 'outcome': ev[-1]}


def fake_active_cancel_then_confirm(app, helper):
    s = Scenario(app, str(FAKE), 'wait:ready,quit,wait:confirmRequired,quit,quit-ae,close-primary,state,cancel,wait:idle,sleep:1500,state,'
                                 'quit,wait:confirmRequired,confirm,confirm,wait:stopping,sleep:8000,state',
                 {'active': 2, 'progress': [2, 1, 0], 'outcome': 'ready'})
    code = s.wait_exit()
    ev, got = s.events(), [r['type'] for r in s.received()]
    states = [e for e in ev if e.startswith('state ')]
    check(code == 0, f'exit {code}', ev)
    check(got.count('PrepareClose') == 2 and got.count('CancelClose') == 1 and got.count('ConfirmClose') == 1, f'commands {got}', ev)
    check(got.index('CancelClose') < got.index('ConfirmClose'), 'cancel before confirm', ev)
    check(states[0].startswith('state phase=confirmRequired') and 'duplicates=3' in states[0], 'no stacked dialog', ev)
    check(sum(e.startswith('joined-quit') for e in ev) == 3, 'Cmd-Q, quit event and window close join the dialog', ev)
    check(not any(e.startswith('will-terminate') for e in ev[:ev.index(states[1])]), 'duplicates never terminate', ev)
    check(states[1].startswith('state phase=idle') and 'primary-visible=true' in states[1], 'Cancel keeps Jojo open', ev)
    check(any(e.startswith('quit phase stopping stopping(ticket: "000000000000000000000000000000a1", activeCount: Optional(1))') for e in ev), 'progress shown', ev)
    check(ev[-1].startswith('will-terminate outcome=clean '), 'clean after ready + exit', ev)
    return {'gui_exit': code, 'helper_commands': got, 'states': states}


def fake_failure_stays_visible(app, helper):
    s = Scenario(app, str(FAKE), 'wait:ready,quit,wait:confirmRequired,confirm,wait:failed,sleep:300,quit,sleep:500,'
                                 'close-primary,sleep:500,state,disconnect,sleep:8000',
                 {'active': 1, 'progress': [1], 'outcome': 'failure'})
    code = s.wait_exit()
    ev, got = s.events(), [r['type'] for r in s.received()]
    state = next(e for e in ev if e.startswith('state '))
    decisions = [e for e in ev if e.startswith('terminate-decision')]
    check(code == 0, f'exit {code}', ev)
    check(decisions[1:3] == ['terminate-decision quitCommand cancel', 'terminate-decision lastPrimaryWindow cancel'],
          'failure refuses quit and window close', ev)
    check(state.startswith('state phase=failed') and 'primary-visible=true' in state, 'failure stays visible', ev)
    check(ev[-1].startswith('will-terminate outcome=emergency-disconnect-unclean'), 'explicit unclean disconnect', ev)
    check(got[-1] == 'EOF', 'helper observed EOF (emergency path)', ev)
    return {'gui_exit': code, 'decisions': decisions, 'helper_commands': got}


def fake_unsettled_command(app, helper):
    attempt = '000000000000000000000000000000a2'
    s = Scenario(app, str(FAKE), f'wait:ready,cancel-attempt:{attempt},quit,wait:failed,state,ack-unsettled,quit,sleep:8000',
                 {'active': 0, 'answer_commands': False})
    code = s.wait_exit()
    ev, rec = s.events(), s.received()
    got = [r['type'] for r in rec]
    cancel_id = next(r['id'] for r in rec if r['type'] == 'CancelAttempt')
    lookup = next(r for r in rec if r['type'] == 'Receipt')
    check(code == 0, f'exit {code}', ev)
    check(lookup['payload'] == {'command': cancel_id}, 'receipt lookup under the original identity', ev)
    check(got.count('CancelAttempt') == 1, 'never resent', ev)
    check(got.index('Receipt') < got.index('PrepareClose'), 'reconcile before PrepareClose', ev)
    check(next(e for e in ev if e.startswith('state ')).startswith('state phase=failed'), 'draft-save failure visible', ev)
    check(ev[-1].startswith('will-terminate outcome=clean-with-unconfirmed-commands'), 'no full preservation claim', ev)
    return {'gui_exit': code, 'helper_commands': got}


def fake_recovery_and_review_display(app, helper):
    s = Scenario(app, str(FAKE), 'wait:ready,sleep:800,review:000000000000000000000000000000e5,wait:review-status,state,quit,sleep:8000',
                 {'active': 0})
    code = s.wait_exit()
    ev, got = s.events(), [r['type'] for r in s.received()]
    state = next(e for e in ev if e.startswith('state '))
    check('recovery=1' in state and 'No reviewable artifact' in state, 'interrupted work and missing review shown', ev)
    check(not set(got) & {'CancelAttempt', 'HumanDecision', 'ConfirmClose'}, 'no automatic execution on reopen', ev)
    return {'gui_exit': code, 'state': state}


SCENARIOS = [real_idle_duplicate_quit, real_apple_event_quit, real_window_policy, real_gui_crash_emergency, real_helper_crash_unknown,
             fake_active_cancel_then_confirm, fake_failure_stays_visible, fake_unsettled_command,
             fake_recovery_and_review_display]

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
