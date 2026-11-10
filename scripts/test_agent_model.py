import datetime
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


class AgentModelTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="devws-model-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.scripts = self.root / "scripts"
        self.scripts.mkdir()
        source = Path(__file__).resolve().parents[1] / "config/tmuxinator"
        for name in ["agent_model.sh", "agent_runner.sh", "agent_state.sh", "agent_statusline.sh",
                     "header_tokens.sh", "agents.sh", "token_cache.pl", "agent_editor_prompt.md"]:
            shutil.copy(source / name, self.scripts / name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.state = self.root / "state.json"
        self.save({"%1": {"@devws_role": "agent"}, "global": {}, "session": {}})
        self.executable(self.bin / "tmux", '''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
path = Path(os.environ['MODEL_TEST_STATE'])
state = json.loads(path.read_text())
args = sys.argv[1:]
command = args.pop(0)
pane = args[args.index('-t') + 1] if '-t' in args else '%1'
if command in ['show-option', 'show-options']:
    scope = 'global' if any(arg.startswith('-g') for arg in args) else pane
    print(state.get(scope, {}).get(args[-1], ''))
elif command == 'display-message':
    text = args[-1].replace('#{session_name}', 'dev-test').replace('#{session_id}', 'session')
    for key in ['@devws_role', '@devws_model', '@devws_model_source', '@devws_model_seen', '@devws_model_transcript', '@devws_transcript']:
        text = text.replace('#{' + key + '}', state.get(pane, {}).get(key, ''))
    print(text)
elif command == 'set-option':
    with open(os.environ['MODEL_TEST_WRITES'], 'a') as output:
        print(' '.join(args), file=output)
    scope = 'global' if '-g' in args else pane
    state.setdefault(scope, {})
    if '-u' in args:
        state[scope].pop(args[-1], None)
    else:
        state[scope][args[-2]] = args[-1]
    path.write_text(json.dumps(state))
elif command == 'list-panes':
    for pane, options in state.items():
        if pane.startswith('%'):
            text = args[-1].replace('#{pane_id}', pane)
            for key in ['@devws_role', '@devws_transcript', '@devws_model_source']:
                text = text.replace('#{' + key + '}', options.get(key, ''))
            print(text)
else:
    raise SystemExit('unexpected tmux call: ' + command)
''')
        self.executable(self.bin / "date", '#!/usr/bin/env bash\nif [ -n "${MODEL_TEST_NOW:-}" ] && [ "$*" = +%s ]; then printf "%s\\n" "$MODEL_TEST_NOW"; else exec ' + shutil.which("date") + ' "$@"; fi\n')
        for name in ["claude", "codex"]:
            self.executable(self.bin / name, "#!/usr/bin/env bash\nexit 0\n")
        self.executable(self.scripts / "agent_status.sh", "#!/usr/bin/env bash\nexit 0\n")
        self.executable(self.scripts / "refresh_devws_pickers.sh",
                        '#!/usr/bin/env bash\nprintf "refresh\\n" >> "$MODEL_TEST_REFRESH"\n')
        self.refreshes = self.root / "refreshes"
        self.metrics = self.root / "metrics"
        self.environment = dict(os.environ, PATH=str(self.bin) + ":" + os.environ["PATH"],
                                HOME=str(self.root / "home"), TMPDIR=str(self.root), TMUX_PANE="%1",
                                MODEL_TEST_STATE=str(self.state), MODEL_TEST_REFRESH=str(self.refreshes),
                                MODEL_TEST_WRITES=str(self.root / "writes"),
                                DEVWS_TOKEN_CACHE_METRICS=str(self.metrics))

    def executable(self, path, text):
        path.write_text(text)
        path.chmod(0o755)

    def save(self, state):
        self.state.write_text(json.dumps(state))

    def options(self):
        return json.loads(self.state.read_text())["%1"]

    def run_script(self, name, *arguments, payload=None):
        return subprocess.check_output([str(self.scripts / name), *arguments], env=self.environment,
                                       input=payload, text=True).strip()

    def test_launch_aliases_are_fallbacks_and_switch_clears_observed_state(self):
        for arguments, expected in [(["--model", "opus"], "opus"), (["-m", "gpt-test"], "gpt-test"),
                                    (["--model=sonnet"], "sonnet"), ([], None)]:
            state = json.loads(self.state.read_text())
            state["%1"].update({"@devws_model": "stale", "@devws_model_source": "runtime",
                                "@devws_transcript": "/old/log", "@devws_model_configured": "stale"})
            self.save(state)
            arguments_file = self.root / "args"
            arguments_file.write_bytes(b"".join(argument.encode() + b"\0" for argument in arguments))
            self.run_script("agent_runner.sh", "codex", "--args-file", str(arguments_file))
            self.assertEqual(self.options().get("@devws_model_configured"), expected)
            self.assertNotIn("@devws_model", self.options())
            self.assertNotIn("@devws_transcript", self.options())

    def test_hook_models_are_observed_and_unchanged_models_do_not_refresh(self):
        payload = json.dumps({"hook_event_name": "PreToolUse", "model": "gpt-runtime"})
        self.run_script("agent_state.sh", payload=payload)
        self.run_script("agent_state.sh", payload=payload)
        self.assertEqual(self.options()["@devws_model"], "gpt-runtime")
        self.assertEqual(self.refreshes.read_text().splitlines(), ["refresh"])
        self.assertFalse(self.metrics.exists(), "hooks must not scan transcripts")
        self.run_script("agent_model.sh", "record", "%1", "bad\nmodel")
        self.assertEqual(self.options()["@devws_model"], "gpt-runtime")

    def test_statusline_uses_model_id_without_inventing_unknown_models(self):
        self.run_script("agent_statusline.sh", payload=json.dumps({"model": {"id": "claude-actual"}}))
        self.assertEqual(self.options()["@devws_model"], "claude-actual")
        self.run_script("agent_statusline.sh", payload=json.dumps({"model": {"display_name": "Pretty name"}}))
        self.assertEqual(self.options()["@devws_model"], "claude-actual")

    def test_transcript_backfill_is_cached_throttled_and_runtime_evidence_wins(self):
        transcript = self.root / "log.jsonl"
        transcript.write_text(json.dumps({"type": "turn_context", "payload": {"model": "gpt-observed"}}) + "\n")
        state = json.loads(self.state.read_text())
        state["%1"]["@devws_transcript"] = str(transcript)
        self.save(state)
        self.run_script("agent_model.sh", "refresh", "--force")
        self.assertEqual(self.options()["@devws_model"], "gpt-observed")
        self.run_script("agent_model.sh", "refresh", "--force")
        self.assertEqual(len(self.metrics.read_text().splitlines()), 1)
        with transcript.open("a") as output:
            output.write(json.dumps({"type": "turn_context", "payload": {"model": "gpt-new"}}) + "\n")
        self.run_script("agent_model.sh", "refresh")
        self.assertEqual(self.options()["@devws_model"], "gpt-observed", "periodic refresh is throttled")
        self.run_script("agent_model.sh", "refresh", "--force")
        self.assertEqual(self.options()["@devws_model"], "gpt-new")
        self.run_script("agent_model.sh", "record", "%1", "gpt-live")
        self.run_script("agent_model.sh", "refresh", "--force")
        self.assertEqual(self.options()["@devws_model"], "gpt-live", "lagging transcripts must not replace runtime evidence")

    def test_newer_transcript_and_changed_transcript_can_update_runtime_models(self):
        transcript = self.root / "log.jsonl"
        state = json.loads(self.state.read_text())
        state["%1"]["@devws_transcript"] = str(transcript)
        self.save(state)
        self.run_script("agent_model.sh", "record", "%1", "gpt-runtime")
        # A later turn-context is evidence of an in-session /model switch.
        future = datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(seconds=5)
        transcript.write_text(json.dumps({"type": "turn_context", "timestamp": future.isoformat().replace("+00:00", "Z"),
                                         "payload": {"model": "gpt-switched"}}) + "\n")
        self.run_script("agent_model.sh", "refresh", "--force")
        self.assertEqual(self.options()["@devws_model"], "gpt-switched")
        # Changing only evidence source must not redraw the sidebar.
        refresh_count = len(self.refreshes.read_text().splitlines())
        self.run_script("agent_model.sh", "record", "%1", "gpt-switched")
        self.assertEqual(len(self.refreshes.read_text().splitlines()), refresh_count)
        # Older panes with a new transcript pointer can recover even when
        # the new transcript contains no timestamp.
        other = self.root / "other.jsonl"
        other.write_text(json.dumps({"type": "turn_context", "payload": {"model": "gpt-new-session"}}) + "\n")
        state = json.loads(self.state.read_text())
        state["%1"]["@devws_transcript"] = str(other)
        state = {"%0": {"@devws_role": "agent", "@devws_transcript": "/missing/file"}, **state}
        self.save(state)
        self.run_script("agent_model.sh", "refresh", "--force")
        self.assertEqual(self.options()["@devws_model"], "gpt-new-session")

    def test_later_same_model_runtime_report_wins_without_redrawing(self):
        transcript = self.root / "log.jsonl"
        state = json.loads(self.state.read_text())
        state["%1"]["@devws_transcript"] = str(transcript)
        self.save(state)
        self.environment["MODEL_TEST_NOW"] = "100"
        self.run_script("agent_model.sh", "record", "%1", "model-a")
        transcript.write_text(json.dumps({"type": "turn_context", "timestamp": "1970-01-01T00:03:20Z",
                                         "payload": {"model": "model-b"}}) + "\n")
        self.environment["MODEL_TEST_NOW"] = "300"
        writes = self.root / "writes"
        prior_writes = len(writes.read_text().splitlines())
        self.run_script("agent_model.sh", "record", "%1", "model-a")
        self.assertEqual(self.options()["@devws_model_seen"], "300")
        self.assertEqual(len(writes.read_text().splitlines()), prior_writes + 1, "only observation time needs updating")
        self.run_script("agent_model.sh", "record", "%1", "model-a")
        self.assertEqual(len(writes.read_text().splitlines()), prior_writes + 1, "same-second reports need no writes")
        self.run_script("agent_model.sh", "refresh", "--force")
        self.assertEqual(self.options()["@devws_model"], "model-a", "the latest direct report must win")
        self.assertEqual(self.refreshes.read_text().splitlines(), ["refresh"], "unchanged text must not redraw")

    def test_claude_model_records_need_no_usage_and_latest_non_synthetic_wins(self):
        transcript = self.root / "claude.jsonl"
        records = [{"type": "assistant", "message": {"model": model, "content": "PRIVATE"}}
                   for model in ["claude-first", "claude-second", "<synthetic>"]]
        transcript.write_text("".join(json.dumps(record) + "\n" for record in records))
        self.assertEqual(self.run_script("header_tokens.sh", "model", str(transcript)), "claude-second")
        cached = list(self.root.glob("devws-tokens-*/*.json"))
        self.assertNotIn("PRIVATE", cached[0].read_text())


if __name__ == "__main__":
    unittest.main()
