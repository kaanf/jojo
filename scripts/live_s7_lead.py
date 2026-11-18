#!/usr/bin/env python3
"""Step 7 live run: a brief turn, discovery turns and a plan turn on the installed CLIs.

Every turn here is a read-only lead turn, run within the human's standing authorization of
2026-11-18 (STATUS.md, Open decisions 1): only in the sample repository this script generates
under --root, at most five live turns in all (one brief, up to three discovery turns, one plan
turn). Nothing writes to the repository, nothing is pushed. This script never approves: the
brief and the requirements draft are approved by the human in the app between the phases,
each against the same private directory under --root:

  brief     generate the sample repository, register it, save the project record and run the
            brief turn on Claude Code.
  discover  discovery turns (Codex, Claude Code, Codex) until the requirements draft exists,
            answering each question with the simplest option; records whether the approved
            brief was carried into each prompt.
  plan      the plan turn on Claude Code: an ADR draft and a multi-step draft plan; then
            checks the developer checkout and writes result.json.

The answers are typed by this script on the human's behalf and stored as such.
"""
import argparse
import getpass
import json
import queue
import subprocess
import sys
import threading
import time
from pathlib import Path

import p2_04_sample as sample

FEATURE = '00000000000000000000000000070700'
PROJECT = '00000000000000000000000000070701'
BRIEF_MESSAGE = 'The product owner brings: code.'
OBJECTIVE = 'slugify should also be usable from the command line.'
ANSWER = 'Take the simplest option that keeps the existing tests passing; nothing beyond that.'
BRIEF_HEADING = '# Project brief (approved by the product owner)'
DISCOVERY = ['Codex', 'ClaudeCode', 'Codex']
TIMEOUT = 600


class Helper(sample.Helper):
    """The sample client, speaking protocol 1.12 for every frame, started with HOME as the app does
    (D3: discovery asks the login shell, which needs it)."""

    def __init__(self, executable, data_root, worktree_root, disposable_root, extra, stderr):
        self.proc = subprocess.Popen([str(executable), '--serve', '--data-root', str(data_root), '--worktree-root',
                                      str(worktree_root), '--disposable-root', str(disposable_root), *map(str, extra)],
                                     stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr,
                                     env={'HOME': str(Path.home())})
        self.replies = queue.Queue()
        threading.Thread(target=self._read, daemon=True).start()
        self.session = f'{sample.secrets.randbits(96) + 1:032x}'
        self.epoch = 0
        hello = self.frame('Hello', {})
        if hello['type'] != 'Response' or hello['payload'].get('slice') is not True:
            raise RuntimeError(f'helper refused the session: {hello}')
        self.epoch = hello['epoch']
        self.hello = hello['payload']

    def frame(self, kind, payload, revision=None, request=None):
        request = request or f'{sample.secrets.randbits(96) + 1:032x}'
        frame = {'major': 1, 'minor': 12, 'session': self.session, 'epoch': self.epoch, 'id': request,
                 'revision': revision, 'type': kind, 'payload': payload}
        self.proc.stdin.write((json.dumps(frame, separators=(',', ':'), ensure_ascii=False) + '\n').encode())
        self.proc.stdin.flush()
        reply = self.receive(timeout=60)
        if reply['id'] != request:
            raise RuntimeError(f'{kind}: unexpected frame {reply}')
        return reply

    def send(self, kind, payload, seconds=60):
        """D3: retried while the helper is still finding or hashing the executable."""
        deadline = time.monotonic() + seconds
        while True:
            try:
                return self.ok(kind, payload)
            except RuntimeError as refused:
                if 'ExecutableChecking' not in str(refused) or time.monotonic() > deadline:
                    raise
                time.sleep(0.5)


def options():
    return ['--claude-home', Path.home(), '--claude-user', getpass.getuser(), '--claude-auth', 'native-subscription',
            '--codex-home', Path.home() / '.codex', '--codex-auth', 'native-subscription']


def open_helper(root, a):
    stderr = open(root / 'helper-stderr.txt', 'ab')
    helper = Helper(a.helper, root / 'data', root / 'worktrees', root / 'disposable', options(), stderr)
    inspection = helper.ok('RecoveryInspect', {})
    if inspection['subjects'] == 0:
        helper.ok('RecoveryStart', {'digest': inspection['digest']})
    return helper


def app_command(root, a):
    parts = [a.app, '--helper', a.helper, '--data-root', root / 'data', '--worktree-root', root / 'worktrees',
             '--disposable-root', root / 'disposable', *options()]
    return ' '.join(f"'{p}'" if ' ' in str(p) else str(p) for p in parts)


def brief_row(helper):
    detail = helper.ok('ProjectDetail', {'project': PROJECT})
    return detail['repositories'][0].get('brief')


def chat_turn(helper, repository, kind, provider, message):
    sent = helper.send('ChatTurn', {'feature': FEATURE, 'kind': kind, 'repository': repository,
                                    'provider': provider, 'message': message})
    started = time.monotonic()
    print(f'{kind} turn on {provider}: {sent["dispatch"]}', flush=True)
    while time.monotonic() - started < TIMEOUT:
        chat = helper.ok('FeatureChat', {'feature': FEATURE, 'after': 0})
        row = next(r for r in chat['turns'] if r['turn'] == sent['turn'])
        if row['state'] not in ('Running', 'Stopping'):
            prompt = helper.ok('ChatText', {'subject': sent['turn'], 'part': 'Prompt', 'offset': 0})['text']
            return chat, row, round(time.monotonic() - started), BRIEF_HEADING in prompt
        time.sleep(2)
    raise RuntimeError(f'{kind} turn {sent["turn"]} did not end in {TIMEOUT} s')


def save(root, name, value):
    (root / name).write_text(json.dumps(value, indent=2, ensure_ascii=False) + '\n')


def brief(root, a):
    (root / 'disposable').mkdir(mode=0o700)
    made = sample.generate_repository(root / 'disposable')
    save(root, 'before.json', sample.repository_state(made['repository']))
    helper = open_helper(root, a)
    try:
        repository = helper.ok('RegisterRepository', {'path': made['repository']})['response']
        (root / 'repository.txt').write_text(repository + '\n' + made['repository'] + '\n')
        helper.ok('SaveProject', {
            'project': PROJECT, 'revision': 0, 'name': 'Slugify sample', 'repositories': [repository],
            'instructions': 'Change only the files of your step.',
            'test_command': ['/usr/bin/python3', '-m', 'unittest'], 'ci_command': [],
            'kinds': [], 'default_provider': 'ClaudeCode', 'capacity': 2, 'repair_limit': 0, 'review': False})
        sent = helper.send('BriefTurn', {'repository': repository, 'provider': 'ClaudeCode', 'message': BRIEF_MESSAGE})
        started = time.monotonic()
        print(f'brief turn on ClaudeCode: {sent["dispatch"]}', flush=True)
        while True:
            row = brief_row(helper)
            if row and row['turn'] and row['turn']['state'] not in ('Running', 'Stopping'):
                break
            if time.monotonic() - started > TIMEOUT:
                raise RuntimeError(f'the brief turn did not end in {TIMEOUT} s')
            time.sleep(2)
        row['seconds'] = round(time.monotonic() - started)
        save(root, 'brief.json', row)
        print(json.dumps(row, indent=2, ensure_ascii=False))
    finally:
        print('helper exit', helper.close())


def discover(root, a):
    repository = (root / 'repository.txt').read_text().split('\n')[0]
    helper = open_helper(root, a)
    rounds = []
    try:
        row = brief_row(helper)
        approval = row['brief']['approval'] if row and row['brief'] else None
        for index, provider in enumerate(DISCOVERY):
            chat, turn, seconds, carried = chat_turn(helper, repository, 'Discover', provider,
                                                     OBJECTIVE if index == 0 else '')
            entry = {k: turn[k] for k in ['ordinal', 'provider', 'state', 'detail', 'summary']}
            entry.update(seconds=seconds, brief_in_prompt=carried,
                         questions=[{k: q[k] for k in ['prompt', 'blocking']} for q in turn['questions']])
            rounds.append(entry)
            print(json.dumps(entry, indent=2, ensure_ascii=False), flush=True)
            if turn['state'] != 'Replied':
                break
            for question in turn['questions']:
                helper.ok('AnswerQuestion', {'question': question['question'], 'revision': question['revision'],
                                             'action': 'Answer', 'answer': ANSWER})
            if chat['prd'] is not None and not turn['questions']:
                break
        chat = helper.ok('FeatureChat', {'feature': FEATURE, 'after': 0})
        if chat['prd'] is not None:
            text = helper.ok('ChatText', {'subject': chat['prd']['document'], 'part': 'Prd', 'offset': 0})
            (root / 'prd.md').write_text(text['text'])
        save(root, 'discover.json', {'brief_approval': approval, 'rounds': rounds, 'prd': chat['prd']})
        print('requirements draft:', json.dumps(chat['prd']))
    finally:
        print('helper exit', helper.close())


def plan(root, a):
    repository, path = (root / 'repository.txt').read_text().split('\n')[:2]
    helper = open_helper(root, a)
    result = {}
    try:
        chat, turn, seconds, carried = chat_turn(helper, repository, 'Plan', 'ClaudeCode', '')
        result['plan_turn'] = {k: turn[k] for k in ['ordinal', 'provider', 'state', 'detail', 'summary', 'plan']}
        result['plan_turn'].update(seconds=seconds, brief_in_prompt=carried)
        if turn['plan']:
            result['adr'] = helper.ok('Document', {'document': turn['plan']['adr'], 'revision': None, 'offset': 0})
            result['plan'] = helper.ok('PlanDetail', {'plan': turn['plan']['plan']}) if turn['plan']['plan'] else None
    finally:
        try:
            result['helper_exit'] = helper.close()
        except Exception as error:  # the evidence is still written
            result['helper_exit'] = f'close failed: {error}'
            helper.proc.kill()
    before = json.loads((root / 'before.json').read_text())
    result['repository_unchanged'] = sample.repository_state(path) == before
    save(root, 'result.json', result)
    print(json.dumps({k: result[k] for k in ['plan_turn', 'repository_unchanged', 'helper_exit']},
                     indent=2, ensure_ascii=False))
    ok = (result['plan_turn']['state'] == 'Replied' and result['plan_turn']['plan'] is not None
          and result['repository_unchanged'] and result['helper_exit'] == 0)
    sys.exit(0 if ok else 1)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('phase', choices=['brief', 'discover', 'plan'])
    for name in ['--helper', '--root', '--app']:
        parser.add_argument(name, required=True)
    a = parser.parse_args()
    a.helper, a.app = Path(a.helper).resolve(), Path(a.app).resolve()
    root = sample.check_disposable_root(a.root)
    {'brief': brief, 'discover': discover, 'plan': plan}[a.phase](root, a)
    print('\nOpen the app on this data root to approve in the Approval Inbox:\n' + app_command(root, a))


if __name__ == '__main__':
    main()
