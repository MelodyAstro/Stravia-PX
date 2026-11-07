"""Verify deployed per-Key aliases using synthetic requests and bounded capture.

Run as root on the native instance after deployment. Reads credentials from the
existing root-only access file and never prints them or response content.
"""
import copy, hashlib, http.client, importlib.util, json, pathlib, re, sqlite3, subprocess, time, urllib.request

root = pathlib.Path('/root')
module_spec = importlib.util.spec_from_file_location('client', root / 'stravia-client.py')
module = importlib.util.module_from_spec(module_spec)
module_spec.loader.exec_module(module)
client = module.Client().login()
access = json.loads((root / 'stravia-access.json').read_text())
keys = client.call('GET', '/api/v1/api-keys')['data']
private = next(k for k in keys if k['name'] == 'GPT-Private')
path = '/api/v1/api-keys/' + private['id'] + '/request-models'
initial = client.call('GET', path)['data']
saved = copy.deepcopy(initial['config'])
assert saved['enabled'] and len(saved['mappings']) == 8
assert len(initial['eligible_models']) == 8
target = next(m['target_model_id'] for m in saved['mappings'] if m['request_model'] == 'gpt-6-luna')
credential = next(g['api_key'] for g in access['groups'] if g['name'] == 'GPT-Private')
results = []

def request(method, endpoint, token=None, body=None):
    connection = http.client.HTTPConnection('127.0.0.1', 23471, timeout=180)
    headers = {'Content-Type': 'application/json'}
    if token:
        headers['Authorization'] = 'Bearer ' + token
    connection.request(method, endpoint, None if body is None else json.dumps(body), headers)
    response = connection.getresponse()
    payload = response.read()
    status = response.status
    connection.close()
    return status, payload

def record(case, **values):
    item = {'case': case, **values}
    results.append(item)
    print(json.dumps(item), flush=True)

def rejected_config(config):
    result = client.call('PUT', path, config)
    assert result.get('error'), 'Invalid mapping unexpectedly accepted'
    assert client.call('GET', path)['data']['config'] == saved

try:
    client.call('POST', path + '/capture', {'enabled': True})
    status, body = request('POST', '/v1/responses', credential, {
        'model': 'px-unconfigured', 'input': 'PX_SYNTHETIC_PRIVATE_TEXT', 'stream': False,
    })
    assert status == 404
    record('authenticated-unknown-name', status=status)
    status, _ = request('POST', '/v1/responses', None, {'model': 'px-unconfigured', 'input': 'OK'})
    assert status == 401
    record('unauthenticated-request', status=status)
    status, _ = request('GET', path, credential)
    assert status in [401, 403]
    record('inference-key-cannot-administer-mappings', status=status)

    duplicate = copy.deepcopy(saved)
    duplicate['mappings'].append(copy.deepcopy(duplicate['mappings'][0]))
    rejected_config(duplicate)
    denied = copy.deepcopy(saved)
    other = next(m for m in client.call('GET', '/api/v1/models')['data'] if m['model_id'].startswith('claude-'))
    denied['mappings'][0]['target_model_id'] = other['id']
    rejected_config(denied)
    record('invalid-and-cross-group-mapping-rejected', passed=True)

    custom = copy.deepcopy(saved)
    custom['mappings'].append({'request_model': 'px-luna', 'target_model_id': target})
    assert client.call('PUT', path, custom)['data']['config'] == custom
    started = time.monotonic()
    status, body = request('POST', '/v1/responses', credential, {
        'model': 'px-luna', 'input': 'Reply with exactly OK.', 'stream': True,
        'store': False, 'reasoning': {'effort': 'low'},
    })
    events = [json.loads(line[5:].strip()) for line in body.splitlines()
              if line.startswith(b'data:') and line[5:].strip() != b'[DONE]']
    completed = next((event['response'] for event in events if event.get('type') == 'response.completed'), None)
    assert status == 200 and completed and completed['model'] == 'px-luna'
    record('custom-alias-complete-response', status=status, response_model_preserved=True,
           total_ms=round((time.monotonic() - started) * 1000))
    client.call('PUT', path, saved)

    for host in ['masub-ex.yuhatze.org', 'masub-ex.retlinks.top']:
        report_path = root / ('stravia-request-model-responses-' + host.split('.')[0] + '-' + host.split('.')[1] + '.json')
        run = subprocess.run(['python3', str(root / 'verify-stravia-responses-streams.py'),
                              '--host', host, '--model', 'gpt-6-luna', '--report', str(report_path)],
                             capture_output=True, text=True, timeout=600)
        if run.returncode:
            # The helper only emits event types/status, never payloads or credentials.
            raise RuntimeError('Responses verification failed for ' + host + ': ' + run.stderr[-1200:])
        verification = json.loads(report_path.read_text())
        assert verification['passed'] and len(verification['results']) == 4
        record('responses-complete-streams-and-tools', host=host, passed=True,
               results=verification['results'])

    view = client.call('GET', path)['data']
    samples = view['capture']['samples']
    assert {'px-unconfigured', 'px-luna', 'gpt-6-luna'}.issubset({s['request_model'] for s in samples})
    assert {'http', 'websocket'}.issubset({s['transport'] for s in samples})
    suggestion = next(s for s in view['suggestions'] if s['request_model'] == 'gpt-6-luna')
    assert suggestion['target_model_id'] == target and suggestion['candidates'] == [target]
    allowed = {'request_model', 'protocol', 'transport', 'stream', 'input_items', 'tools', 'count'}
    assert all(set(sample) == allowed for sample in samples)
    assert 'PX_SYNTHETIC_PRIVATE_TEXT' not in json.dumps(view)
    record('bounded-capture-and-unique-suggestion', passed=True, samples=len(samples),
           http_and_websocket=True, content_not_retained=True)

    for group in access['groups']:
        matching = next(k for k in keys if k['name'] == group['name'])
        status, body = request('GET', '/v1/models', group['api_key'])
        if not matching['is_enabled']:
            assert status == 403
            record('model-list', key_name=group['name'], status=status)
            continue
        assert status == 200
        names = {m['id'] for m in json.loads(body)['data']}
        if group['name'] in ['GPT-Private', 'All']:
            assert 'gpt-6-luna' in names and not any(n.startswith('private/') for n in names)
            assert len(names) == (8 if group['name'] == 'GPT-Private' else 11)
        else:
            assert names == set(group['models'])
        record('model-list', key_name=group['name'], status=status, model_count=len(names))
    status, _ = request('POST', '/v1/responses', credential, {'model': other['model_id'], 'input': 'OK'})
    assert status == 403
    record('cross-group-inference-denied', status=status)

    pages = []
    for host in ['masub-ex.yuhatze.org', 'masub-ex.retlinks.top']:
        with urllib.request.urlopen('https://' + host + '/login', timeout=60) as response:
            html = response.read().decode()
            assert response.status == 200 and 'no-store' in response.headers.get('Cache-Control', '')
        entry = re.search(r'(/_app/immutable/entry/[^\"\s]+\.js)', html)
        assert entry
        with urllib.request.urlopen('https://' + host + entry.group(1), timeout=60) as response:
            assert response.status == 200 and 'immutable' in response.headers.get('Cache-Control', '')
        pages.append(host)
    record('webui-and-immutable-assets', passed=True, hosts=pages)
finally:
    assert client.call('PUT', path, saved)['data']['config'] == saved
    client.call('DELETE', path + '/capture')
    assert not client.call('GET', path)['data']['capture']['samples']

assert not client.call('GET', '/api/v1/observations/debug')['data']['enabled']
with sqlite3.connect('file:/var/lib/stravia/db/gateway.db?mode=ro', uri=True) as db:
    assert db.execute('PRAGMA quick_check').fetchone()[0] == 'ok'
    assert not db.execute('PRAGMA foreign_key_check').fetchall()
report = {'passed': True, 'results': results, 'capture_cleared': True, 'debug_off': True,
          'original_mapping_restored': True, 'sqlite_integrity': 'ok'}
(root / 'stravia-request-model-live-verification.json').write_text(json.dumps(report, indent=2))
print(json.dumps({'passed': True, 'cases': len(results), 'capture_cleared': True}), flush=True)
