#!/usr/bin/env python3
"""P2-04 sample: deterministic disposable repository, feature bundle and helper pipe client.

Three things, each usable on its own and by the rehearsal (`gui_p2_04.py`):

* `repository` creates the sample repository (`slugify.py` stub, `test_slugify.py`) with one
  commit under an existing private disposable root. Same inputs, same commit.
* `bundle` prints the `ImportBundle` payload for that repository: the PRD, ADR, one-step
  plan inputs and the validation plan from `scripts/samples/slugify/`.
* `import` registers the repository and sends the bundle to a real `jojo-helper --serve`
  over pipes, once. The app has no registration or import control, so this is how a bundle
  reaches the helper. Importing creates drafts only; the same command is run again after
  each approval in the app and reports the stage reached.

Nothing here opens the database, starts a provider or approves anything, and nothing is
written outside the disposable root given on the command line.
"""
import argparse
import json
import os
import queue
import secrets
import stat
import subprocess
import sys
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SAMPLE = ROOT / 'scripts/samples/slugify'
REPOSITORY_NAME = 'slugify-sample'
FILES = ['slugify.py', 'test_slugify.py']
# The one commit the generator can produce from the files above.
SAMPLE_COMMIT = '45147111b537d1965241c4d2b40e32ad9b740ea9'
PLAN_SCHEMA = 'jojo-validation-plan-v2'
GIT = '/usr/bin/git'
GIT_ENV = {'PATH': '/usr/bin:/bin', 'GIT_CONFIG_NOSYSTEM': '1', 'GIT_CONFIG_GLOBAL': '/dev/null', 'TZ': 'UTC',
           'GIT_AUTHOR_NAME': 'Jojo Sample', 'GIT_AUTHOR_EMAIL': 'sample@jojo.invalid',
           'GIT_AUTHOR_DATE': '2026-11-15T00:00:00+0000', 'GIT_COMMITTER_NAME': 'Jojo Sample',
           'GIT_COMMITTER_EMAIL': 'sample@jojo.invalid', 'GIT_COMMITTER_DATE': '2026-11-15T00:00:00+0000'}
REHEARSAL_NOTE = ('\nRehearsal directive\nRead only by the fake Codex executable of the offline rehearsal. '
                  'A bundle for a live attempt never contains this section.\nfixture: {}\n')


def git(repository, *args):
    p = subprocess.run([GIT, *args], cwd=repository, env=GIT_ENV, capture_output=True, text=True, timeout=60)
    if p.returncode:
        raise RuntimeError(f'git {args[0]}: {p.stderr.strip()}')
    return p.stdout.strip()


def check_disposable_root(root):
    """The helper's rule (private, three levels deep), plus: never inside an existing project."""
    root = Path(root)
    if not root.is_absolute() or root.is_symlink() or not root.is_dir():
        raise ValueError(f'disposable root must be an existing absolute directory: {root}')
    real = root.resolve()
    if stat.S_IMODE(real.stat().st_mode) & 0o077 or len(real.parts) < 4:
        raise ValueError('disposable root must be private (0700) and at least three levels deep')
    for parent in [real, *real.parents]:
        if parent == ROOT or (parent / '.git').exists():
            raise ValueError(f'disposable root is inside an existing project: {parent}')
    return real


def generate_repository(disposable_root, name=REPOSITORY_NAME):
    """Creates <disposable root>/<name> with the sample files and one commit. Never overwrites."""
    root = check_disposable_root(disposable_root)
    if '/' in name or name in {'', '.', '..'}:
        raise ValueError('repository name must be one path component')
    repository = root / name
    repository.mkdir(mode=0o700)  # FileExistsError: an existing directory is never reused
    for file in FILES:
        target = repository / file
        target.write_bytes((SAMPLE / file).read_bytes())
        target.chmod(0o644)
    git(repository, 'init', '--quiet', '--template=', '--initial-branch=main')
    git(repository, 'add', '--', *FILES)
    git(repository, 'commit', '--quiet', '--no-verify', '-m', 'Sample: slugify stub and acceptance tests')
    commit = git(repository, 'rev-parse', 'HEAD')
    if commit != SAMPLE_COMMIT:
        raise RuntimeError(f'sample commit is {commit}, expected {SAMPLE_COMMIT}: the sample files changed')
    return {'repository': str(repository), 'commit': commit, 'branch': 'main', 'files': FILES}


def repository_state(repository):
    """What must be unchanged after any run: HEAD, branch, refs, remotes, index and working tree."""
    return {'head': git(repository, 'rev-parse', 'HEAD'), 'branch': git(repository, 'symbolic-ref', '--short', 'HEAD'),
            'refs': git(repository, 'for-each-ref', '--format=%(refname) %(objectname)'),
            'remotes': git(repository, 'remote', '-v'),
            'status': git(repository, 'status', '--porcelain', '--untracked-files=all')}


def manifest():
    return json.loads((SAMPLE / 'bundle.json').read_text())


def validation_plan():
    """The canonical validation-plan text (`ValidationPlan::canonical`); the helper refuses any other form."""
    m = manifest()
    step = [m['step'], '1', sorted(m['write_scope']), sorted(m['baseline']), sorted(m['tests']), m['command']]
    return json.dumps([PLAN_SCHEMA, [step]], separators=(',', ':'), ensure_ascii=False)


def adr_text(fixture=None):
    text = (SAMPLE / 'adr.md').read_text()
    return text + REHEARSAL_NOTE.format(fixture) if fixture else text


def import_payload(repository_id, base, fixture=None):
    """The exact `ImportBundle` field set. `fixture` is for the offline rehearsal only."""
    m = manifest()
    return {'feature': m['feature'], 'prd_id': m['prd_id'], 'adr_id': m['adr_id'], 'plan_id': m['plan_id'],
            'validation_id': m['validation_id'], 'prd': (SAMPLE / m['prd']).read_text(), 'adr': adr_text(fixture),
            'step': m['step'], 'repository': repository_id, 'base': base, 'provider': m['provider'],
            'validation': validation_plan()}


def start_subjects():
    m = manifest()
    return {'prd': m['prd_id'], 'adr': m['adr_id'], 'plan': m['plan_id'], 'validation': m['validation_id'],
            'step': m['step']}


class Helper:
    """One `jojo-helper --serve` session over private pipes, speaking protocol 1.1 frames only."""

    def __init__(self, executable, data_root, worktree_root, disposable_root, codex=(), stderr=None):
        self.proc = subprocess.Popen([str(executable), '--serve', '--data-root', str(data_root), '--worktree-root',
                                      str(worktree_root), '--disposable-root', str(disposable_root), *map(str, codex)],
                                     stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr, env={})
        self.replies = queue.Queue()
        threading.Thread(target=self._read, daemon=True).start()
        self.session = f'{secrets.randbits(96) + 1:032x}'
        self.epoch = 0
        hello = self.frame('Hello', {})
        if hello['type'] != 'Response' or hello['payload'].get('slice') is not True:
            raise RuntimeError(f'helper refused the session: {hello}')
        self.epoch = hello['epoch']
        self.hello = hello['payload']

    def _read(self):
        for line in self.proc.stdout:
            self.replies.put(json.loads(line))
        self.replies.put(None)

    def receive(self, timeout=30):
        try:
            reply = self.replies.get(timeout=timeout)
        except queue.Empty:
            raise RuntimeError('helper did not answer') from None
        if reply is None:
            raise RuntimeError(f'helper closed its output (exit {self.proc.wait(timeout=10)})')
        return reply

    def frame(self, kind, payload, revision=None, request=None):
        # A command identity is used once: an identity with a receipt is answered from it.
        request = request or f'{secrets.randbits(96) + 1:032x}'
        frame = {'major': 1, 'minor': 1, 'session': self.session, 'epoch': self.epoch, 'id': request,
                 'revision': revision, 'type': kind, 'payload': payload}
        self.proc.stdin.write((json.dumps(frame, separators=(',', ':'), ensure_ascii=False) + '\n').encode())
        self.proc.stdin.flush()
        reply = self.receive()
        if reply['id'] != request:
            raise RuntimeError(f'{kind}: unexpected frame {reply}')
        return reply

    def ok(self, kind, payload):
        reply = self.frame(kind, payload)
        if reply['type'] != 'Response':
            raise RuntimeError(f"{kind} refused: {reply['payload']}")
        return reply['payload']

    def close(self):
        """A clean close: READY receipt, then the helper exits on its own. Returns its exit code."""
        self.ok('PrepareClose', {})
        ready = self.receive()
        if ready['type'] != 'ShutdownReady':
            raise RuntimeError(f'close was not READY: {ready}')
        self.proc.stdin.close()
        return self.proc.wait(timeout=30)


def import_once(helper, repository, base, fixture=None):
    """Registers the repository (idempotent for the same path) and advances the bundle one stage."""
    repository_id = helper.ok('RegisterRepository', {'path': str(repository)})['response']
    stage = helper.ok('ImportBundle', import_payload(repository_id, base, fixture))['response']
    return {'repository_id': repository_id, 'stage': stage}


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    sub = parser.add_subparsers(dest='action', required=True)
    r = sub.add_parser('repository', help='create the sample repository under a private disposable root')
    r.add_argument('--disposable-root', required=True)
    r.add_argument('--name', default=REPOSITORY_NAME)
    b = sub.add_parser('bundle', help='print the ImportBundle payload')
    b.add_argument('--repository-id', required=True)
    b.add_argument('--base', default=SAMPLE_COMMIT)
    i = sub.add_parser('import', help='register the repository and send the bundle to a real helper, once')
    for name in ['--helper', '--data-root', '--worktree-root', '--disposable-root', '--repository']:
        i.add_argument(name, required=True)
    a = parser.parse_args()
    if a.action == 'repository':
        result = generate_repository(a.disposable_root, a.name)
    elif a.action == 'bundle':
        result = import_payload(a.repository_id, a.base)
    else:
        check_disposable_root(a.disposable_root)
        helper = Helper(a.helper, a.data_root, a.worktree_root, a.disposable_root)
        result = import_once(helper, a.repository, git(a.repository, 'rev-parse', 'HEAD'))
        result['helper_exit'] = helper.close()
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    try:
        main()
    except (ValueError, RuntimeError, OSError) as error:
        sys.exit(f'p2_04_sample: {error}')
