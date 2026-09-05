import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from ddragon import ChampionIndex, ItemIndex, normalize_name  # noqa: E402

FIX = Path(__file__).parent / "fixtures"


class ItemIndexTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.items = ItemIndex(json.loads((FIX / "item_subset.json").read_text()))

    def test_name_normalization(self):
        self.assertEqual(normalize_name("B. F. Sword"), "bfsword")
        self.assertEqual(normalize_name("B.F. Sword"), normalize_name("bf sword"))
        self.assertEqual(self.items.id_for("B.F. Sword"), "1038")
        self.assertEqual(self.items.id_for("dorans blade"), "1055")
        self.assertIsNone(self.items.id_for("Sword of Nonexistence"))

    def test_duplicate_names_prefer_real_shop_item(self):
        self.assertEqual(self.items.id_for("The Collector"), "6676")

    def test_components(self):
        self.assertEqual(self.items.components("3031"), ["1038", "1037", "1018"])
        self.assertEqual(self.items.components("3508"), ["3057", "3133", "1018"])
        leaves = self.items.leaf_components("3033")  # Mortal Reminder
        self.assertIn("1036", leaves)  # Long Sword via Executioner's / Last Whisper
        self.assertNotIn("3123", leaves)  # intermediates expanded away
        self.assertEqual(self.items.cost("3031"), 3500)
        self.assertTrue(self.items.purchasable_on_sr("3031"))
        self.assertFalse(self.items.purchasable_on_sr("999999"))

    def test_names(self):
        self.assertEqual(self.items.name(6675), "Navori Flickerblade")
        self.assertEqual(self.items.name("nope"), "item nope")


class ChampionIndexTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.champs = ChampionIndex(json.loads((FIX / "champion_subset.json").read_text()))

    def test_lookups(self):
        self.assertEqual(self.champs.key_for("Xayah"), 498)
        self.assertEqual(self.champs.key_for("Wukong"), 62)
        self.assertEqual(self.champs.key_for("MonkeyKing"), 62)
        self.assertEqual(self.champs.name(16), "Soraka")
        self.assertEqual(self.champs.name(0), "-")
        self.assertEqual(self.champs.name(99999), "champ 99999")


if __name__ == "__main__":
    unittest.main()
