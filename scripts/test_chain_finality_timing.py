"""Validate real-clock finality observation boundary serialization (no network run)."""
import unittest

from testnet import observed_finality_timing


class FinalityClockTests(unittest.TestCase):
    def test_positive_monotonic_interval(self):
        item = observed_finality_timing(1_000_000, 3_500_000)
        self.assertEqual(item["elapsed_ms"], 2.5)
        self.assertEqual(item["start_ns"], 1_000_000)
        self.assertEqual(item["end_ns"], 3_500_000)
        self.assertEqual(item["clock"], "time.perf_counter_ns")

    def test_rejects_invalid_intervals(self):
        for a, b in ((1, 1), (3, 2), (-1, 4), (True, 4), (0, False), (1.1, 4)):
            with self.subTest(start=a, end=b):
                with self.assertRaisesRegex(ValueError, "invalid monotonic"):
                    observed_finality_timing(a, b)


if __name__ == "__main__":
    unittest.main()
