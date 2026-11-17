#!/usr/bin/env python3
"""M2 live run: Feature Chat rounds on the generated sample repository, through a real helper.

This starts the installed Claude Code and Codex CLIs under the user's subscriptions, read-only
in a repository registered with the core under the disposable root. Run it only within the
human's authorization. It does what the app does, as protocol 1.2 frames to a real
`jojo-helper --serve`: register the sample repository, open admission after a reviewed
inspection, send a discovery turn with an objective, answer each question it asks, and send
further discovery turns until a requirements draft exists or the round limit is reached.

The answers are typed by this script on the human's behalf and stored as such; they say only
to take the simplest option. It never approves anything: the four document approvals stay
with the human. Nothing is pushed. Everything is created under --root, a new private
directory outside any project, and is kept as evidence.
"""
import argparse
import getpass
import hashlib
import json
import sys
import time
from pathlib import Path

import p2_04_sample as sample

CLAUDE_VERSION = '2.1.311 (Claude Code)'
CODEX_VERSION = 'codex-cli 0.169.0'
FEATURE = '00000000000000000000000000020500'
OBJECTIVE = 'slugify should also be usable from the command line.'
ANSWER = 'Take the simplest option that keeps the existing tests passing; nothing beyond that.'


class Helper(sample.Helper):
    """The sample client, speaking protocol 1.2 for every frame."""

    def frame(self, kind, payload, revision=None, request=None, minor=2):
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


def wait_for_result(helper, turn, seconds):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        chat = helper.ok('FeatureChat', {'feature': FEATURE, 'after': 0})
        row = next(r for r in chat['turns'] if r['turn'] == turn)
        if row['state'] not in ('Running', 'Stopping'):
            return chat, row
        time.sleep(2)
    raise RuntimeError(f'timed out waiting for turn {turn}')


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    for name in ['--helper', '--root']:
        parser.add_argument(name, required=True)
    # D3: optional; an executable named here is pinned, one left out is discovered.
    for name in ['--claude-executable', '--codex-executable']:
        parser.add_argument(name)
    parser.add_argument('--rounds', type=int, default=2, help='live turns at most (each is one provider run)')
    parser.add_argument('--timeout', type=int, default=600)
    a = parser.parse_args()
    root = sample.check_disposable_root(a.root)
    pins = {'claude': (a.claude_executable, CLAUDE_VERSION), 'codex': (a.codex_executable, CODEX_VERSION)}
    for executable, _ in pins.values():
        if executable and Path(executable).resolve() != Path(executable):
            raise SystemExit(f'{executable} must be the fully resolved native binary')
    disposable = root / 'disposable'
    disposable.mkdir(mode=0o700)
    made = sample.generate_repository(disposable)
    before = sample.repository_state(made['repository'])

    def pinned(name):
        executable, version = pins[name]
        if not executable:
            return []
        return [f'--{name}-executable', executable, f'--{name}-version', version, f'--{name}-sha256', digest(executable)]

    options = [*pinned('claude'),
               '--claude-home', Path.home(), '--claude-user', getpass.getuser(), '--claude-auth', 'native-subscription',
               *pinned('codex'),
               '--codex-home', Path.home() / '.codex', '--codex-auth', 'native-subscription']
    result = {name: {'version': version, 'sha256': digest(executable)} if executable else 'discovered by the helper'
              for name, (executable, version) in pins.items()}
    result['rounds'] = []
    with open(root / 'helper-stderr.txt', 'wb') as stderr:
        helper = Helper(a.helper, root / 'data', root / 'worktrees', disposable, options, stderr)
        try:
            repository = helper.ok('RegisterRepository', {'path': made['repository']})['response']
            inspection = helper.ok('RecoveryInspect', {})
            helper.ok('RecoveryStart', {'digest': inspection['digest']})
            providers = ['ClaudeCode', 'Codex']
            for index in range(a.rounds):
                provider = providers[index % 2]
                message = OBJECTIVE if index == 0 else ''
                sent = helper.ok('ChatTurn', {'feature': FEATURE, 'kind': 'Discover', 'repository': repository,
                                              'provider': provider, 'message': message})
                turn = sent['turn']
                print(f'round {index + 1}: {provider} turn {turn} {sent["dispatch"]}', flush=True)
                chat, row = wait_for_result(helper, turn, a.timeout)
                entry = {k: row[k] for k in ['ordinal', 'provider', 'state', 'detail', 'summary', 'prd']}
                entry['questions'] = [{k: q[k] for k in ['prompt', 'blocking']} for q in row['questions']]
                result['rounds'].append(entry)
                print(json.dumps(entry, indent=2), flush=True)
                if row['state'] != 'Replied':
                    break
                for question in row['questions']:
                    helper.ok('AnswerQuestion', {'question': question['question'], 'revision': question['revision'],
                                                 'action': 'Answer', 'answer': ANSWER})
                if chat['prd'] is not None and not row['questions']:
                    break
            chat = helper.ok('FeatureChat', {'feature': FEATURE, 'after': 0})
            result['prd'] = chat['prd']
            if chat['prd'] is not None:
                text = helper.ok('ChatText', {'subject': chat['prd']['document'], 'part': 'Prd', 'offset': 0})
                (root / 'prd.md').write_text(text['text'])
            after = sample.repository_state(made['repository'])
            result['repository_unchanged'] = after == before
        finally:
            try:
                result['helper_exit'] = helper.close()
            except Exception as error:  # the evidence is still written
                result['helper_exit'] = f'close failed: {error}'
                helper.proc.kill()
            (root / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({k: result[k] for k in ['prd', 'repository_unchanged', 'helper_exit'] if k in result}, indent=2))
    ok = (all(r['state'] == 'Replied' for r in result['rounds']) and result.get('prd') is not None
          and result['prd']['approved'] is False and result.get('repository_unchanged')
          and result.get('helper_exit') == 0)
    sys.exit(0 if ok else 1)


if __name__ == '__main__':
    main()
