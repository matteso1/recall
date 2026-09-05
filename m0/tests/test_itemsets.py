import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import itemsets  # noqa: E402
from ddragon import ChampionIndex, ItemIndex  # noqa: E402

FIX = Path(__file__).parent / "fixtures"
REPO = Path(__file__).resolve().parents[2]


class ItemSetTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.items = ItemIndex(json.loads((FIX / "item_subset.json").read_text()))
        cls.champs = ChampionIndex(json.loads((FIX / "champion_subset.json").read_text()))
        cls.spec = itemsets.load_spec(REPO / "data" / "itemsets" / "xayah.json")
        cls.existing = json.loads((FIX / "itemsets_existing.json").read_text())

    def test_parse_item_entry(self):
        self.assertEqual(itemsets.parse_item_entry("Health Potion x2"), ("Health Potion", 2, ""))
        self.assertEqual(itemsets.parse_item_entry("Infinity Edge"), ("Infinity Edge", 1, ""))
        self.assertEqual(itemsets.parse_item_entry({"item": "IE", "why": "crit"}), ("IE", 1, "crit"))

    def test_build_xayah_spec(self):
        s, warnings = itemsets.build_item_set(self.spec, self.items, self.champs)
        self.assertEqual(warnings, [])
        self.assertEqual(s["associatedChampions"], [498])
        self.assertEqual(s["title"], "Featherstorm Xayah")
        self.assertEqual((s["type"], s["map"], s["mode"]), ("custom", "any", "any"))
        ids = [i["id"] for b in s["blocks"] for i in b["items"]]
        self.assertTrue(all(i.isdigit() for i in ids))
        for must in ("3508", "3006", "3031", "6675", "3033", "3026"):
            self.assertIn(must, ids)
        er = s["blocks"][1]
        self.assertTrue(er["type"].startswith("1. Essence Reaver"))
        self.assertEqual([i["id"] for i in er["items"]], ["3057", "3133", "1018", "3508"])
        greaves = s["blocks"][2]
        self.assertIn({"id": "1042", "count": 2}, greaves["items"])  # Dagger x2 merged
        full = next(b for b in s["blocks"] if b["type"] == "Full build (in order)")
        self.assertEqual([i["id"] for i in full["items"]], ["3508", "3006", "3031", "6675", "3033", "3026"])
        s2, _ = itemsets.build_item_set(self.spec, self.items, self.champs)
        self.assertEqual(s["uid"], s2["uid"])

    def test_unresolved_names_are_reported(self):
        spec = json.loads(json.dumps(self.spec))
        spec["core"].append("Sword of Nonexistence")
        _, warnings = itemsets.build_item_set(spec, self.items, self.champs)
        self.assertTrue(any("Sword of Nonexistence" in w for w in warnings))

    def test_unknown_champion(self):
        with self.assertRaises(itemsets.SpecError):
            itemsets.build_item_set({"champion": "Nobody"}, self.items, self.champs)

    def test_upsert_is_idempotent_and_remove_works(self):
        s, _ = itemsets.build_item_set(self.spec, self.items, self.champs)
        p1 = itemsets.upsert(self.existing, s)
        self.assertEqual([x["title"] for x in p1["itemSets"]], ["OP.GG Xayah", "Featherstorm Xayah"])
        p2 = itemsets.upsert(p1, s)
        self.assertEqual(len(p2["itemSets"]), 2)
        p3 = itemsets.remove(p2, s["title"])
        self.assertEqual([x["title"] for x in p3["itemSets"]], ["OP.GG Xayah"])
        self.assertEqual(len(self.existing["itemSets"]), 1)  # inputs never mutated
        self.assertGreater(p1["timestamp"], 0)


if __name__ == "__main__":
    unittest.main()
