#!/usr/bin/env python3
"""Optional real OpenCode 1.19.6 probe using only a localhost fake provider.

Run with `python3 scripts/probe_opencode_reviewed.py`. Requires OpenCode and
its bundled OpenAI-compatible provider adapter; never uses account credentials.
Artifacts remain in the printed temporary directory for inspection.
"""
import tempfile, pathlib, os, subprocess, json, threading, http.server
root = pathlib.Path(tempfile.mkdtemp(prefix='devws-opencode-probe-'))
(root / 'mcp.py').write_text('import sys,json\nfor line in sys.stdin:\n try:\n  q=json.loads(line); method=q.get("method"); result={}\n  if method=="initialize": result={"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}\n  elif method=="tools/list": result={"tools":[{"name":"worker","description":"Scoped worker fixture","inputSchema":{"type":"object","properties":{"action":{"type":"string"}},"required":["action"]}}]}\n  elif method=="tools/call": result={"content":[{"type":"text","text":"worker fixture passed"}]}\n  if "id" in q: print(json.dumps({"jsonrpc":"2.0","id":q["id"],"result":result}),flush=True)\n except Exception as error: print(str(error), file=sys.stderr); raise\n')
requests = []
assert subprocess.check_output(['opencode', '--version'], text=True).strip() == '1.19.6'

class Handler(http.server.BaseHTTPRequestHandler):

    def log_message(self, *arguments):
        """Keep fixture HTTP logging out of probe output."""

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        requests.append(body)
        result = {'id': 'probe', 'object': 'chat.completion', 'created': 0, 'model': 'fixture', 'choices': [{'index': 0, 'message': {'role': 'assistant', 'content': 'Probe done.'}, 'finish_reason': 'stop'}], 'usage': {'prompt_tokens': 1, 'completion_tokens': 1, 'total_tokens': 2}}
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream' if body.get('stream') else 'application/json')
        self.end_headers()
        if body.get('stream'):
            call = bool(body.get('tools')) and (not any((message.get('role') == 'tool' for message in body.get('messages', []))))
            chunks = [({'role': 'assistant', 'tool_calls': [{'index': 0, 'id': 'call_fixture', 'type': 'function', 'function': {'name': 'devagentics_worker_worker', 'arguments': '{"action":"status"}'}}]}, None), ({}, 'tool_calls')] if call else [({'role': 'assistant', 'content': 'Probe done.'}, None), ({}, 'stop')]
            for delta, finish in chunks:
                self.wfile.write(('data: ' + json.dumps({'id': 'probe', 'object': 'chat.completion.chunk', 'created': 0, 'model': 'fixture', 'choices': [{'index': 0, 'delta': delta, 'finish_reason': finish}]}) + '\n\n').encode())
            self.wfile.write(b'data: [DONE]\n\n')
        else:
            self.wfile.write(json.dumps(result).encode())
server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
configuration = {'plugin': [], 'instructions': [], 'autoupdate': False, 'share': 'disabled', 'permission': {'*': 'deny', 'devagentics_worker_worker': 'allow'}, 'agent': {'devagentics-worker': {'mode': 'primary', 'description': 'Managed worker', 'permission': {'*': 'deny', 'devagentics_worker_worker': 'allow'}}}, 'mcp': {'devagentics_worker': {'type': 'local', 'enabled': True, 'command': ['python3', str(root / 'mcp.py')]}}, 'provider': {'fixture': {'npm': '@ai-sdk/openai-compatible', 'options': {'baseURL': f'http://127.0.0.1:{server.server_port}/v1', 'apiKey': 'fixture'}, 'models': {'fixture': {'name': 'fixture', 'limit': {'context': 32000, 'output': 1000}}}}}}
environment = {name: value for name, value in os.environ.items() if not name.startswith('OPENCODE_')}
for key, subdirectory in [('HOME', 'home'), ('OPENCODE_TEST_HOME', 'home'), ('XDG_CONFIG_HOME', 'config'), ('XDG_STATE_HOME', 'state'), ('XDG_CACHE_HOME', 'cache'), ('XDG_DATA_HOME', 'data'), ('OPENCODE_CONFIG_DIR', 'config/opencode'), ('OPENCODE_TEST_MANAGED_CONFIG_DIR', 'managed')]:
    (root / subdirectory).mkdir(parents=True, exist_ok=True)
    environment[key] = str(root / subdirectory)
for key in ['OPENCODE_DISABLE_PROJECT_CONFIG', 'OPENCODE_DISABLE_CLAUDE_CODE', 'OPENCODE_DISABLE_CLAUDE_CODE_PROMPT', 'OPENCODE_DISABLE_CLAUDE_CODE_SKILLS', 'OPENCODE_DISABLE_EXTERNAL_SKILLS', 'OPENCODE_DISABLE_DEFAULT_PLUGINS', 'OPENCODE_DISABLE_MODELS_FETCH', 'OPENCODE_DISABLE_LSP_DOWNLOAD', 'OPENCODE_PURE']:
    environment[key] = 'true'
hostile = {'plugin': ['file:///nonexistent-hostile-plugin.js'], 'mcp': {'hostile': {'type': 'local', 'command': ['false']}}, 'instructions': ['HOSTILE.md']}
(root / 'opencode.json').write_text(json.dumps(hostile))
(root / 'HOSTILE.md').write_text('HOSTILE_INSTRUCTION_SENTINEL')
(root / 'home/.claude').mkdir()
(root / 'home/.claude/CLAUDE.md').write_text('HOSTILE_INSTRUCTION_SENTINEL')
environment['OPENCODE_CONFIG_CONTENT'] = json.dumps(configuration)
for arguments in [['debug', 'config', '--pure'], ['debug', 'agent', 'devagentics-worker', '--pure'], ['run', '--pure', '--format', 'json', '--agent', 'devagentics-worker', '--model', 'fixture/fixture', '--', 'Call worker status then finish.']]:
    try:
        result = subprocess.run(['opencode'] + arguments, cwd=root, env=environment, capture_output=True, text=True, timeout=45)
        (root / ('result-' + arguments[0] + '.json')).write_text(result.stdout)
        assert result.returncode == 0, f'{arguments[0]} failed: {result.stderr[-500:]}'
        if arguments[:2] == ['debug', 'agent']:
            assert all((value is False for value in json.loads(result.stdout)['tools'].values()))
        if arguments[:2] == ['debug', 'config']:
            resolved = json.loads(result.stdout)
            assert set(resolved['mcp']) == {'devagentics_worker'}
            assert resolved['plugin'] == [] and resolved['instructions'] == []
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(f'OpenCode probe timed out: {arguments}') from error
events = [json.loads(line) for line in (root / 'result-run.json').read_text().splitlines() if line.startswith('{')]
session = events[0]['sessionID']
result = subprocess.run(['opencode', 'run', '--pure', '--format', 'json', '--agent', 'devagentics-worker', '--model', 'fixture/fixture', '--session', session, '--', 'Confirm previous worker status and finish.'], cwd=root, env=environment, capture_output=True, text=True, timeout=45)
(root / 'resume.json').write_text(result.stdout)
assert result.returncode == 0
assert result.stdout.strip(), 'Resume produced no events'
assert all((json.loads(line).get('sessionID', session) == session for line in result.stdout.splitlines() if line.startswith('{')))
assert any((event.get('type') == 'tool_use' and event['part']['state']['status'] == 'completed' for event in events))
assert any((message.get('role') == 'tool' and message.get('content') == 'worker fixture passed' for request in requests for message in request.get('messages', [])))
assert all(('HOSTILE_INSTRUCTION_SENTINEL' not in json.dumps(request) for request in requests))
(root / 'requests.json').write_text(json.dumps(requests, indent=2))
for request in requests:
    tools = [tool.get('function', {}).get('name') for tool in request.get('tools', [])]
    assert tools in ([], ['devagentics_worker_worker']), tools
assert any((request.get('tools') for request in requests))
server.shutdown()
print('OpenCode 1.19.6 probe passed: native tools disabled, worker MCP invoked, exact session resumed, hostile project config ignored.')
print(f'Artifacts: {root}')
