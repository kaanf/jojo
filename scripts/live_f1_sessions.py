#!/usr/bin/env python3
"""Follow-ups live run: session continuation and choice answers on the installed CLIs.

Four read-only discovery turns, run within the human's standing authorization of 2026-11-18
(STATUS.md, Open decisions 1), only in the sample repository this script generates under
--root: Codex (fresh), Codex (continues the first), Claude Code (fresh: another provider never
continues a session it did not start), Claude Code (continues the third). Between turns every
question is answered with the first choice the lead offered, else the simplest option. Nothing
is approved, nothing writes to the repository, nothing is pushed. Writes result.json; exits 0
when both continuations ran as Continued and the repository is unchanged.
"""
import argparse
import json
import sys
import time
from pathlib import Path

import live_s7_lead as s7
import p2_04_sample as sample

FEATURE = '000000000000000000000000000f1f00'
OBJECTIVE = ('slugify should also be usable from the command line. Ask me at least one question, '
             'and offer choices where the answer is one of a few options.')
ANSWER = 'Take the simplest option that keeps the existing tests passing; nothing beyond that.'
TURNS = ['Codex', 'Codex', 'ClaudeCode', 'ClaudeCode']


class Helper(s7.Helper):
    """The step 7 client at protocol 1.14."""

    def frame(self, kind, payload, revision=None, request=None):
        request = request or f'{sample.secrets.randbits(96) + 1:032x}'
        frame = {'major': 1, 'minor': 14, 'session': self.session, 'epoch': self.epoch, 'id': request,
                 'revision': revision, 'type': kind, 'payload': payload}
        self.proc.stdin.write((json.dumps(frame, separators=(',', ':'), ensure_ascii=False) + '\n').encode())
        self.proc.stdin.flush()
        reply = self.receive(timeout=60)
        if reply['id'] != request:
            raise RuntimeError(f'{kind}: unexpected frame {reply}')
        return reply


def turn(helper, repository, provider, message):
    sent = helper.send('ChatTurn', {'feature': FEATURE, 'kind': 'Discover', 'repository': repository,
                                    'provider': provider, 'message': message})
    started = time.monotonic()
    while time.monotonic() - started < s7.TIMEOUT:
        chat = helper.ok('FeatureChat', {'feature': FEATURE, 'after': 0})
        row = next(r for r in chat['turns'] if r['turn'] == sent['turn'])
        if row['state'] not in ('Running', 'Stopping'):
            return row, round(time.monotonic() - started)
        time.sleep(2)
    raise RuntimeError(f'turn {sent["turn"]} did not end in {s7.TIMEOUT} s')


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--helper', required=True)
    parser.add_argument('--root', required=True)
    a = parser.parse_args()
    a.helper = Path(a.helper).resolve()
    root = sample.check_disposable_root(a.root)
    (root / 'disposable').mkdir(mode=0o700)
    made = sample.generate_repository(root / 'disposable')
    before = sample.repository_state(made['repository'])
    stderr = open(root / 'helper-stderr.txt', 'ab')
    helper = Helper(a.helper, root / 'data', root / 'worktrees', root / 'disposable', s7.options(), stderr)
    rounds = []
    try:
        inspection = helper.ok('RecoveryInspect', {})
        if inspection['subjects'] == 0:
            helper.ok('RecoveryStart', {'digest': inspection['digest']})
        repository = helper.ok('RegisterRepository', {'path': made['repository']})['response']
        for index, provider in enumerate(TURNS):
            row, seconds = turn(helper, repository, provider, OBJECTIVE if index == 0 else '')
            entry = {k: row[k] for k in ['ordinal', 'provider', 'state', 'detail', 'session', 'summary']}
            entry.update(seconds=seconds, questions=[
                {'prompt': q['prompt'], 'blocking': q['blocking'], 'choices': c}
                for q, c in zip(row['questions'], row['choices'])])
            rounds.append(entry)
            print(json.dumps(entry, indent=2, ensure_ascii=False), flush=True)
            for question, choices in zip(row['questions'], row['choices']):
                helper.ok('AnswerQuestion', {'question': question['question'], 'revision': question['revision'],
                                             'action': 'Answer', 'answer': choices[0] if choices else ANSWER})
            if row['state'] != 'Replied':
                break
    finally:
        try:
            exit_code = helper.close()
        except Exception as error:  # the evidence is still written
            exit_code = f'close failed: {error}'
            helper.proc.kill()
    result = {'rounds': rounds, 'helper_exit': exit_code,
              'repository_unchanged': sample.repository_state(made['repository']) == before,
              'choices_offered': sum(1 for r in rounds for q in r['questions'] if q['choices'])}
    (root / 'result.json').write_text(json.dumps(result, indent=2, ensure_ascii=False) + '\n')
    sessions = [r['session'] for r in rounds]
    print(json.dumps({k: result[k] for k in ['helper_exit', 'repository_unchanged', 'choices_offered']}), sessions)
    ok = sessions == ['Fresh', 'Continued', 'Fresh', 'Continued'] and result['repository_unchanged']
    sys.exit(0 if ok else 1)


if __name__ == '__main__':
    main()
