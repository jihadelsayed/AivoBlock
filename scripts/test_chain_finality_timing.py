"""Validate real-clock finality observation boundary serialization (no network run)."""
import unittest

from testnet import observed_finality_timing, submission_observation_timing


class FinalityClockTests(unittest.TestCase):
    def test_positive_monotonic_interval(self):
        item = observed_finality_timing(1_000_000, 3_500_000)
        self.assertEqual(item["elapsed_ms"], 2.5)
        self.assertEqual(item["start_ns"], 1_000_000)
        self.assertEqual(item["end_ns"], 3_500_000)
        self.assertEqual(item["clock"], "time.perf_counter_ns")

    def test_client_phase_split_preserves_total_and_unknown_components(self):
        phases = submission_observation_timing(1_000_000, 3_000_000, 9_000_000)
        self.assertEqual(phases["cli_submission_ms"], 2)
        self.assertEqual(phases["post_submission_to_observed_finality_ms"], 6)
        self.assertEqual(phases["total_ms"], 8)
        self.assertIsNone(phases["consensus_only_ms"])
        self.assertIsNone(phases["ai_execution_only_ms"])

    def test_rejects_invalid_phase_ordering(self):
        for times in ((1, 1, 3), (1, 4, 4), (3, 2, 4), (-1, 2, 3), (True, 2, 3)):
            with self.subTest(times=times), self.assertRaisesRegex(ValueError, "invalid monotonic"):
                submission_observation_timing(*times)

    def test_rejects_invalid_intervals(self):
        for a, b in ((1, 1), (3, 2), (-1, 4), (True, 4), (0, False), (1.1, 4)):
            with self.subTest(start=a, end=b):
                with self.assertRaisesRegex(ValueError, "invalid monotonic"):
                    observed_finality_timing(a, b)


if __name__ == "__main__":
    unittest.main()
