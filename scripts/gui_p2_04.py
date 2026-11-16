#!/usr/bin/env python3
"""P2-04 offline dress rehearsal: the whole slice through the real helper and the real app.

Every scenario gets its own new directory under --root (disposable root with a freshly
generated sample repository, data root, worktree root, a private copy of the fake Codex
executable and an empty Codex home) and is never cleaned up: a failed or interrupted
scenario stays on disk as evidence, and no data root is reused by another scenario.

Only production interfaces are used:

* registration and bundle import are protocol frames to a real `jojo-helper --serve`
  (`p2_04_sample.Helper`); the app has no control for them;
* everything else is the built JojoApp on WindowServer with `--ui-script`, whose steps call
  the entry points of the Review, Start, Recovery and Candidate tabs: review and approve
  each exact revision, reviewed start, Start, candidate inspection, Validate, Accept/Reject,
  recovery choices, Cmd-Q and its confirmation;
* a second protocol session afterwards reads the helper's own answers back and compares
  them with what the app displayed.

The database is never opened. "Codex" is `jojo-codex-fixture`: no provider, credential or
network. The harness (never the app or helper) sends SIGKILL to an exact PID it observed,
only to simulate a crash of the app, the helper or the agent. After every scenario no
process started from the scenario's fake Codex executable may remain.
"""
import argparse
import hashlib
import json
import os
import secrets
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

import gui_p1_14 as g
import p2_04_sample as sample

CODEX_VERSION = 'codex-cli 0.169.0'
M = sample.manifest()
SUBJECTS = sample.start_subjects()
START = 'start:' + '/'.join(SUBJECTS[k] for k in ['prd', 'adr', 'plan', 'validation', 'step'])
QUIT = 'quit,sleep:8000'
INSPECT = 'recovery-inspect,wait:status=Inspection refreshed'
CLAIM = 'All tests passed (says the agent)'
PS = '/bin/ps'


OPEN = []  # every app session started, so that a failed scenario leaves no app behind


def approve(subject):
    return (f'review:{subject},wait:status=Review loaded at revision 1,review-state,decide:Approve,'
            'wait:status=Approve committed')


class Session(g.Scenario):
    """One run of the app on a rehearsal root; the app starts the real helper itself."""

    def __init__(self, rehearsal, script):
        r = rehearsal
        self.data, self.worktrees, self.helper = r.data, r.worktrees, r.helper
        self.trace = r.base / 'traces' / f'{len(r.sessions) + 1:02d}.jsonl'
        self.proc = subprocess.Popen([r.app, '--helper', r.helper, '--data-root', r.data, '--worktree-root', r.worktrees,
                                      '--disposable-root', r.disposable, *r.codex, '--trace', self.trace,
                                      '--ui-script', script], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)

    def lines(self, prefix):
        return [e for e in self.events() if e.startswith(prefix)]

    def last(self, prefix):
        return self.lines(prefix)[-1]

    def finish(self, clean=True, timeout=90):
        """The app exits on its own; a script that stopped early is a failure, not a pass."""
        code = self.wait_exit(timeout)
        ev = self.events()
        g.check(code == 0, f'app exit {code}', ev)
        g.check(not any(e.startswith(('script-error', 'script-timeout')) for e in ev), 'the script stopped early', ev)
        if clean:
            g.check(ev[-1].startswith('will-terminate outcome=clean '), 'clean quit after the helper shutdown receipt', ev)
        g.wait_gone(self.helper_pid())
        return ev


class Rehearsal:
    def __init__(self, tools, name, fixture):
        self.app, self.helper = tools['app'], tools['helper']
        self.base = Path(tools['root']) / name
        self.base.mkdir(parents=True)  # FileExistsError: a rehearsal root is never reused
        self.disposable, self.data, self.worktrees = self.base / 'disposable', self.base / 'data', self.base / 'worktrees'
        self.disposable.mkdir(mode=0o700)
        for d in ['traces', 'logs', 'bin']:
            (self.base / d).mkdir()
        self.fake = self.base / 'bin/codex'
        shutil.copyfile(tools['fixture'], self.fake)
        self.fake.chmod(0o755)
        self.home = self.base / 'codex-home'
        self.home.mkdir(mode=0o700)
        # Stands for sign-in state. Jojo must neither read nor relay it: it may appear nowhere else.
        self.canary = secrets.token_hex(24)
        (self.home / 'SIGN-IN-CANARY').write_text(self.canary + '\n')
        self.codex = ['--codex-executable', self.fake, '--codex-version', CODEX_VERSION, '--codex-sha256',
                      hashlib.sha256(self.fake.read_bytes()).hexdigest(), '--codex-home', self.home,
                      '--codex-auth', 'native-subscription']
        self.fixture = fixture
        self.repository = sample.generate_repository(self.disposable)
        self.before = sample.repository_state(self.repository['repository'])
        self.sessions, self.stages, self.attempt = [], [], None

    def protocol(self):
        return sample.Helper(self.helper, self.data, self.worktrees, self.disposable, self.codex,
                             stderr=(self.base / 'logs/helper-protocol.err').open('a'))

    def import_stage(self):
        helper = self.protocol()
        stage = sample.import_once(helper, self.repository['repository'], self.repository['commit'], self.fixture)['stage']
        code = helper.close()
        if code != 0:
            raise AssertionError(f'helper exit {code} after import')
        self.stages.append(stage)
        return stage

    def run(self, script):
        session = Session(self, script)
        self.sessions.append(session)
        OPEN.append(session)
        return session

    def prepare(self):
        """Import, then review and approve in the app, stage by stage. Returns the script
        that approves the last two documents, opens admission and sends the one Start."""
        for subject in [M['prd_id'], M['adr_id']]:
            stage = self.import_stage()
            if stage.split('awaiting=')[1] != subject:
                raise AssertionError(f'unexpected import stage {stage}')
            s = self.run(f'wait:ready,{START},wait:status=Start refused,{approve(subject)},{QUIT}')
            s.finish()
        stage = self.import_stage()
        if stage != f"stage=PLAN awaiting={M['validation_id']},{M['plan_id']}":
            raise AssertionError(f'unexpected import stage {stage}')
        return (f"wait:ready,{approve(M['validation_id'])},{approve(M['plan_id'])},{INSPECT},recovery-state,"
                f'recovery-start,wait:admission=Open,{START},wait:status=Start accepted for attempt,remember-attempt,'
                'candidate:@')

    def started(self, session):
        session.wait_event('attempt ')
        self.attempt = session.last('attempt ').split()[1]
        return self.attempt

    def agents(self):
        """Processes started from this scenario's fake Codex executable: (pid, ppid, pgid)."""
        out = subprocess.run([PS, '-axo', 'pid=,ppid=,pgid=,command='], capture_output=True, text=True, timeout=30).stdout
        rows = [line.split(None, 3) for line in out.splitlines()]
        return [tuple(map(int, r[:3])) for r in rows if len(r) == 4 and r[3].startswith(f'{self.fake} ')]

    def guards(self, parent=None):
        """Guard processes of this rehearsal's helper executable: (pid, ppid, pgid, leader)."""
        out = subprocess.run([PS, '-axo', 'pid=,ppid=,pgid=,command='], capture_output=True, text=True, timeout=30).stdout
        rows = [line.split(None, 3) for line in out.splitlines()]
        # The helper names itself as the OS reports it, which may spell the same file differently.
        found = [(*map(int, r[:3]), int(r[3].split()[2])) for r in rows
                 if len(r) == 4 and ' --agent-guard ' in r[3] and r[3].split()[1] == '--agent-guard'
                 and os.path.realpath(r[3].split()[0]) == os.path.realpath(self.helper)]
        return [g for g in found if parent is None or g[1] == parent]

    def group(self, pgid):
        """Live members of one process group: [(pid, command)]."""
        out = subprocess.run([PS, '-axo', 'pid=,pgid=,stat=,command='], capture_output=True, text=True, timeout=30).stdout
        rows = [line.split(None, 3) for line in out.splitlines()]
        return [(int(r[0]), r[3][:80]) for r in rows if len(r) == 4 and int(r[1]) == pgid and not r[2].startswith('Z')]

    def wait_agent(self, timeout=30):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            found = self.agents()
            if found:
                if len(found) != 1:
                    raise AssertionError(f'more than one agent process: {found}')
                return found[0]
            time.sleep(0.05)
        raise AssertionError('the agent process was never observed')

    def wait_no_agents(self, timeout):
        deadline = time.monotonic() + timeout
        while self.agents():
            if time.monotonic() > deadline:
                raise AssertionError(f'agent processes remain: {self.agents()}')
            time.sleep(0.1)

    def audit(self):
        """Independent read-back through the protocol, plus what the files on disk show."""
        helper = self.protocol()
        out = {'import_stage_after_approvals': sample.import_once(
            helper, self.repository['repository'], self.repository['commit'], self.fixture)['stage']}
        rows = {}
        for section in ['aggregates', 'effects']:
            after, cursor = '', None
            while True:
                page = helper.ok('Snapshot', {'section': section, 'after': after, 'cursor': cursor, 'limit': 8})
                rows.update({r['key']: r['value'] for r in page['rows']})
                if not page.get('next') or len(page['rows']) < 8:
                    break
                after, cursor = page['next'], page['as_of_event_seq']
        out['launch_effects'] = {k: v for k, v in rows.items() if k.startswith('scheduler-launch:')}
        out['attempts'] = sorted(k.split(':')[1] for k in rows if k.startswith('scheduler_attempt:'))
        out['recovery_subjects'] = helper.ok('RecoveryInspect', {})['subjects']
        if self.attempt:
            view = helper.ok('Candidate', {'attempt': self.attempt})
            out['candidate'] = view
            if view.get('candidate'):
                changes = helper.frame('CandidateChanges', {'attempt': self.attempt, 'candidate': view['candidate'],
                                                            'offset': 0, 'limit': 8})['payload']
                out['changes'] = {'scope': changes.get('scope'), 'rows': [
                    f"{r['scope']}:{r['path']}" for r in changes.get('rows', [])]}
        out['helper_exit'] = helper.close()
        after = sample.repository_state(self.repository['repository'])
        # The owned worktree is a linked worktree on a Jojo branch at the base commit. That
        # one ref is Jojo's own; everything the human owns must be exactly as it was.
        owned = [ref for ref in after['refs'].splitlines() if ref.startswith('refs/heads/jojo/')]
        after['refs'] = '\n'.join(ref for ref in after['refs'].splitlines() if ref not in owned)
        out['source_repository'] = {'before': self.before, 'after': after, 'jojo_worktree_refs': owned}
        out['validation_directories'] = sorted(p.name for p in (self.data / 'validation').iterdir()) \
            if (self.data / 'validation').is_dir() else []
        leaked = []
        for directory in [self.data, self.worktrees, self.base / 'traces', self.base / 'logs', self.disposable]:
            for p in directory.rglob('*') if directory.exists() else []:
                if p.is_file() and not p.is_symlink() and self.canary.encode() in p.read_bytes():
                    leaked.append(str(p))
        out['sign_in_canary'] = {'copies_outside_codex_home': leaked,
                                 'codex_home_entries': sorted(p.name for p in self.home.iterdir())}
        out['agent_processes_remaining'] = self.agents()
        out['guard_processes_remaining'] = self.guards()
        failures = []
        if after != self.before or len(owned) > 1 or any(not ref.endswith(' ' + self.before['head']) for ref in owned):
            failures.append(f'the source repository changed: {after} {owned}')
        if (Path(self.repository['repository']) / 'slugify.py').read_bytes() != (sample.SAMPLE / 'slugify.py').read_bytes():
            failures.append('slugify.py in the source repository was written')
        if leaked or out['sign_in_canary']['codex_home_entries'] != ['SIGN-IN-CANARY']:
            failures.append(f'Codex home content was copied or changed: {leaked}')
        if out['agent_processes_remaining'] or out['guard_processes_remaining']:
            failures.append(f"orphaned agent or guard process: {out['agent_processes_remaining']} "
                            f"{out['guard_processes_remaining']}")
        if len(out['launch_effects']) > 1 or len(out['attempts']) > 1:
            failures.append(f"more than one launch: {out['launch_effects']} {out['attempts']}")
        if out['import_stage_after_approvals'] != 'stage=COMPLETE awaiting=':
            failures.append(f"approved documents are not the bundle: {out['import_stage_after_approvals']}")
        if out['helper_exit'] != 0:
            failures.append(f"audit helper exit {out['helper_exit']}")
        if failures:
            raise AssertionError('; '.join(failures))
        return out

    def result(self, audit, **observed):
        return {'root': str(self.base), 'fixture': self.fixture, 'attempt': self.attempt,
                'fake_codex': {'path': str(self.fake), 'sha256': hashlib.sha256(self.fake.read_bytes()).hexdigest()},
                'import_stages': self.stages, 'app_sessions': [str(s.trace) for s in self.sessions],
                'reviews_approved': [e for s in self.sessions for e in s.lines('review-state ')],
                'starts_sent': sum(len(s.lines('client send Start ')) for s in self.sessions),
                'starts_accepted': sum(len(s.lines('attempt ')) for s in self.sessions),
                'audit': audit, **observed}


def shown(session, kind='candidate-state '):
    return session.lines(kind)


def reopen_refuses_start(r, tail=''):
    """A new app session on the same data root: what recovery shows, and that Start stays refused."""
    s = r.run(f'wait:ready,{INSPECT},recovery-state,{START},wait:status=Start refused (AdmissionClosed,recovery-start,'
              f'wait:admission=Open,{START},wait:status=Start refused (,state,candidate:{r.attempt},wait:sealed,'
              f'candidate-result,candidate-state,{tail}{QUIT}')
    return s, s.finish()


def decide_failed(r, s, ev, outcome, reason):
    """Common end of a run that did not succeed: visible, not acceptable, rejected by the human."""
    result, states = s.last('candidate-result '), shown(s)
    g.check(f'outcome={outcome} reason={reason} ' in result, f'the result is shown as {outcome}', ev)
    g.check('validation=NOT RUN ' in states[0] and 'can-accept=false' in states[0], 'nothing was validated or acceptable', ev)
    g.check('decision=REJECTED' in states[-1], 'the human rejection is recorded', ev)
    g.check(not any('candidate accepted' in e for e in ev), 'nothing was accepted', ev)


def successful_development_is_validated_and_accepted(tools):
    r = Rehearsal(tools, 'success', 'implement-slugify')
    s = r.run(f'{r.prepare()},wait:sealed,wait:outcome=Succeeded,wait:files=1,wait:diff,wait:log={CLAIM},candidate-result,'
              'candidate-state,candidate-text,candidate-accept,wait:status=Accept is unavailable,validate,'
              'wait:validation=PASS,wait:report,candidate-result,candidate-state,candidate-text,candidate-accept,'
              f'wait:decision=ACCEPTED,candidate-state,state,{QUIT}')
    r.started(s)
    ev = s.finish()
    states, diff, report = shown(s), s.last('candidate-diff '), s.last('candidate-report ')
    g.check('validation=NOT RUN ' in states[0] and 'can-accept=false' in states[0] and CLAIM in states[0],
            'the agent saying "tests passed" is shown as its statement and validates nothing', ev)
    g.check('scope=PASS ' in states[0] and 'files=ALLOWED:slugify.py ' in states[0], 'only slugify.py changed', ev)
    g.check('+    return re.sub(' in diff and 'test_slugify.py' not in diff, 'the repository-derived diff is shown', ev)
    g.check('validation=PASS ' in states[1] and 'can-accept=true' in states[1], 'host validation passed', ev)
    g.check('network: denied by OS sandbox' in report and 'Ran 8 tests' in report
            and 'command: ["/usr/bin/python3", "-m", "unittest", "-q"]' in report, 'the validator report is shown', ev)
    g.check('decision=ACCEPTED' in states[2], 'human acceptance recorded', ev)
    g.check(len(s.lines('client send Start ')) == 1 and len(s.lines('client send Validate ')) == 1, 'one Start, one Validate', ev)
    again, ev2 = reopen_refuses_start(r)
    state = again.last('state ')
    g.check('subjects=0 ' in again.last('recovery-state '), 'a successful run leaves nothing to recover', ev2)
    g.check('DuplicateStart' in state, 'the step cannot be started a second time', ev2)
    g.check('decision=ACCEPTED' in shown(again)[-1], 'the acceptance survives a restart', ev2)
    audit = r.audit()
    view = audit['candidate']
    g.check(view['outcome'] == 'Succeeded' and view['decision'] == 'ACCEPTED' and view['validation']['state'] == 'PASS'
            and f"candidate={view['candidate']} " in s.last('candidate-result '), 'the helper agrees with the app', ev)
    g.check(audit['changes'] == {'scope': 'PASS', 'rows': ['ALLOWED:slugify.py']}, 'changed paths', ev)
    g.check(len(audit['validation_directories']) == 1 and len(audit['launch_effects']) == 1, 'one launch, one validation', ev)
    return r.result(audit, displayed=states, restart=state)


def _scope_violation(tools, name, fixture, files, reason):
    r = Rehearsal(tools, name, fixture)
    s = r.run(f'{r.prepare()},wait:sealed,wait:files=2,wait:diff,candidate-result,candidate-state,candidate-text,validate,'
              'wait:validation=FAIL,wait:report,candidate-state,candidate-text,candidate-accept,'
              f'wait:status=Accept is unavailable,candidate-reject,wait:decision=REJECTED,candidate-state,{QUIT}')
    r.started(s)
    ev = s.finish()
    states, report = shown(s), s.last('candidate-report ')
    g.check('scope=FAIL ' in states[0] and f'files={files} ' in states[0], 'the violation is visible before validation', ev)
    g.check('validation=FAIL ' in states[1] and 'can-accept=false' in states[1] and reason in states[1],
            'validation fails with the reason', ev)
    g.check('validation command was not run' in report and 'Ran ' not in report, 'the command was never run', ev)
    g.check('decision=REJECTED' in states[2] and not any('candidate accepted' in e for e in ev), 'rejected, never accepted', ev)
    audit = r.audit()
    g.check(audit['candidate']['decision'] == 'REJECTED' and audit['candidate']['validation']['state'] == 'FAIL'
            and audit['changes']['scope'] == 'FAIL', 'the helper agrees with the app', ev)
    return r.result(audit, displayed=states)


def out_of_scope_write_fails_validation_and_is_rejected(tools):
    return _scope_violation(tools, 'out-of-scope-write', 'implement-slugify-stray',
                            'OUT_OF_SCOPE:NOTES.md;ALLOWED:slugify.py', 'out-of-scope change "NOTES.md"')


def edited_baseline_test_fails_validation_and_is_rejected(tools):
    return _scope_violation(tools, 'baseline-test-edit', 'implement-slugify-edit-tests',
                            'ALLOWED:slugify.py;BASELINE:test_slugify.py', 'baseline modified "test_slugify.py"')


def _failed_run(tools, name, fixture, reason, log):
    r = Rehearsal(tools, name, fixture)
    s = r.run(f'{r.prepare()},wait:sealed,wait:outcome=Failed,wait:log={log},candidate-result,candidate-state,'
              'candidate-accept,wait:status=Accept is unavailable,candidate-reject,wait:decision=REJECTED,candidate-state,'
              f'{QUIT}')
    r.started(s)
    ev = s.finish()
    decide_failed(r, s, ev, 'Failed', reason)
    g.check(log in shown(s)[0], 'the provider error is shown in the agent output', ev)
    again, ev2 = reopen_refuses_start(r)
    g.check('subjects=1 ' in again.last('recovery-state ') and 'RecoveryHold' in again.last('state '),
            'the failed run is a recovery subject and Start stays refused on this data root', ev2)
    audit = r.audit()
    g.check(audit['candidate']['outcome'] == 'Failed' and audit['candidate']['decision'] == 'REJECTED', 'helper agrees', ev)
    return r.result(audit, displayed=shown(s), restart=again.last('state '), recovery=again.lines('recovery-row '))


def codex_turn_failure_is_visible_and_never_accepted(tools):
    return _failed_run(tools, 'turn-failure', 'turn-failed', 'provider_error', 'synthetic failure')


def authentication_error_is_visible_and_nothing_is_relayed(tools):
    out = _failed_run(tools, 'authentication-error', 'auth-error', 'provider_error', '401 Unauthorized')
    # No sign-in was attempted or relayed: one launch, and the Codex home was only located.
    assert out['starts_accepted'] == 1 and len(out['audit']['launch_effects']) == 1, out
    return out


def unknown_outcome_is_never_treated_as_success(tools):
    """Exit status 0 without a terminal record: the provider never said how the turn ended."""
    r = Rehearsal(tools, 'unknown-outcome', 'exit-zero-no-terminal')
    s = r.run(f'{r.prepare()},wait:sealed,candidate-result,candidate-state,candidate-accept,'
              'wait:status=Accept is unavailable,validate,wait:validation=FAIL,wait:report,candidate-state,'
              f'candidate-accept,wait:status=Accept is unavailable,candidate-reject,wait:decision=REJECTED,candidate-state,{QUIT}')
    r.started(s)
    ev = s.finish()
    result, states = s.last('candidate-result '), shown(s)
    g.check('outcome=Failed reason=protocol_incomplete ' in result and 'provider=[] ' in states[0],
            'exit 0 without a terminal record is not a success', ev)
    g.check('validation=FAIL ' in states[1] and 'can-accept=false' in states[1], 'host validation fails on its own evidence', ev)
    g.check('decision=REJECTED' in states[2] and not any('candidate accepted' in e for e in ev), 'never accepted', ev)
    again, ev2 = reopen_refuses_start(r)
    g.check('subjects=1 ' in again.last('recovery-state ') and 'RecoveryHold' in again.last('state '), 'held', ev2)
    audit = r.audit()
    g.check(audit['candidate']['outcome'] == 'Failed' and audit['candidate']['decision'] == 'REJECTED', 'helper agrees', ev)
    return r.result(audit, displayed=states, restart=again.last('state '))


def recover(r, expect_reason):
    """After the agent is proven gone: inspect in the app, choose "validate existing output",
    see the interrupted result, validate it, and find that it can be rejected but never
    accepted and that Start stays refused on this data root."""
    a = r.attempt
    s = r.run(f'wait:ready,{INSPECT},wait:recovery=1,recovery-state,{START},wait:status=Start refused (AdmissionClosed,'
              f'recovery-resolve:{a}/ValidateOutput,wait:status=Recovery choice ValidateOutput committed,sleep:500,'
              f'recovery-state,candidate:{a},wait:sealed,wait:files=1,candidate-result,candidate-state,validate,'
              'wait:validation=PASS,wait:report,candidate-state,candidate-accept,wait:status=Accept refused (,state,'
              'candidate-reject,wait:decision=REJECTED,candidate-state,recovery-start,wait:admission=Open,'
              f'{START},wait:status=Start refused (RecoveryHold,state,{QUIT}')
    ev = s.finish()
    rows, states, result = s.lines('recovery-row '), shown(s), s.last('candidate-result ')
    g.check('choices: [ValidateOutput, ContinueFreshContext, Hold]' in rows[0] and 'blockers: []' in rows[0],
            'with the agent proven gone the helper offers the reviewed choices', ev)
    g.check(f'outcome=Interrupted reason={expect_reason} ' in result, 'the run is shown as interrupted, never as a success', ev)
    g.check('files=ALLOWED:slugify.py ' in states[0] and 'validation=NOT RUN ' in states[0], 'its partial work is inspectable', ev)
    g.check('validation=PASS ' in states[1] and "Accept refused (only a succeeded invocation's candidate" in s.lines('state ')[0],
            'a passing validation of an interrupted run still cannot be accepted', ev)
    g.check('decision=REJECTED' in states[2] and not any('candidate accepted' in e for e in ev), 'rejected by the human', ev)
    g.check('RecoveryHold' in s.last('state '), 'no new attempt can be started on this data root', ev)
    return s, {'recovery': rows, 'displayed': states, 'restart': s.last('state ')}


def agent_killed_during_execution_is_failed_and_nothing_remains(tools):
    r = Rehearsal(tools, 'agent-killed', 'work-then-hang 120')
    s = r.run(f'{r.prepare()},wait:sealed,wait:files=1,candidate-result,candidate-state,candidate-accept,'
              f'wait:status=Accept is unavailable,candidate-reject,wait:decision=REJECTED,candidate-state,{QUIT}')
    r.started(s)
    pid = r.wait_agent()[0]
    time.sleep(1)
    os.kill(pid, signal.SIGKILL)  # simulated crash of the agent itself (exact PID, observed above)
    ev = s.finish()
    g.check('outcome=Succeeded' not in s.last('candidate-result '), 'a killed agent is not a success', ev)
    g.check('validation=NOT RUN ' in shown(s)[0] and 'decision=REJECTED' in shown(s)[-1], 'visible and rejected', ev)
    r.wait_no_agents(0)
    again, ev2 = reopen_refuses_start(r)
    g.check('subjects=1 ' in again.last('recovery-state ') and 'RecoveryHold' in again.last('state '), 'held', ev2)
    audit = r.audit()
    return r.result(audit, displayed=shown(s), result=s.last('candidate-result '), restart=again.last('state '))


def application_quit_during_execution_stops_the_agent_and_is_recoverable(tools):
    r = Rehearsal(tools, 'app-quit', 'work-then-hang 120')
    s = r.run(f'{r.prepare()},sleep:1500,candidate-result,quit,wait:confirmRequired,state,sleep:500,confirm,sleep:60000')
    r.started(s)
    agent = r.wait_agent()
    ev = s.finish()
    g.check('sealed=false' in s.last('candidate-result '), 'the run was active when Quit was chosen', ev)
    g.check(s.last('state ').startswith('state phase=confirmRequired'), 'quitting with an active run asks first', ev)
    g.check(len(s.lines('client send ConfirmClose ')) == 1, 'one confirmed close', ev)
    r.wait_no_agents(0)
    again, shownow = recover(r, 'shutdown')
    audit = r.audit()
    g.check(audit['candidate']['outcome'] == 'Interrupted' and audit['candidate']['decision'] == 'REJECTED', 'helper agrees', ev)
    return r.result(audit, agent=agent, quit=ev[-1], **shownow)


def application_crash_during_execution_stops_the_agent_and_is_recoverable(tools):
    r = Rehearsal(tools, 'app-crash', 'work-then-hang 120')
    s = r.run(f'{r.prepare()},sleep:60000')
    r.started(s)
    agent = r.wait_agent()
    time.sleep(1)
    helper = s.helper_pid()
    s.proc.send_signal(signal.SIGKILL)  # simulated crash of the app; the helper sees end-of-file
    s.proc.wait()
    g.wait_gone(helper)
    r.wait_no_agents(0)
    again, shownow = recover(r, 'shutdown')
    audit = r.audit()
    g.check(audit['candidate']['outcome'] == 'Interrupted' and audit['candidate']['decision'] == 'REJECTED', 'helper agrees',
            s.events())
    return r.result(audit, agent=agent, **shownow)


def guarded(r, session, leader):
    """The one guard of the running group: a child of the app's helper, inside that group."""
    found = r.guards(parent=session.helper_pid())
    g.check(len(found) == 1 and found[0][2] == leader and found[0][3] == leader,
            f'one guard inside the process group it guards: {found}', session.events())
    return found[0]


def helper_killed_during_execution_stops_the_agent_and_is_recoverable(tools):
    r = Rehearsal(tools, 'helper-killed', 'work-then-hang 120')
    s = r.run(f'{r.prepare()},wait:ended,state,candidate-state,{QUIT}')
    r.started(s)
    agent = r.wait_agent()
    guard = guarded(r, s, agent[2])
    time.sleep(1)
    killed = time.monotonic()
    os.kill(s.helper_pid(), signal.SIGKILL)  # simulated crash of the helper
    r.wait_no_agents(10)
    took = round(time.monotonic() - killed, 2)
    ev = s.finish(clean=False)
    g.check('ended(status: -9)' in s.last('state '), 'the app shows that the helper is gone', ev)
    g.check(ev[-1].startswith('will-terminate outcome=unverified-helper-gone'), 'the app never claims a clean stop', ev)
    g.check(not r.group(agent[2]) and not r.guards(), 'the whole group, guard included, is gone', ev)
    # Same data root: the agent is proven gone, so recovery can be resolved at once.
    again, shownow = recover(r, 'helper_lost_recovered')
    audit = r.audit()
    g.check(audit['candidate']['outcome'] == 'Interrupted' and audit['candidate']['decision'] == 'REJECTED', 'helper agrees', ev)
    return r.result(audit, agent=agent, guard=guard, agent_gone_seconds_after_helper_kill=took, **shownow)


def helper_killed_while_the_guard_cannot_act_is_held_until_the_agent_is_gone(tools):
    """The guard is lost at the moment the helper dies, standing for any way it could
    fail. Nothing stops the agent then, and the fail-closed rules of P2-04 must still hold."""
    r = Rehearsal(tools, 'helper-killed-guard-lost', 'work-then-hang 25')
    s = r.run(f'{r.prepare()},wait:ended,state,candidate-state,{QUIT}')
    r.started(s)
    agent = r.wait_agent()
    guard = guarded(r, s, agent[2])
    time.sleep(1)
    # Simulated double failure, on exact PIDs observed above: the helper is frozen so that
    # it cannot replace the guard, the guard is killed, then the helper is killed.
    os.kill(s.helper_pid(), signal.SIGSTOP)
    os.kill(guard[0], signal.SIGKILL)
    os.kill(s.helper_pid(), signal.SIGKILL)
    ev = s.finish(clean=False)
    g.check(ev[-1].startswith('will-terminate outcome=unverified-helper-gone'), 'the app never claims a clean stop', ev)
    orphan = r.agents()
    g.check(orphan == [(agent[0], 1, agent[2])], f'without a working guard the agent outlives the helper: {orphan}', ev)
    a = r.attempt
    held = r.run(f'wait:ready,{INSPECT},wait:recovery=1,wait:choices=[Hold],recovery-state,{START},'
                 f'wait:status=Start refused (AdmissionClosed,recovery-resolve:{a}/ValidateOutput,'
                 f'wait:status=Recovery choice refused,candidate:{a},sleep:800,candidate-result,candidate-state,state,'
                 'quit,sleep:4000,state,disconnect,sleep:8000')
    code = held.wait_exit(90)
    ev2 = held.events()
    g.wait_gone(held.helper_pid())
    row = held.last('recovery-row ')
    g.check(code == 0 and not any(e.startswith(('script-error', 'script-timeout')) for e in ev2), f'held session exit {code}', ev2)
    g.check(held.last('state ').startswith('state phase=failed') and 'neither settled nor proven absent' in ev2[-1]
            and ev2[-1].startswith('will-terminate outcome=emergency-disconnect-unclean'),
            'with a live agent the close fails visibly and is never reported clean', ev2)
    g.check(f'live orphan pid:{agent[0]} ' in row and 'choices: [Hold]' in row, 'the live agent is named and only Hold is offered', ev2)
    g.check('sealed=false' in held.last('candidate-result '), 'nothing was sealed while the agent may be running', ev2)
    still = r.agents()
    g.check(still == orphan, f'Jojo signalled nothing: {still}', ev2)
    r.wait_no_agents(40)  # the fixture ends by itself; nothing else can end it
    again, shownow = recover(r, 'helper_lost_recovered')
    audit = r.audit()
    g.check(audit['candidate']['outcome'] == 'Interrupted' and audit['candidate']['decision'] == 'REJECTED', 'helper agrees', ev)
    return r.result(audit, agent=agent, guard=guard, orphan_after_helper_kill=orphan, orphan_during_held_session=still,
                    held=[row, held.last('state '), ev2[-1]], **shownow)


def helper_killed_during_validation_stops_the_validation_command(tools):
    """Host validation runs candidate code. It is guarded like the agent."""
    r = Rehearsal(tools, 'helper-killed-validating', 'implement-slugify-hang-tests')
    s = r.run(f'{r.prepare()},wait:sealed,wait:outcome=Succeeded,validate,wait:validation=UNKNOWN,candidate-state,'
              f'wait:ended,state,{QUIT}')
    r.started(s)
    s.wait_event('candidate-state ')
    deadline = time.monotonic() + 30
    while not r.guards(parent=s.helper_pid()):
        g.check(time.monotonic() < deadline, 'the validation command was never guarded', s.events())
        time.sleep(0.05)
    guard = r.guards(parent=s.helper_pid())[0]
    time.sleep(1.5)
    running = [m for m in r.group(guard[2]) if m[0] != guard[0]]
    g.check(running and running[0][0] == guard[2], f'the validation command is running: {running}', s.events())
    killed = time.monotonic()
    os.kill(s.helper_pid(), signal.SIGKILL)  # simulated crash of the helper
    while r.group(guard[2]):
        g.check(time.monotonic() - killed < 10, f'validation processes remain: {r.group(guard[2])}', s.events())
        time.sleep(0.05)
    took = round(time.monotonic() - killed, 2)
    ev = s.finish(clean=False)
    g.check('validation=UNKNOWN ' in s.last('candidate-state '), 'no verdict existed', ev)
    again = r.run(f'wait:ready,candidate:{r.attempt},wait:sealed,sleep:800,candidate-result,candidate-state,'
                  f'candidate-accept,wait:status=Accept is unavailable,candidate-reject,wait:decision=REJECTED,'
                  f'candidate-state,{QUIT}')
    ev2 = again.finish()
    states = shown(again)
    g.check('validation=UNKNOWN ' in states[0] and 'can-accept=false' in states[0],
            'an interrupted validation is UNKNOWN, never a pass', ev2)
    g.check('decision=REJECTED' in states[-1], 'rejected', ev2)
    audit = r.audit()
    g.check(audit['candidate']['validation']['state'] == 'UNKNOWN' and audit['candidate']['decision'] == 'REJECTED',
            'helper agrees', ev2)
    return r.result(audit, guard=guard, validation_processes=running, validation_gone_seconds_after_helper_kill=took,
                    displayed=states)


SCENARIOS = [successful_development_is_validated_and_accepted,
             out_of_scope_write_fails_validation_and_is_rejected,
             edited_baseline_test_fails_validation_and_is_rejected,
             codex_turn_failure_is_visible_and_never_accepted,
             authentication_error_is_visible_and_nothing_is_relayed,
             unknown_outcome_is_never_treated_as_success,
             agent_killed_during_execution_is_failed_and_nothing_remains,
             application_quit_during_execution_stops_the_agent_and_is_recoverable,
             application_crash_during_execution_stops_the_agent_and_is_recoverable,
             helper_killed_during_execution_stops_the_agent_and_is_recoverable,
             helper_killed_while_the_guard_cannot_act_is_held_until_the_agent_is_gone,
             helper_killed_during_validation_stops_the_validation_command]

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    for name in ['--app', '--helper', '--fixture', '--root', '--out']:
        parser.add_argument(name, required=True)
    parser.add_argument('--only')
    a = parser.parse_args()
    sample.check_disposable_root(a.root)
    tools = {name: str(Path(getattr(a, name)).resolve()) for name in ['app', 'helper', 'fixture', 'root']}
    results, failed = {}, []
    for scenario in SCENARIOS:
        if a.only and scenario.__name__ != a.only:
            continue
        started = time.monotonic()
        try:
            results[scenario.__name__] = {'passed': True, **scenario(tools)}
            print(f'PASS {scenario.__name__}', flush=True)
        except Exception as error:
            text = str(error)
            results[scenario.__name__] = {'passed': False, 'error': text if len(text) < 8000 else f'{text[:1000]} … {text[-6000:]}'}
            failed.append(scenario.__name__)
            print(f'FAIL {scenario.__name__}: {text[:300]}', flush=True)
        for session in OPEN:
            # A failed scenario may leave its app running. Ending the harness's own child
            # makes the helper stop its work (end-of-file); the evidence root is kept.
            if session.proc.poll() is None:
                session.proc.kill()
                session.proc.wait()
        OPEN.clear()
        results[scenario.__name__]['seconds'] = round(time.monotonic() - started, 1)
    Path(a.out).write_text(json.dumps(results, indent=2) + '\n')
    sys.exit(1 if failed else 0)
