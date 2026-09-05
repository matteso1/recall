import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import champselect  # noqa: E402
from ddragon import ChampionIndex  # noqa: E402

FIX = Path(__file__).parent / "fixtures"


class ChampSelectTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.session = json.loads((FIX / "champselect_session.json").read_text())
        cls.champs = ChampionIndex(json.loads((FIX / "champion_subset.json").read_text()))

    def test_extract_me(self):
        st = champselect.extract(self.session)
        self.assertEqual(st.my_cell, 3)
        self.assertTrue(st.me.is_me)
        self.assertEqual(st.me.position, "bottom")
        self.assertEqual(st.me.champion_id, 498)
        self.assertTrue(st.me.locked)
        self.assertEqual(st.phase, "BAN_PICK")

    def test_locks_and_hovers(self):
        st = champselect.extract(self.session)
        self.assertEqual(st.locked(ally=True), [75, 62, 498, 497])
        self.assertEqual(st.locked(ally=False), [18, 16, 54, 516])
        mid = st.cells[2]
        self.assertFalse(mid.locked)
        self.assertEqual(mid.shown_champion, 238)
        self.assertFalse(st.all_locked)

    def test_initial_diff_lists_everything(self):
        text = "\n".join(champselect.diff(None, champselect.extract(self.session), self.champs))
        self.assertIn("you are cell 3, bot", text)
        self.assertIn("[LOCK]  ally bot (you): Xayah", text)
        self.assertIn("[LOCK]  enemy cell 6: Soraka", text)
        self.assertIn("[HOVER] ally mid: Zed", text)
        self.assertIn("[BAN]   ally banned Yuumi", text)
        self.assertIn("[BAN]   enemy banned Lulu", text)

    def test_incremental_diff_and_summary(self):
        s2 = json.loads(json.dumps(self.session))
        s2["myTeam"][2]["championId"] = 238
        s2["actions"].append([{"actorCellId": 2, "championId": 238, "completed": True, "id": 99,
                               "isAllyAction": True, "isInProgress": False, "pickTurn": 9, "type": "pick"}])
        s2["theirTeam"][4]["championId"] = 412
        s2["timer"]["phase"] = "FINALIZATION"
        s2["bans"]["theirTeamBans"].append(89)
        prev, cur = champselect.extract(self.session), champselect.extract(s2)
        text = "\n".join(champselect.diff(prev, cur, self.champs))
        self.assertIn("[PHASE] BAN_PICK -> FINALIZATION", text)
        self.assertIn("[LOCK]  ally mid: Zed", text)
        self.assertIn("[LOCK]  enemy cell 9: Thresh", text)
        self.assertIn("[BAN]   enemy banned Leona", text)
        self.assertTrue(cur.all_locked)
        self.assertIn("ALLY : Nasus (top), Wukong (jungle), Zed (mid), Xayah (bot) *you*, Rakan (support)", text)
        self.assertIn("ENEMY: Tristana, Soraka, Malphite, Ornn, Thresh", text)
        self.assertEqual(champselect.diff(cur, cur, self.champs), [])

    def test_without_ddragon_shows_ids(self):
        text = "\n".join(champselect.diff(None, champselect.extract(self.session), None))
        self.assertIn("enemy cell 6: 16", text)


class PracticeToolCaptureTests(unittest.TestCase):
    """Real payloads captured from the client on 2026-09-05 (Practice Tool, patch 16.17)."""

    @classmethod
    def setUpClass(cls):
        cls.hover = json.loads((FIX / "champselect_practicetool_hover.json").read_text())
        cls.lock = json.loads((FIX / "champselect_practicetool_lock.json").read_text())
        cls.champs = ChampionIndex(json.loads((FIX / "champion_subset.json").read_text()))

    def test_hover_is_intent_not_lock(self):
        st = champselect.extract(self.hover)
        me = st.me
        self.assertEqual((st.my_cell, st.phase), (0, "BAN_PICK"))
        self.assertEqual((me.champion_id, me.intent_id, me.locked), (0, 498, False))
        self.assertEqual(me.shown_champion, 498)
        self.assertFalse(st.all_locked)
        self.assertEqual(st.enemies(), [])  # Practice Tool has no enemy team

    def test_lock_and_diff(self):
        prev, cur = champselect.extract(self.hover), champselect.extract(self.lock)
        self.assertEqual((cur.me.champion_id, cur.me.intent_id, cur.me.locked), (498, 0, True))
        text = "\n".join(champselect.diff(prev, cur, self.champs))
        self.assertIn("[PHASE] BAN_PICK -> FINALIZATION", text)
        self.assertIn("[LOCK]  ally cell 0 (you): Xayah", text)
        self.assertIn("ALLY : Xayah *you*", text)


if __name__ == "__main__":
    unittest.main()
