"""Inventory contracts use synthetic items and anonymous participant slots."""
import unittest

from riot_inventory import equipment, reconstruct


def item(name, cost, *, parts=(), tags=(), **extra):
    return dict(name=name, gold=dict(total=cost, base=cost, purchasable=True),
                maps={"11": True}, tags=list(tags), **({"from": list(map(str, parts))} if parts else {}), **extra)


CATALOG = {"100": item("Part", 300), "200": item("Combined", 1000, parts=[100, 100]),
           "300": item("Potion", 50, consumed=True, stacks=5),
           "400": item("Ward", 0, tags=["Trinket"])}


def event(kind, time, **fields):
    return dict(type=kind, timestamp=time, participantId=1, **fields)


def fixture(events, final, *, times=(0, 60000, 120000), role="MIDDLE", role_item=0):
    player = dict(participantId=1, championName="Fixture", teamPosition=role, roleBoundItem=role_item,
                  **{f"item{i}": final[i] if i < len(final) else 0 for i in range(7)})
    match = {"info": {"participants": [player]}}
    frames = [dict(timestamp=t, events=[e for e in events if (times[i-1] if i else -1) < e["timestamp"] <= t])
              for i, t in enumerate(times)]
    return match, {"info": {"frames": frames}}


class InventoryContracts(unittest.TestCase):
    def run_case(self, events, final, **kwargs):
        return reconstruct(*fixture(events, final, **kwargs), CATALOG)["players"][1]

    def test_undo_combined_item_restores_actual_components_and_cancels_label(self):
        events = [event("ITEM_PURCHASED", 1, itemId=100), event("ITEM_PURCHASED", 2, itemId=100),
                  event("ITEM_DESTROYED", 65000, itemId=100), event("ITEM_DESTROYED", 65000, itemId=100),
                  event("ITEM_PURCHASED", 65000, itemId=200),
                  event("ITEM_UNDO", 66000, beforeId=200, afterId=0, goldGain=400)]
        result = self.run_case(events, [100, 100])
        self.assertTrue(result["valid"], result["issues"])
        self.assertEqual(result["frames"][-1]["items"], [100, 100])
        self.assertFalse(next(p for p in result["purchases"] if p["item"] == 200)["retained"])

    def test_sale_undo_preserves_duplicates(self):
        result = self.run_case([event("ITEM_PURCHASED", 1, itemId=100),
                               event("ITEM_PURCHASED", 2, itemId=100),
                               event("ITEM_SOLD", 65000, itemId=100),
                               event("ITEM_UNDO", 66000, beforeId=0, afterId=100, goldGain=-210)], [100, 100])
        self.assertTrue(result["valid"], result["issues"])
        self.assertEqual(result["frames"][-1]["items"], [100, 100])

    def test_undo_does_not_resurrect_unrelated_consumption_at_same_time(self):
        events = [event("ITEM_PURCHASED", 1, itemId=100), event("ITEM_PURCHASED", 2, itemId=300),
                  event("ITEM_DESTROYED", 65000, itemId=300), event("ITEM_DESTROYED", 65000, itemId=100),
                  event("ITEM_PURCHASED", 65000, itemId=200),
                  event("ITEM_UNDO", 66000, beforeId=200, afterId=0)]
        result = self.run_case(events, [100])
        self.assertTrue(result["valid"], result["issues"])
        self.assertEqual(result["frames"][-1]["items"], [100])

    def test_same_time_potion_consumption_is_not_an_unknown_removal(self):
        result = self.run_case([event("ITEM_DESTROYED", 1, itemId=300),
                               event("ITEM_PURCHASED", 1, itemId=300)], [])
        self.assertTrue(result["valid"], result["issues"])
        self.assertEqual(result["frames"][-1]["items"], [])

    def test_missing_purchase_and_final_mismatch_are_not_backfilled(self):
        result = self.run_case([event("ITEM_PURCHASED", 1, itemId=100)], [200])
        self.assertFalse(result["valid"])
        self.assertEqual(result["frames"][1]["items"], [100])
        self.assertIn("final_inventory_mismatch", [i["kind"] for i in result["issues"]])
        result = self.run_case([event("ITEM_DESTROYED", 65000, itemId=100)], [])
        self.assertFalse(result["valid"])
        self.assertIn("unowned_removal", [i["kind"] for i in result["issues"]])

    def test_future_events_do_not_change_earlier_frames(self):
        result = self.run_case([event("ITEM_PURCHASED", 65000, itemId=100)], [100])
        self.assertTrue(result["valid"])
        self.assertEqual(result["frames"][1]["items"], [])
        self.assertEqual(result["frames"][2]["items"], [100])

    def test_lane_tag_does_not_remove_control_wards_or_support_items(self):
        self.assertTrue(equipment(item("Control Ward", 75, tags=["Lane", "Consumable"])))
        self.assertFalse(equipment(dict(tags=["Lane"], gold=dict(total=0, purchasable=False))))

    def test_recall_tokens_are_not_physical_inventory(self):
        catalog = CATALOG | {"2001": item("Recall", 60)}
        result = reconstruct(*fixture([event("ITEM_DESTROYED", 2, itemId=2001)], []), catalog)["players"][1]
        self.assertTrue(result["valid"], result["issues"])

    def test_final_slots_do_not_expose_stack_counts(self):
        result = self.run_case([event("ITEM_PURCHASED", 1, itemId=300),
                               event("ITEM_PURCHASED", 2, itemId=300)], [300])
        self.assertTrue(result["valid"], result["issues"])
        self.assertEqual(result["frames"][-1]["items"], [300, 300])

    def test_free_biscuits_are_granted_at_the_observed_time_only(self):
        match, timeline = fixture([event("ITEM_DESTROYED", 120001, itemId=2010)], [], times=(0, 60000, 120000, 180000))
        match["info"]["participants"][0]["perks"] = {"styles": [{"selections": [{"perk": 8345}]}]}
        catalog = CATALOG | {"2010": item("Biscuit", 0, consumed=True, stacks=3)}
        result = reconstruct(match, timeline, catalog)["players"][1]
        self.assertTrue(result["valid"], result["issues"])
        self.assertEqual(result["frames"][1]["items"], [])
        self.assertEqual(result["frames"][2]["items"], [2010])
        self.assertEqual(result["frames"][3]["items"], [])

    def test_adc_quest_moves_boots_without_consuming_them(self):
        catalog = CATALOG | {"1001": item("Boots", 300, tags=["Boots"]),
                             "1202": dict(tags=["Lane"], gold=dict(total=0, purchasable=False))}
        events = [event("ITEM_PURCHASED", 1, itemId=1001),
                  event("ITEM_DESTROYED", 65000, itemId=1202),
                  event("ITEM_DESTROYED", 65000, itemId=1001)]
        result = reconstruct(*fixture(events, [], role="BOTTOM", role_item=1001), catalog)["players"][1]
        self.assertTrue(result["valid"], result["issues"])
        self.assertEqual(result["frames"][-1]["items"], [1001])
        self.assertEqual(result["frames"][-1]["role_slot_boots"], 1001)

    def test_mid_free_upgrade_undo_restores_parts_not_the_free_upgrade(self):
        catalog = CATALOG | {"1001": item("Boots", 300, tags=["Boots"]),
                             "3006": item("Tier two", 1100, parts=[1001], tags=["Boots"]),
                             "3172": dict(name="Upgrade", tags=["AttackSpeed"],
                                          gold=dict(total=1100, base=0, purchasable=True), **{"from": ["3006"]}),
                             "1201": dict(tags=["Lane"], gold=dict(total=0, purchasable=False))}
        events = [event("ITEM_PURCHASED", 1, itemId=1001), event("ITEM_DESTROYED", 2, itemId=1201),
                  event("ITEM_DESTROYED", 65000, itemId=1001),
                  event("ITEM_DESTROYED", 65000, itemId=3006),
                  event("ITEM_PURCHASED", 65000, itemId=3006),
                  event("ITEM_UNDO", 66000, beforeId=3006, afterId=0)]
        result = reconstruct(*fixture(events, [1001]), catalog)["players"][1]
        self.assertTrue(result["valid"], result["issues"])
        self.assertEqual(result["frames"][-1]["items"], [1001])

    def test_support_final_choice_does_not_fill_earlier_unknown_slots(self):
        catalog = CATALOG | {str(i): item("Support", 400) for i in (3865,3866,3867,3877)}
        events = [event("ITEM_DESTROYED", 1, itemId=3865),
                  event("ITEM_DESTROYED", 65000, itemId=3866),
                  event("ITEM_DESTROYED", 66000, itemId=3867)]
        result = reconstruct(*fixture(events, [3877], role="UTILITY"), catalog)["players"][1]
        self.assertTrue(result["reconciled"], result["issues"])
        self.assertFalse(result["frames"][0]["exact"])
        self.assertTrue(result["frames"][1]["exact"])
        self.assertEqual(result["frames"][1]["items"], [3866])
        self.assertFalse(result["frames"][-1]["exact"])
        self.assertNotIn(3877, result["frames"][-1]["items"])


if __name__ == "__main__":
    unittest.main()
