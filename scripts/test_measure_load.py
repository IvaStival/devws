import unittest
from unittest.mock import call, patch
import signal

from measure_load import cpu_groups, distribution, hooks, rendering, tokens


class MeasurementTests(unittest.TestCase):
    def test_distributions(self):
        self.assertEqual(distribution([4, 1, 2, 3, 5]),
                         {"median": 3, "p95": 5, "max": 5, "samples": 5})

    def test_cpu_normalization_excludes_new_and_exited_processes(self):
        result = cpu_groups({1: (2, 10, "tmux"), 2: (9, 10, "gone")},
                            {1: (3, 1024, "tmux"), 3: (8, 10, "new")}, 2)
        self.assertEqual(result, [{"command": "tmux", "cpu_percent_one_core": 50,
                                  "resident_mb": 1, "processes": 1}])

    def test_fixture_runs_without_real_tmux_or_agents(self):
        result = hooks(1, 2)
        self.assertEqual(len(result["results"]), 6)
        self.assertEqual([row["agents"] for row in result["results"]], [1, 2, 4, 1, 2, 4])
        self.assertTrue(all(row["pair_wall_ms"]["samples"] == 1 for row in result["results"]))
        for row in result["results"]:
            self.assertEqual(row["repository_scans"], [2 * row["agents"] if row["linked_editor"] else 0])

    def test_ticker_is_resumed_when_sampling_fails(self):
        with patch("measure_load.subprocess.check_output", side_effect=["123", "header_ticker.sh --run"]), \
                patch("measure_load.os.kill") as kill, \
                patch("measure_load.live", side_effect=OSError("sampling failed")):
            with self.assertRaises(OSError):
                rendering(1, 1)
            self.assertEqual(kill.call_args_list[-1], call(123, signal.SIGCONT))

    def test_token_fixture_counts_source_reads(self):
        result = tokens(1, 0.01)
        self.assertEqual([row["source_parses"] for row in result["results"]], [1, 0, 1])


if __name__ == "__main__":
    unittest.main()
