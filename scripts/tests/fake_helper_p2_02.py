#!/usr/bin/python3 -I
"""Test-only protocol 1.1 stand-in for the P2-02 GUI scenarios (Start and recovery controls).

It launches nothing and signals nothing: it answers frames the way the real helper's
contract says and records every frame the GUI sent to <data-root>/received.jsonl, so a
scenario can assert exactly what the controls put on the wire. Scenario:
<data-root>/scenario.json with optional `admission` (default Closed) and `answer_start`.
"""
import json
import sys
from pathlib import Path

args = sys.argv[1:]
data = Path(args[args.index('--data-root') + 1])
scenario = json.loads((data / 'scenario.json').read_text())
log = (data / 'received.jsonl').open('a')
EPOCH = 5
ATTEMPT = '000000000000000000000000000000a2'
state = {'admission': scenario.get('admission', 'Closed'), 'inspection': 1, 'session': None}


def write(kind, payload, request):
    frame = {'major': 1, 'minor': 1, 'session': state['session'], 'epoch': EPOCH, 'id': request,
             'revision': None, 'type': kind, 'payload': payload}
    sys.stdout.write(json.dumps(frame, sort_keys=True, separators=(',', ':')) + '\n')
    sys.stdout.flush()


def ack(e, response, **extra):
    write('Response', {'acknowledged': True, 'operation_completed': False, 'command': e['id'], 'epoch': EPOCH,
                       'event_seq': 10, 'response': response, 'replayed': False, **extra}, e['id'])


def digest():
    return f"fixture-{state['inspection']}"


for line in sys.stdin:
    e = json.loads(line)
    state['session'] = state['session'] or e['session']
    log.write(json.dumps({'type': e['type'], 'id': e['id'], 'minor': e['minor'], 'payload': e['payload']}) + '\n')
    log.flush()
    kind, p = e['type'], e['payload']
    if kind == 'Hello':
        write('Response', {'compatible': True, 'epoch': EPOCH, 'slice': True}, e['id'])
    elif kind == 'Snapshot':
        write('Response', {'as_of_event_seq': 10, 'epoch': EPOCH, 'admission': state['admission'], 'shutdown': 'Unclean',
                           'section': 'aggregates', 'next': None, 'rows': []}, e['id'])
    elif kind == 'Events':
        write('Response', [], e['id'])
    elif kind == 'Recovery':
        rows = [{'attempt': ATTEMPT, 'assessment': 'Assessment { blockers: ["process state unknown"], choices: [Hold] }',
                 'findings': 'stage=Spawned; worktree=/tmp/jojo-worktrees/a2', 'detail_complete': True}]
        write('Response', {'digest': digest(), 'rows': rows if p['offset'] == 0 else []}, e['id'])
    elif kind == 'RecoveryInspect':
        state['inspection'] += 1
        write('Response', {'digest': digest(), 'subjects': 1, 'other_quarantined': 0}, e['id'])
    elif kind == 'RecoveryResolve':
        if p.get('digest') != digest():
            write('Error', {'code': 'StaleInspection: inspect again'}, e['id'])
        elif p.get('choice') != 'Hold':
            write('Error', {'code': f"{p.get('choice')} is not offered: process state unknown"}, e['id'])
        else:
            ack(e, 'HOLD')
    elif kind == 'RecoveryStart':
        if p.get('digest') != digest():
            write('Error', {'code': 'StaleInspection: inspect again'}, e['id'])
        else:
            state['admission'] = 'Open'
            ack(e, 'admission open after reviewed recovery')
    elif kind == 'Start':
        if not scenario.get('answer_start', True):
            pass  # the response is lost
        elif state['admission'] != 'Open':
            write('Error', {'code': 'AdmissionClosed'}, e['id'])
        else:
            ack(e, f"accepted attempt={e['id']}", attempt=e['id'], dispatch='launched')
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
