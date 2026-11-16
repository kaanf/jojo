#!/usr/bin/python3 -I
"""Test-only P1-13 protocol stand-in for runtime GUI scenarios that need active work.

The real helper exposes no Start command over stdio, so the GUI cannot create active work
against it. This fixture launches nothing and signals nothing: it only answers frames and
records what the GUI sent to <data-root>/received.jsonl. Scenario: <data-root>/scenario.json.
"""
import json
import sys
from pathlib import Path

args = sys.argv[1:]
data = Path(args[args.index('--data-root') + 1])
scenario = json.loads((data / 'scenario.json').read_text())
log = (data / 'received.jsonl').open('a')
TICKET = '000000000000000000000000000000a1'
SHUTDOWN = '800000000000000500000000000000b2'
EPOCH = 5
state = {'state': 'Open', 'ticket': None, 'active': scenario.get('active', 0)}
progress = list(scenario.get('progress', [1, 0]))
session = None


def write(kind, payload, request='0' * 32, null=False):
    frame = {'major': 1, 'minor': 0, 'session': session or '0' * 32, 'epoch': EPOCH, 'id': request,
             'revision': None, 'type': kind, 'payload': None if null else payload}
    sys.stdout.write(json.dumps(frame, sort_keys=True, separators=(',', ':')) + '\n')
    sys.stdout.flush()


def ack(e, response):
    write('Response', {'acknowledged': True, 'operation_completed': False, 'command': e['id'], 'epoch': EPOCH,
                       'event_seq': 10, 'response': response, 'replayed': False}, e['id'])


def receipt(ticket, ready):
    return {'ticket': ticket, 'shutdown': SHUTDOWN, 'event_seq': 42, 'outcome': 'Ready' if ready else 'Failed',
            'process_stop_confirmed': ready, 'persistence_sealed': ready,
            'targets': ['attempt:000000000000000000000000000000a2'],
            'recovery_items': ['attempt:000000000000000000000000000000a2'],
            'failures': [] if ready else ['attempt a2: stop unconfirmed after TERM/KILL budget']}


for line in sys.stdin:
    e = json.loads(line)
    session = session or e['session']
    log.write(json.dumps({'type': e['type'], 'id': e['id'], 'payload': e['payload']}) + '\n')
    log.flush()
    kind = e['type']
    if kind == 'Hello':
        write('Response', {'compatible': True, 'epoch': EPOCH}, e['id'])
    elif kind == 'PrepareClose':
        if state['active'] == 0:
            ack(e, 'NO_ACTIVE')
            write('ShutdownReady', receipt('000000000000000000000000000000c3', True))
            sys.exit(0)
        state.update(state='AwaitingConfirmation', ticket=TICKET)
        ack(e, f"CONFIRM_REQUIRED {state['active']}")
    elif kind == 'CancelClose':
        state.update(state='Open', ticket=None)
        ack(e, 'CANCELLED')
    elif kind == 'ConfirmClose':
        state['state'] = 'Stopping'
        ack(e, f"CONFIRMED {state['active']}")
    elif kind == 'CloseStatus':
        if state['state'] == 'Stopping' and progress:
            state['active'] = progress.pop(0)
        write('Response', {'state': state['state'], 'ticket': state['ticket'], 'active_count': state['active'],
                           'receipt': None}, e['id'])
        if state['state'] == 'Stopping' and not progress:
            if scenario.get('outcome', 'ready') == 'ready':
                write('ShutdownReady', receipt(TICKET, True))
                sys.exit(0)
            state['state'] = 'Failed'
            write('ShutdownFailure', receipt(TICKET, False))
    elif kind == 'CancelAttempt':
        if scenario.get('answer_commands', True):
            ack(e, 'cancel committed; completion requires result')
    elif kind in ('Receipt', 'ReviewBundle', 'ShutdownReceipt'):
        write('Response', None, e['id'], null=True)
    elif kind == 'Recovery':
        rows = [{'attempt': '000000000000000000000000000000a2', 'assessment': 'ConfirmedDead',
                 'findings': 'worktree=/tmp/jojo-worktrees/a2 stdout=12 bytes', 'detail_complete': True}]
        write('Response', {'digest': 'fixture', 'rows': rows if e['payload']['offset'] == 0 else []}, e['id'])
    elif kind == 'Snapshot':
        write('Response', {'as_of_event_seq': 10, 'epoch': EPOCH, 'admission': 'Closed', 'shutdown': 'Unclean',
                           'section': 'aggregates', 'next': None, 'rows': []}, e['id'])
    elif kind == 'Events':
        frame = {'major': 1, 'minor': 0, 'session': session, 'epoch': EPOCH, 'id': e['id'], 'revision': None,
                 'type': 'Response', 'payload': []}  # the real helper answers Events with a list
        sys.stdout.write(json.dumps(frame, sort_keys=True, separators=(',', ':')) + '\n')
        sys.stdout.flush()
    else:
        write('Error', {'code': 'UnknownType'}, e['id'])
log.write(json.dumps({'type': 'EOF'}) + '\n')
log.flush()
