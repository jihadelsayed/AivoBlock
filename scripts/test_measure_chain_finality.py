import unittest
from unittest.mock import patch
from measure_chain_finality import measure_launch


class StubNet:
    out = __import__("pathlib").Path("/tmp/research/launch-001/three-node")
    def __init__(self):
        self.height = 0
        self.report = {}
        self.saved = 0
    def nodes(self):
        return ["node-1", "node-2", "node-3"]
    def up(self):
        pass
    def status(self, node):
        return {"height": self.height}
    def submit(self, *args):
        return {"sequence": self.height + 1, "task_id": f"task-{self.height + 1}", "rows": 12}
    def wait_height(self, target, nodes, interval=1.0):
        self.height = target
        self.last_interval = interval
    def verify(self, nodes):
        return {"height": self.height, "blocks": [
            {"height": h, "workload": "qmlp-v1", "block_id": f"block-{h}"}
            for h in range(1, self.height + 1)]}
    def save_report(self):
        self.saved += 1


class RepeatedFinalityTests(unittest.TestCase):
    def test_measure_two_real_boundary_samples(self):
        n = StubNet()
        with patch("measure_chain_finality.time.perf_counter_ns", side_effect=[10, 400010, 1000010, 20, 600020, 2000020]):
            rows = measure_launch(n, samples=2, rows=12)
        self.assertEqual(len(rows), 2)
        self.assertEqual([x["finalized_height"] for x in rows], [1, 2])
        self.assertEqual([x["request_id"] for x in rows], ["launch-001-0", "launch-001-1"])
        self.assertEqual([x["elapsed_ms"] for x in rows], [1.0, 2.0])
        self.assertEqual([x["timing_phases"]["cli_submission_ms"] for x in rows], [0.4, 0.6])
        self.assertEqual([x["timing_phases"]["post_submission_to_observed_finality_ms"] for x in rows], [0.6, 1.4])
        for item in rows:
            self.assertEqual(item["timing_phases"]["total_ms"], item["elapsed_ms"])
            self.assertEqual(item["timing_phases"]["clock"], item["clock"])
            self.assertIsNone(item["timing_phases"]["ai_execution_only_ms"])
            self.assertIsNone(item["timing_phases"]["consensus_only_ms"])
        self.assertEqual(n.saved, 2)
        self.assertEqual(n.last_interval, 1.0)

    def test_configurable_poll_interval(self):
        n = StubNet()
        with patch("measure_chain_finality.time.perf_counter_ns", side_effect=[10, 250010, 1000010]):
            rows = measure_launch(n, samples=1, rows=12, polling_interval_s=0.1)
        self.assertEqual(n.last_interval, 0.1)
        self.assertEqual(rows[0]["polling_interval_s"], 0.1)

    def test_invalid_poll_interval(self):
        with self.assertRaises(ValueError):
            measure_launch(StubNet(), samples=1, rows=12, polling_interval_s=0)

    def test_reject_invalid_sample_count(self):
        with self.assertRaises(ValueError):
            measure_launch(StubNet(), samples=0, rows=12)

    def test_reject_missing_finalized_qmlp(self):
        n = StubNet()
        n.verify = lambda nodes: {"height": n.height, "blocks": []}
        with self.assertRaisesRegex(ValueError, "not included"):
            measure_launch(n, samples=1, rows=12)


if __name__ == "__main__":
    unittest.main()
