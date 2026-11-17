#!/usr/bin/python3 -I
"""Test-only protocol 1.5 stand-in for the M4 GUI scenario (repairs and reviews).

It launches nothing, repairs nothing and reviews nothing: it reports three attempts of one
plan step in its snapshot and answers `Repairs` the way the real helper's contract says,
and records every frame the GUI sent to <data-root>/received.jsonl.
"""
import json
import sys
from pathlib import Path

args = sys.argv[1:]
data = Path(args[args.index('--data-root') + 1])
log = (data / 'received.jsonl').open('a')
EPOCH = 5
FIRST, REPAIR, HELD = 'a1' * 16, 'b2' * 16, 'c3' * 16
state = {'session': None}


def write(kind, payload, request, minor=1):
    frame = {'major': 1, 'minor': minor, 'session': state['session'], 'epoch': EPOCH, 'id': request,
             'revision': None, 'type': kind, 'payload': payload}
    sys.stdout.write(json.dumps(frame, sort_keys=True, separators=(',', ':')) + '\n')
    sys.stdout.flush()


def ack(e, response):
    write('Response', {'acknowledged': True, 'operation_completed': False, 'command': e['id'], 'epoch': EPOCH,
                       'event_seq': 10, 'response': response, 'replayed': False}, e['id'])


def row(attempt, key, state_, repair, hold, review):
    return {'attempt': attempt, 'plan': 'd4' * 16, 'step': 'e5' * 16, 'key': key, 'provider': 'Codex',
            'state': state_, 'repair': repair, 'hold': hold, 'review': review}


REVIEW = {'review': 'f6' * 16, 'provider': 'ClaudeCode', 'state': 'REJECTED', 'notes': 'src/fix.txt: handle zero.',
          'objection': None, 'detail': ''}
REPAIRS = {'complete': True, 'attempts': [
    row(HELD, 'never', 'NeedsHuman', {'ordinal': 2, 'limit': 2, 'failure': 'host validation failed: exit 1'},
        'host validation failed; 2 of 2 repairs used', None),
    row(REPAIR, 'fix', 'Repairing', {'ordinal': 1, 'limit': 2, 'failure': 'rejected by ClaudeCode'}, None, None),
    row(FIRST, 'fix', None, None, None, REVIEW),
]}

for line in sys.stdin:
    e = json.loads(line)
    state['session'] = state['session'] or e['session']
    log.write(json.dumps({'type': e['type'], 'id': e['id'], 'minor': e['minor'], 'payload': e['payload']}) + '\n')
    log.flush()
    kind = e['type']
    if kind == 'Hello':
        write('Response', {'compatible': True, 'epoch': EPOCH, 'slice': True}, e['id'])
    elif kind == 'Snapshot':
        rows = [{'key': f'scheduler_attempt:{a}', 'value': 'RESERVED', 'detail_complete': True}
                for a in (FIRST, REPAIR, HELD)]
        write('Response', {'as_of_event_seq': 10, 'epoch': EPOCH, 'admission': 'Open', 'shutdown': 'Clean',
                           'section': 'aggregates', 'next': None, 'rows': rows}, e['id'])
    elif kind == 'Events':
        write('Response', [], e['id'])
    elif kind == 'Recovery':
        write('Response', {'digest': 'fixture-1', 'rows': []}, e['id'])
    elif kind == 'Repairs' and e['minor'] >= 5:
        write('Response', REPAIRS, e['id'], minor=5)
    elif kind in ('Receipt', 'ReviewBundle', 'ShutdownReceipt'):
        write('Response', None, e['id'])
    elif kind == 'PrepareClose':
        ack(e, 'NO_ACTIVE')
        write('ShutdownReady', {'ticket': '000000000000000000000000000000c3', 'shutdown': '800000000000000500000000000000b2',
                                'event_seq': 42, 'outcome': 'Ready', 'process_stop_confirmed': True,
                                'persistence_sealed': True, 'targets': [], 'recovery_items': [], 'failures': []},
              '0' * 32)
        sys.exit(0)
    else:
        write('Error', {'code': 'UnknownType'}, e['id'])
log.write(json.dumps({'type': 'EOF'}) + '\n')
log.flush()
