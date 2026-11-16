#!/usr/bin/python3 -I
"""Test-only protocol 1.1 stand-in for the P2-03 GUI scenarios (candidate review).

It launches nothing, validates nothing and signals nothing: it answers frames the way the
real helper's contract says and records every frame the GUI sent to
<data-root>/received.jsonl, so a scenario can assert exactly what the controls put on the
wire. Scenario (<data-root>/scenario.json):
  views            validation objects; the first is shown until Validate, the second on the
                   first Candidate query after it, the third afterwards
  changes          the CandidateChanges payload, or {"error": code}
  log              one agent output record (what the provider said)
  answer_validate  false: the Validate response is lost
  refuse_accept    error code for an Accept decision
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
CANDIDATE = 'ab' * 32
state = {'session': None, 'view': 0, 'decision': 'NONE'}
views = scenario['views']


def write(kind, payload, request):
    frame = {'major': 1, 'minor': 1, 'session': state['session'], 'epoch': EPOCH, 'id': request,
             'revision': None, 'type': kind, 'payload': payload}
    sys.stdout.write(json.dumps(frame, sort_keys=True, separators=(',', ':')) + '\n')
    sys.stdout.flush()


def ack(e, response, **extra):
    write('Response', {'acknowledged': True, 'operation_completed': False, 'command': e['id'], 'epoch': EPOCH,
                       'event_seq': 10, 'response': response, 'replayed': False, **extra}, e['id'])


def page(text, offset):
    chunk = text[offset:offset + 4096]
    return {'attempt': ATTEMPT, 'offset': offset, 'next': offset + len(chunk), 'total': len(text), 'complete': True,
            'text': chunk}


for line in sys.stdin:
    e = json.loads(line)
    state['session'] = state['session'] or e['session']
    log.write(json.dumps({'type': e['type'], 'id': e['id'], 'minor': e['minor'], 'payload': e['payload']}) + '\n')
    log.flush()
    kind, p = e['type'], e['payload']
    if kind == 'Hello':
        write('Response', {'compatible': True, 'epoch': EPOCH, 'slice': True}, e['id'])
    elif kind == 'Snapshot':
        write('Response', {'as_of_event_seq': 10, 'epoch': EPOCH, 'admission': 'Open', 'shutdown': 'Clean',
                           'section': 'aggregates', 'next': None, 'rows': []}, e['id'])
    elif kind == 'Events':
        write('Response', [], e['id'])
    elif kind == 'Recovery':
        write('Response', {'digest': 'fixture-1', 'rows': []}, e['id'])
    elif kind == 'Candidate':
        view = dict(views[min(state['view'], len(views) - 1)])
        if state['view'] >= 1:
            state['view'] += 1
        view.setdefault('detail', '')
        view.setdefault('receipt', None)
        view.setdefault('reasons', [])
        view['candidate'] = CANDIDATE if view['receipt'] is not None else None
        view['reasons_total'] = len(view['reasons'])
        write('Response', {'attempt': ATTEMPT, 'sealed': True, 'outcome': 'Succeeded', 'reason': 'provider turn completed',
                           'provider_terminal': 'turn.completed', 'candidate': CANDIDATE, 'log_records': 1,
                           'validation': view, 'decision': state['decision']}, e['id'])
    elif kind == 'CandidateChanges':
        changes = scenario.get('changes', {'scope': 'PASS', 'rows': [
            {'path': 'src/feature.rs', 'kind': 'Added', 'scope': 'ALLOWED', 'detail_complete': True}]})
        if 'error' in changes:
            write('Error', {'code': changes['error']}, e['id'])
        else:
            write('Response', {'attempt': ATTEMPT, 'candidate': CANDIDATE, 'base': 'c0' * 20,
                               'total': len(changes['rows']), **changes}, e['id'])
    elif kind == 'CandidateDiff':
        write('Response', page('untracked new file src/feature.rs\n+written by fixture\n', p['offset']), e['id'])
    elif kind == 'ValidationReport':
        write('Response', page('jojo-validator-report/1\nresult: fixture\n', p['offset']), e['id'])
    elif kind == 'Log':
        write('Response', {'attempt': ATTEMPT, 'sequence': p['sequence'], 'offset': 0, 'next': 1, 'total': 1,
                           'type': 'AgentMessage', 'text': scenario.get('log', 'developed')}, e['id'])
    elif kind == 'Validate':
        if p != {'attempt': ATTEMPT, 'candidate': CANDIDATE}:
            write('Error', {'code': 'InvalidPayload: unexpected or missing fields'}, e['id'])
        elif scenario.get('answer_validate', True):
            state['view'] = 1
            ack(e, f'validation requested attempt={ATTEMPT}', validation='started')
    elif kind == 'CandidateDecision':
        if p.get('action') == 'Accept' and scenario.get('refuse_accept'):
            write('Error', {'code': scenario['refuse_accept']}, e['id'])
        else:
            state['decision'] = 'ACCEPTED' if p.get('action') == 'Accept' else 'REJECTED'
            ack(e, 'candidate accepted' if p.get('action') == 'Accept' else 'candidate rejected')
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
