import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from liveclient import diff, fmt_time, heartbeat, summarize  # noqa: E402

FIX = Path(__file__).parent / "fixtures"


class LiveClientTests(unittest.TestCase):
    def setUp(self):
        self.data = json.loads((FIX / "allgamedata.json").read_text())

    def test_summarize(self):
        s = summarize(self.data)
        self.assertEqual(s.mode, "CLASSIC")
        self.assertEqual(s.me.champion, "Xayah")
        self.assertEqual(s.me.position, "BOTTOM")
        self.assertEqual(s.me.gold, 500.0)
        self.assertEqual(list(s.me.abilities.items()), [("Q", 1), ("W", 0), ("E", 0), ("R", 0)])
        self.assertEqual([p.champion for p in s.enemies], ["Tristana", "Soraka", "Malphite", "Ornn", "Thresh"])
        self.assertEqual(len(s.allies), 4)
        self.assertEqual(s.me.item_names(), ["Doran's Blade", "Health Potion", "Stealth Ward"])
        self.assertEqual(fmt_time(s.game_time), "01:05")
        self.assertIn("Xayah lvl 1 | 500g", heartbeat(s))

    def test_first_diff_describes_game(self):
        lines = diff(None, summarize(self.data))
        self.assertIn("game detected: CLASSIC at 01:05, you are Xayah (BOTTOM)", lines[0])
        self.assertIn("enemies: Tristana, Soraka, Malphite, Ornn, Thresh", "\n".join(lines))

    def test_incremental_diff(self):
        d2 = json.loads(json.dumps(self.data))
        me = next(p for p in d2["allPlayers"] if p["riotId"] == "matteso#NA1")
        me["items"] = [i for i in me["items"] if i["itemID"] != 2003] + [
            {"canUse": False, "consumable": False, "count": 1, "displayName": "Pickaxe", "itemID": 1037,
             "price": 875, "rawDescription": "", "rawDisplayName": "", "slot": 1}]
        me["level"] = 2
        d2["activePlayer"]["level"] = 2
        d2["activePlayer"]["abilities"]["E"]["abilityLevel"] = 1
        d2["activePlayer"]["currentGold"] = 12.0
        soraka = next(p for p in d2["allPlayers"] if p["championName"] == "Soraka")
        soraka["items"].append({"canUse": False, "consumable": False, "count": 1, "displayName": "Executioner's Calling",
                                "itemID": 3123, "price": 800, "rawDescription": "", "rawDisplayName": "", "slot": 2})
        s1, s2 = summarize(self.data), summarize(d2)
        text = "\n".join(diff(s1, s2))
        self.assertIn("LEVEL 1 -> 2", text)
        self.assertIn("you + Pickaxe", text)
        self.assertIn("you - Health Potion", text)
        self.assertIn("enemy Soraka + Executioner's Calling", text)
        self.assertEqual(diff(s2, s2), [])

    def test_skill_point_without_level(self):
        d2 = json.loads(json.dumps(self.data))
        d2["activePlayer"]["abilities"]["W"]["abilityLevel"] = 1
        text = "\n".join(diff(summarize(self.data), summarize(d2)))
        self.assertIn("skilled W", text)


class PracticeToolCaptureTests(unittest.TestCase):
    """Real allgamedata captured from a Practice Tool game on 2026-09-05 (patch 16.17)."""

    def test_real_capture(self):
        s = summarize(json.loads((FIX / "allgamedata_practicetool.json").read_text()))
        self.assertEqual(s.mode, "PRACTICETOOL")
        self.assertEqual(s.me.champion, "Xayah")
        self.assertEqual(s.me.position, "")  # the API says "NONE" here
        self.assertEqual(s.me.item_names(), ["Total Biscuit of Everlasting Will"])
        self.assertEqual(list(s.me.abilities.items()), [("Q", 1), ("W", 0), ("E", 0), ("R", 0)])
        self.assertAlmostEqual(s.me.gold, 613.2, places=0)
        self.assertEqual((s.allies, s.enemies), ([], []))
        self.assertIn("you are Xayah (no lane)", diff(None, s)[0])


if __name__ == "__main__":
    unittest.main()
