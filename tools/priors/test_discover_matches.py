import json
from pathlib import Path
import tempfile
import unittest

from discover_matches import discover


class DiscoveryContracts(unittest.TestCase):
    def test_seed_output_cannot_overwrite_its_provenance_sidecar(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(ValueError, "\\.txt"):
                discover(None, None, Path(directory) / "seeds.json", start_time=1)

    def test_ranked_seed_cohort_deduplicates_matches_and_keeps_private_provenance(self):
        class Ladder:
            def get(self, path):
                tier = path.split("/")[4]
                return {"entries": [{"puuid": tier + str(i), "leaguePoints": i} for i in range(3)]}

        class History:
            def __init__(self):
                self.calls = []

            def get(self, path):
                self.calls.append(path)
                return ["EUW1_1", f"EUW1_{len(self.calls) + 1}"]

        history = History()
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "seeds.txt"
            result = discover(Ladder(), history, output, players=6, count=20, start_time=1)
            self.assertEqual(result["players"], 6)
            self.assertEqual(result["matches"], 7)
            self.assertEqual(len(set(output.read_text().splitlines())), 7)
            self.assertTrue(all("queue=420" in p and "count=20" in p and "startTime=1" in p
                                for p in history.calls))
            metadata = json.loads(output.with_suffix(".json").read_text())
            self.assertEqual(len(metadata["cohort"]), 6)
            self.assertEqual({p["tier"] for p in metadata["cohort"]},
                             {"master", "grandmaster", "challenger"})


if __name__ == "__main__":
    unittest.main()
