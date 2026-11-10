import concurrent.futures
import datetime
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


class TokenCacheTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="devws-token-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.scripts = self.root / "scripts"
        self.scripts.mkdir()
        source = Path(__file__).resolve().parents[1] / "config/tmuxinator"
        for filename in ["header_tokens.sh", "agents.sh", "agent_prices.tsv", "token_cache.pl"]:
            if (source / filename).exists():
                shutil.copy(source / filename, self.scripts / filename)
        self.now = int(datetime.datetime.now().timestamp())
        self.timestamp = datetime.datetime.fromtimestamp(self.now, datetime.timezone.utc).isoformat().replace("+00:00", "Z")
        self.log = self.root / "home/.codex/sessions/one.jsonl"
        self.log.parent.mkdir(parents=True)
        self.metrics = self.root / "metrics"
        self.environment = dict(os.environ, HOME=str(self.root / "home"), TMPDIR=str(self.root),
                                DEVWS_TOKEN_CACHE_METRICS=str(self.metrics), DEVWS_AGENTS="codex",
                                DEVWS_TOKENS_NOW=str(self.now))
        self.write([self.context(), self.tokens()])

    def context(self):
        return {"type": "turn_context", "timestamp": self.timestamp,
                "payload": {"turn_id": "one", "model": "gpt-5.6-sol", "prompt": "PRIVATE"}}

    def tokens(self, total=1000, percent=37):
        return {"type": "event_msg", "timestamp": self.timestamp,
                "payload": {"type": "token_count", "info": {
                    "total_token_usage": {"total_tokens": total, "input_tokens": 100,
                                          "output_tokens": 5, "cached_input_tokens": 20},
                    "last_token_usage": {"input_tokens": 100}, "model_context_window": 10000},
                    "rate_limits": {"primary": {"used_percent": percent, "resets_at": self.now + 60},
                                    "secondary": {"used_percent": 12, "resets_at": self.now + 120}}}}

    def write(self, records):
        self.log.write_text("".join(json.dumps(record) + "\n" for record in records))

    def run_tokens(self, mode="session", extra_environment=None):
        arguments = [str(self.scripts / "header_tokens.sh"), mode]
        if mode in ["session", "usage"]:
            arguments.append(str(self.log))
        elif mode == "limits":
            arguments.append("codex")
        return subprocess.check_output(arguments, env=self.environment | (extra_environment or {}), text=True).strip()

    def parses(self):
        return self.metrics.read_text().splitlines() if self.metrics.exists() else []

    def test_all_modes_reuse_compact_records(self):
        self.assertEqual(self.run_tokens(), "conv 1k · ctx 1%")
        self.assertEqual(self.run_tokens("usage"), "80 5 20 0 0")
        self.assertEqual(self.run_tokens("limits"), "37 12")
        self.assertEqual(self.run_tokens("costs"), "codex 0 0")
        self.assertEqual(len(self.parses()), 1, "unchanged source must be parsed once across modes")
        cached = list(self.root.glob("devws-tokens-*/*.json"))
        self.assertTrue(cached)
        self.assertNotIn("PRIVATE", cached[0].read_text())
        self.assertEqual(cached[0].stat().st_mode & 0o777, 0o600)
        self.assertEqual(cached[0].parent.stat().st_mode & 0o777, 0o700)

    def test_append_and_incomplete_tail_completion(self):
        self.run_tokens()
        record = json.dumps(self.tokens(total=2000))
        with self.log.open("a") as output:
            output.write(record[:30])
        self.assertEqual(self.run_tokens(), "conv 1k · ctx 1%")
        with self.log.open("a") as output:
            output.write(record[30:] + "\n")
        self.assertEqual(self.run_tokens(), "conv 2k · ctx 1%")
        self.assertEqual(len(self.parses()), 3)

    def test_truncation_replacement_and_same_size_rewrite(self):
        self.run_tokens()
        previous = self.log.stat()
        self.write([self.context(), self.tokens(total=2000)])
        os.utime(self.log, ns=(previous.st_atime_ns, previous.st_mtime_ns))
        self.assertEqual(self.run_tokens(), "conv 2k · ctx 1%")
        replacement = self.log.with_suffix(".replacement")
        replacement.write_text(json.dumps(self.tokens(total=3000)) + "\n")
        replacement.replace(self.log)
        self.assertEqual(self.run_tokens(), "conv 3k · ctx 1%")
        self.log.write_text("")
        self.assertEqual(self.run_tokens(), "")
        self.assertEqual(len(self.parses()), 4)

    def test_limits_reset_without_source_change(self):
        self.assertEqual(self.run_tokens("limits"), "37 12")
        self.assertEqual(self.run_tokens("limits", {"DEVWS_TOKENS_NOW": str(self.now + 61)}), "0 12")
        self.assertEqual(len(self.parses()), 1)

    def test_limits_do_not_require_a_token_count_record(self):
        self.write([{"type": "event_msg", "timestamp": self.timestamp, "payload": {
            "type": "rate_limit_update", "rate_limits": {
                "primary": {"used_percent": 77, "resets_at": self.now + 120}}}}])
        self.assertEqual(self.run_tokens("limits"), "77 -")

    def test_only_changed_log_reparses_and_deleted_log_is_removed(self):
        other = self.log.with_name("two.jsonl")
        other.write_text(json.dumps(self.tokens(percent=55)) + "\n")
        self.run_tokens("limits")
        self.assertEqual(len(self.parses()), 2)
        other.write_text(json.dumps(self.tokens(percent=66)) + "\n")
        self.run_tokens("limits")
        self.assertEqual(len(self.parses()), 3)
        other.unlink()
        self.assertEqual(self.run_tokens("limits"), "37 12")
        self.assertEqual(len(self.parses()), 3)

    def test_concurrent_readers_and_corrupt_cache_recover(self):
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as workers:
            outputs = list(workers.map(lambda _: self.run_tokens(), range(8)))
        self.assertEqual(set(outputs), {"conv 1k · ctx 1%"})
        cached = list(self.root.glob("devws-tokens-*/*.json"))
        self.assertTrue(cached)
        cached[0].write_text("broken")
        self.assertEqual(self.run_tokens(), "conv 1k · ctx 1%")

    def test_cache_failure_falls_back_to_source(self):
        blocked = self.root / "blocked"
        blocked.write_text("not a directory")
        self.assertEqual(self.run_tokens(extra_environment={"TMPDIR": str(blocked)}), "conv 1k · ctx 1%")

    def test_cache_cleanup_handles_spaces_without_touching_other_files(self):
        temporary = self.root / "cache root"
        temporary.mkdir()
        sentinel = self.root / "cache"
        sentinel.write_text("keep")
        os.utime(sentinel, (self.now - 60 * 86400, self.now - 60 * 86400))
        self.assertEqual(self.run_tokens(extra_environment={"TMPDIR": str(temporary)}), "conv 1k · ctx 1%")
        self.assertTrue(sentinel.exists(), "cache pruning must stay inside its directory")

    def test_claude_dedup_month_boundary_and_price_changes(self):
        folder = self.root / "home/.claude/projects/fixture"
        folder.mkdir(parents=True)
        message = {"type": "assistant", "timestamp": self.timestamp, "message": {
            "id": "same", "model": "claude-opus-5-5", "content": "PRIVATE",
            "usage": {"input_tokens": 1000000, "output_tokens": 0}}}
        for filename in ["one.jsonl", "two.jsonl"]:
            (folder / filename).write_text(json.dumps(message) + "\n")
        environment = {"DEVWS_AGENTS": "claude"}
        original = self.run_tokens("costs", environment)
        self.assertEqual(original, "claude 4 0")
        next_month = (datetime.datetime.now().replace(day=28) + datetime.timedelta(days=5)).replace(day=1)
        self.assertEqual(self.run_tokens("costs", environment | {"DEVWS_TOKENS_NOW": str(int(next_month.timestamp()))}), "claude 0 0")
        prices = self.scripts / "agent_prices.tsv"
        rows = prices.read_text().splitlines()
        prices.write_text("\n".join("claude-opus-5-5\t8\t10\t16\t1\t40" if row.startswith("claude-opus-5-5\t") else row for row in rows) + "\n")
        self.assertEqual(self.run_tokens("costs", environment), "claude 8 0")
        self.assertEqual(len(self.parses()), 2)


if __name__ == "__main__":
    unittest.main()
