"""Test assembly using explicitly synthetic fixtures only."""
import copy
import unittest

from assemble_service_chain import assemble
from test_validate_service_chain import fixture


class AssemblyTests(unittest.TestCase):
    def fragments(self):
        return copy.deepcopy(fixture()["scenarios"])

    def test_three_scenarios_validate(self):
        bundle, result = assemble(self.fragments())
        self.assertEqual(result["status"], "PASS")
        self.assertEqual(set(bundle["scenarios"]), set(self.fragments()))

    def test_missing_scenario_rejected(self):
        fragments = self.fragments()
        del fragments["chain_observed_finality"]
        with self.assertRaisesRegex(ValueError, "exactly three"):
            assemble(fragments)

    def test_extra_scenario_rejected(self):
        fragments = self.fragments()
        fragments["unknown"] = {}
        with self.assertRaisesRegex(ValueError, "exactly three"):
            assemble(fragments)

    def test_cross_scenario_commitment_mismatch_rejected(self):
        fragments = self.fragments()
        fragments["chain_observed_finality"]["canonical_input_commitment"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "canonical"):
            assemble(fragments)

    def test_insufficient_samples_rejected(self):
        fragments = self.fragments()
        fragments["service_direct"]["samples"].pop()
        with self.assertRaisesRegex(ValueError, "measured samples"):
            assemble(fragments)

    def test_unfinalized_chain_rejected(self):
        fragments = self.fragments()
        del fragments["chain_observed_finality"]["samples"][0]["finalized_height"]
        with self.assertRaisesRegex(ValueError, "finalized height"):
            assemble(fragments)


if __name__ == "__main__":
    unittest.main()
