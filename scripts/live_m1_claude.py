#!/usr/bin/env python3
"""M1 live run: one Claude Code step on the generated sample repository, through a real helper.

This starts the installed Claude Code CLI under the user's subscription. Run it only with the
human's authorization for this exact run. It does what the app does, as protocol 1.1 frames
to a real `jojo-helper --serve`: register the sample repository, import the sample bundle with
the provider set to Claude Code, approve each exact revision, open admission after a reviewed
inspection, Start once, wait for the sealed result, ask for host validation, and close.

It never accepts or rejects the candidate, and nothing is pushed. Everything is created under
--root, a new private directory outside any project, and is kept as evidence.
"""
import argparse
import getpass
import hashlib
import json
import sys
import time
from pathlib import Path

import p2_04_sample as sample

VERSION = '2.1.311 (Claude Code)'


def decide(helper, subject):
    """Approves exactly the review bundle the helper returns for the subject."""
    bundle = helper.ok('ReviewBundle', {'subject': subject})
    revision = bundle.pop('revision')
    reply = helper.frame('HumanDecision', {**bundle, 'action': 'Approve'}, revision=revision)
    if reply['type'] != 'Response':
        raise RuntimeError(f'approval of {subject} refused: {reply["payload"]}')


def wait(what, probe, seconds):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        found = probe()
        if found is not None:
            return found
        time.sleep(2)
    raise RuntimeError(f'timed out waiting for {what}')


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    for name in ['--helper', '--root', '--claude-executable']:
        parser.add_argument(name, required=True)
    parser.add_argument('--timeout', type=int, default=900)
    a = parser.parse_args()
    root = sample.check_disposable_root(a.root)
    executable = Path(a.claude_executable)
    if executable.resolve() != executable:
        raise SystemExit('--claude-executable must be the fully resolved native binary')
    digest = hashlib.sha256(executable.read_bytes()).hexdigest()
    disposable = root / 'disposable'
    disposable.mkdir(mode=0o700)
    made = sample.generate_repository(disposable)
    before = sample.repository_state(made['repository'])
    claude = ['--claude-executable', executable, '--claude-version', VERSION, '--claude-sha256', digest,
              '--claude-home', Path.home(), '--claude-user', getpass.getuser(), '--claude-auth', 'native-subscription']
    with open(root / 'helper-stderr.txt', 'wb') as stderr:
        helper = sample.Helper(a.helper, root / 'data', root / 'worktrees', disposable, claude, stderr)
        result = {'claude': {'executable': str(executable), 'version': VERSION, 'sha256': digest}}
        try:
            repository_id = helper.ok('RegisterRepository', {'path': made['repository']})['response']
            payload = {**sample.import_payload(repository_id, made['commit']), 'provider': 'ClaudeCode'}
            m = sample.manifest()
            for subjects in [[m['prd_id']], [m['adr_id']], [m['validation_id'], m['plan_id']]]:
                helper.ok('ImportBundle', payload)
                for subject in subjects:
                    decide(helper, subject)
            result['import'] = helper.ok('ImportBundle', payload)['response']
            inspection = helper.ok('RecoveryInspect', {})
            helper.ok('RecoveryStart', {'digest': inspection['digest']})
            started = helper.ok('Start', sample.start_subjects())
            attempt = started['attempt']
            result['start'] = {'attempt': attempt, 'dispatch': started['dispatch']}
            print(f'started attempt {attempt}', flush=True)

            def sealed():
                c = helper.ok('Candidate', {'attempt': attempt})
                return c if c.get('sealed') else None
            candidate = wait('the sealed result', sealed, a.timeout)
            result['result'] = {k: candidate.get(k) for k in ['outcome', 'reason', 'provider_terminal', 'candidate', 'log_records']}
            print(f"sealed: {result['result']}", flush=True)
            if candidate.get('candidate'):
                changes = helper.ok('CandidateChanges', {'attempt': attempt, 'candidate': candidate['candidate'],
                                                         'offset': 0, 'limit': 8})
                result['changes'] = {'scope': changes['scope'], 'rows': changes['rows']}
                helper.ok('Validate', {'attempt': attempt, 'candidate': candidate['candidate']})

                def validated():
                    v = helper.ok('Candidate', {'attempt': attempt})['validation']
                    return v if v.get('state') in ('PASS', 'FAIL') else None
                result['validation'] = wait('host validation', validated, 300)
            after = sample.repository_state(made['repository'])
            # The owned worktree is a linked worktree on a Jojo branch at the base commit. That
            # one ref is Jojo's own; everything the human owns must be exactly as it was.
            owned = [ref for ref in after['refs'].splitlines() if ref.startswith('refs/heads/jojo/')]
            after['refs'] = '\n'.join(ref for ref in after['refs'].splitlines() if ref not in owned)
            result['repository_unchanged'] = after == before
            result['jojo_worktree_refs'] = owned
        finally:
            try:
                result['helper_exit'] = helper.close()
            except Exception as error:  # the evidence is still written
                result['helper_exit'] = f'close failed: {error}'
                helper.proc.kill()
            (root / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
    ok = (result.get('result', {}).get('outcome') == 'Succeeded' and result.get('validation', {}).get('state') == 'PASS'
          and result.get('repository_unchanged') and result.get('helper_exit') == 0)
    sys.exit(0 if ok else 1)


if __name__ == '__main__':
    main()
