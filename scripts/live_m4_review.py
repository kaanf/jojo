#!/usr/bin/env python3
"""M4 live run: one plan step done by Codex and reviewed by Claude Code.

Two provider runs at most: the step's attempt and its cross-provider review. The lead of
this run is the test fixture (`jojo-codex-fixture`), so that no live lead turn is spent;
the project's repair limit is 0, so that a failure or a rejection holds the step for the
human instead of starting a third provider run (repair itself is proven with the fixture
in the test suite). This script never approves, accepts or runs anything live. It has
three phases, each against the same private directory under --root:

  prepare  generate the sample repository, register it, save the project record (docs
           steps to Codex, repair limit 0, reviewer on, capacity 2) and run fixture
           discovery turns until the requirements draft exists.
  plan     run the fixture plan turn: the ADR draft of a one-step plan.
  collect  read the run, its repairs and review, the plan branch and the developer
           checkout; write result.json.

Between the phases the human opens the app on the same data root (the command is printed),
approves the documents in the Approval Inbox, runs the plan on Execution Plan and accepts
each step on Verification & Evidence. Nothing is pushed.
"""
import argparse
import getpass
import hashlib
import json
import subprocess
import sys
import time
from pathlib import Path

import p2_04_sample as sample

CLAUDE_VERSION = '2.1.311 (Claude Code)'
CODEX_VERSION = 'codex-cli 0.169.0'
FEATURE = '00000000000000000000000000040400'
PROJECT = '00000000000000000000000000040401'
OBJECTIVE = 'slugify should also be usable from the command line.\nfixture: lead'
ANSWER = 'Take the simplest option that keeps the existing tests passing.'


class Helper(sample.Helper):
    """The sample client; the minor follows the request (1.3 for the runner's, 1.5 for repairs)."""

    def frame(self, kind, payload, revision=None, request=None, minor=None):
        runner = ('SaveProject', 'Projects', 'RunPlan', 'ControlPlan', 'PlanRun')
        minor = minor or (5 if kind == 'Repairs' else 3 if kind in runner else 2)
        request = request or f'{sample.secrets.randbits(96) + 1:032x}'
        frame = {'major': 1, 'minor': minor, 'session': self.session, 'epoch': self.epoch, 'id': request,
                 'revision': revision, 'type': kind, 'payload': payload}
        self.proc.stdin.write((json.dumps(frame, separators=(',', ':'), ensure_ascii=False) + '\n').encode())
        self.proc.stdin.flush()
        reply = self.receive(timeout=60)
        if reply['id'] != request:
            raise RuntimeError(f'{kind}: unexpected frame {reply}')
        return reply


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def fixture_options(fixture):
    return ['--codex-executable', fixture, '--codex-version', CODEX_VERSION, '--codex-sha256', digest(fixture),
            '--codex-home', Path.home() / '.codex', '--codex-auth', 'native-subscription']


def turn(helper, repository, kind, message, seconds=60):
    deadline = time.monotonic() + seconds
    while True:
        try:
            sent = helper.ok('ChatTurn', {'feature': FEATURE, 'kind': kind, 'repository': repository,
                                          'provider': 'Codex', 'message': message})
            break
        except RuntimeError as refused:
            # D3: the helper is still learning the executable's digest; nothing was accepted.
            if 'ExecutableChecking' not in str(refused) or time.monotonic() > deadline:
                raise
            time.sleep(0.5)
    while time.monotonic() < deadline:
        chat = helper.ok('FeatureChat', {'feature': FEATURE, 'after': 0})
        row = next(r for r in chat['turns'] if r['turn'] == sent['turn'])
        if row['state'] not in ('Running', 'Stopping'):
            return chat, row
        time.sleep(0.5)
    raise RuntimeError('the fixture turn did not end')


def open_helper(root, a):
    stderr = open(root / 'helper-stderr.txt', 'ab')
    helper = Helper(a.helper, root / 'data', root / 'worktrees', root / 'disposable', fixture_options(a.fixture), stderr)
    inspection = helper.ok('RecoveryInspect', {})
    if inspection['subjects'] == 0:
        helper.ok('RecoveryStart', {'digest': inspection['digest']})
    return helper


def pinned(name, executable, version):
    """D3: an executable given here is pinned; without one the helper discovers it."""
    if not executable:
        return []
    path = Path(executable)
    return [f'--{name}-executable', path, f'--{name}-version', version, f'--{name}-sha256', digest(path)]


def app_command(root, a):
    options = ['--helper', a.helper, '--data-root', root / 'data', '--worktree-root', root / 'worktrees',
               '--disposable-root', root / 'disposable',
               *pinned('claude', a.claude_executable, CLAUDE_VERSION),
               '--claude-home', Path.home(), '--claude-user', getpass.getuser(), '--claude-auth', 'native-subscription',
               *pinned('codex', a.codex_executable, CODEX_VERSION),
               '--codex-home', Path.home() / '.codex', '--codex-auth', 'native-subscription']
    return ' '.join([str(a.app)] + [f"'{o}'" if ' ' in str(o) else str(o) for o in options])


def prepare(root, a):
    (root / 'disposable').mkdir(mode=0o700)
    made = sample.generate_repository(root / 'disposable')
    (root / 'before.json').write_text(json.dumps(sample.repository_state(made['repository']), indent=2))
    helper = open_helper(root, a)
    try:
        repository = helper.ok('RegisterRepository', {'path': made['repository']})['response']
        helper.ok('SaveProject', {
            'project': PROJECT, 'revision': 0, 'name': 'Slugify sample', 'repositories': [repository],
            'instructions': 'Change only the files of your step. Keep slugify.py and its tests as they are.',
            'test_command': ['/usr/bin/python3', '-m', 'unittest'], 'ci_command': [],
            'kinds': [{'kind': 'backend', 'provider': 'ClaudeCode'}, {'kind': 'docs', 'provider': 'Codex'}],
            'default_provider': 'ClaudeCode', 'capacity': 2, 'repair_limit': 0, 'review': True})
        chat, row = turn(helper, repository, 'Discover', OBJECTIVE)
        for question in row['questions']:
            helper.ok('AnswerQuestion', {'question': question['question'], 'revision': question['revision'],
                                         'action': 'Answer', 'answer': ANSWER})
        chat, row = turn(helper, repository, 'Discover', '')
        if chat['prd'] is None:
            raise RuntimeError(f'no requirements draft: {row}')
        (root / 'repository.txt').write_text(repository + '\n' + made['repository'] + '\n')
        print(f"requirements draft {chat['prd']['document']} revision {chat['prd']['revision']}, unapproved")
    finally:
        print('helper exit', helper.close())


def plan(root, a):
    repository = (root / 'repository.txt').read_text().split('\n')[0]
    helper = open_helper(root, a)
    try:
        chat, row = turn(helper, repository, 'Plan', 'fixture: lead-live-review')
        if row['state'] != 'Replied' or row['plan'] is None:
            raise RuntimeError(f'no plan draft: {row}')
        (root / 'plan.json').write_text(json.dumps(row['plan'], indent=2))
        print(f"ADR draft {row['plan']['adr']}; plan {row['plan']['plan']} follows the ADR's approval")
    finally:
        print('helper exit', helper.close())


def git(path, *args):
    return subprocess.run(['/usr/bin/git', '-C', str(path), *args], capture_output=True, text=True, check=True).stdout.strip()


def collect(root, a):
    repository, path = (root / 'repository.txt').read_text().split('\n')[:2]
    planned = json.loads((root / 'plan.json').read_text())
    helper = open_helper(root, a)
    try:
        run = helper.ok('PlanRun', {'plan': planned['plan']})
        repairs = helper.ok('Repairs', {})
        events = helper.ok('Events', {'after': 0, 'limit': 8})
    finally:
        exit_code = helper.close()
    result = {'run': run, 'repairs': repairs, 'helper_exit': exit_code}
    if run:
        branch, base, head = run['branch'], run['base'], run['head']
        result['branch_log'] = git(path, 'log', '--format=%H %an <%ae> %s', f'{base}..{head}')
        result['branch_ref'] = git(path, 'rev-parse', f'refs/heads/{branch}')
        result['branch_files'] = git(path, 'ls-tree', '--name-only', '-r', head)
    before = json.loads((root / 'before.json').read_text())
    result['developer_checkout_unchanged'] = {k: v for k, v in sample.repository_state(path).items()
                                              if k != 'refs'} == {k: v for k, v in before.items() if k != 'refs'}
    result['first_events'] = events
    # Done when the step was reviewed by the other provider and nothing else ran: either it
    # is on the plan branch, or it waits for the human after a rejection, with no repair.
    step = run['steps'][0] if run else {}
    review = step.get('review') or {}
    result['reviewed_by_other_provider'] = review.get('provider') not in (None, step.get('provider'))
    result['provider_runs'] = len(repairs['attempts']) + (1 if review else 0)
    ok = (bool(run) and result['reviewed_by_other_provider'] and result['provider_runs'] <= 2
          and step.get('repairs') == 0 and result['developer_checkout_unchanged']
          and (run['state'] == 'DONE' or step.get('state') == 'NeedsHuman'))
    result['passed'] = ok
    (root / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({k: result[k] for k in result if k != 'first_events'}, indent=2))
    sys.exit(0 if ok else 1)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('phase', choices=['prepare', 'plan', 'collect'])
    for name in ['--helper', '--fixture', '--root', '--app']:
        parser.add_argument(name, required=True)
    # D3: optional; an executable named here is pinned, one left out is discovered.
    for name in ['--claude-executable', '--codex-executable']:
        parser.add_argument(name)
    a = parser.parse_args()
    a.helper, a.fixture, a.app = (Path(p).resolve() for p in (a.helper, a.fixture, a.app))
    root = sample.check_disposable_root(a.root)
    {'prepare': prepare, 'plan': plan, 'collect': collect}[a.phase](root, a)
    if a.phase != 'collect':
        print('\nOpen the app on this data root (the plan steps run live from there):\n' + app_command(root, a))


if __name__ == '__main__':
    main()
