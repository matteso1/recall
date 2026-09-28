"""Causal Match-v5 inventory ledger; final items validate, never repair history.

Trinkets are outside the reconciliation contract: implicit grants and Herald swaps
are not fully logged. Stack presence (not quantity) is all the final response
exposes. Unobserved transformations carry explicit alternatives and make affected
frames ineligible for exact-inventory evaluation.
"""
from collections import Counter
from itertools import groupby

ITEM_EVENTS = {"ITEM_PURCHASED", "ITEM_DESTROYED", "ITEM_SOLD", "ITEM_UNDO"}
TEAR = {3003: 3040, 3004: 3042, 3119: 3121, 2526: 2530}
SUPPORT = {3869, 3870, 3871, 3876, 3877}


def equipment(item):
    return bool(item) and not ("Lane" in item.get("tags", []) and
                              not item.get("gold", {}).get("total", 0))


def runes(player):
    return {s["perk"] for style in player.get("perks", {}).get("styles", [])
            for s in style.get("selections", [])}


class Ledger:
    def __init__(self, items, player):
        self.catalog = items
        self.player = player
        self.runes = runes(player)
        self.bag = Counter()
        self.uncertain = {}  # representative -> possible identities, never filled from final items
        self.issues = []
        self.inferences = []
        self.transactions = []
        self.purchases = []
        self.frames = []
        self.adc_quest = False
        self.mid_quest = False
        self.boot_time = 720000 if 8304 in self.runes else None
        self.biscuit_times = [120000, 240000, 360000] if 8345 in self.runes else []
        self.tonics = set()
        if player.get("teamPosition") == "UTILITY" and "3865" in items:
            # The purchase is absent in these timelines. Its start time is unknown.
            self.uncertain[3865] = {0, 3865}
        if player.get("championName") == "Viego":
            self.issue("unsupported_possession_inventory", 0)

    def auxiliary(self, item):
        return "Trinket" in self.catalog.get(str(item), {}).get("tags", [])

    def tracked(self, item):
        return item not in (0, 2001, 2002) and not self.auxiliary(item) and equipment(self.catalog.get(str(item)))

    def issue(self, kind, timestamp, **details):
        self.issues.append(dict(kind=kind, timestamp=timestamp, **details))

    def grant(self, item, timestamp, reason):
        if str(item) not in self.catalog:
            self.issue("unknown_grant", timestamp, item=item)
            return
        self.bag[item] += 1
        self.inferences.append(dict(timestamp=timestamp, item=item, reason=reason))

    def advance(self, timestamp):
        while self.biscuit_times and self.biscuit_times[0] <= timestamp:
            self.grant(2010, self.biscuit_times.pop(0), "biscuit_rune")
        if self.boot_time is not None and self.boot_time <= timestamp:
            self.grant(2422, self.boot_time, "footwear_rune")
            self.boot_time = None

    def public_event(self, event):
        timestamp = event["timestamp"]
        if event["type"] == "CHAMPION_KILL" and self.boot_time is not None:
            pid = self.player["participantId"]
            if pid in [event.get("killerId"), *event.get("assistingParticipantIds", [])]:
                self.boot_time = max(timestamp, self.boot_time - 45000)
                self.advance(timestamp)
        if event["type"] == "LEVEL_UP" and event.get("participantId") == self.player["participantId"] and 8313 in self.runes:
            level = event["level"]
            if level in (3, 6, 9) and level not in self.tonics:
                self.tonics.add(level)
                self.grant({3: 2151, 6: 2152, 9: 2150}[level], timestamp, "triple_tonic")
                if level == 9:
                    # Automatic consumption is not consistently an ITEM_DESTROYED event.
                    self.bag[2150] -= 1
                    self.uncertain[2150] = {0, 2150}

    def remove(self, item, timestamp):
        if item in self.uncertain:
            del self.uncertain[item]
            return True
        for representative, choices in list(self.uncertain.items()):
            if item in choices:
                del self.uncertain[representative]
                return True
        if self.bag[item] <= 0:
            self.issue("unowned_removal", timestamp, item=item)
            return False
        self.bag[item] -= 1
        return True

    def boot_upgrade(self, item):
        return next((int(i) for i, data in self.catalog.items()
                     if data.get("from") == [str(item)] and data.get("gold", {}).get("base") == 0
                     ), None)

    def recipe_parts(self, item):
        parts, pending = set(), list(self.catalog.get(str(item), {}).get("from", []))
        while pending:
            part = int(pending.pop())
            if part in parts:
                continue
            parts.add(part)
            pending.extend(self.catalog.get(str(part), {}).get("from", []))
        return parts

    def apply(self, events):
        timestamp = events[0]["timestamp"]
        destroyed = Counter(e["itemId"] for e in events if e["type"] == "ITEM_DESTROYED")
        buys = Counter(e["itemId"] for e in events if e["type"] == "ITEM_PURCHASED")
        if destroyed[1202]:
            self.adc_quest = True
        if destroyed[1201]:
            self.mid_quest = True
        pending = []
        consumed = Counter()
        upgrade_on_buy = set()
        for event in events:
            kind = event["type"]
            item = event.get("itemId", 0)
            if kind != "ITEM_UNDO":
                if item and str(item) not in self.catalog:
                    self.issue("unknown_item", timestamp, item=item)
                if not self.tracked(item):
                    continue
            if kind == "ITEM_DESTROYED":
                data = self.catalog.get(str(item), {})
                if "Boots" in data.get("tags", []):
                    if destroyed[1202] and not buys:
                        self.inferences.append(dict(timestamp=timestamp, item=item, reason="adc_boots_slot_move"))
                        continue
                    upgrade = self.boot_upgrade(item) if self.mid_quest else None
                    if upgrade and buys[item] and self.bag[item] <= 0:
                        upgrade_on_buy.add(item)
                        continue
                    if upgrade and not buys:
                        self.remove(item, timestamp)
                        self.grant(upgrade, timestamp, "mid_quest_upgrade")
                        continue
                # Support role slot refresh destroys a placeholder immediately before purchase.
                if item == 2055 and buys[item] and (destroyed[1203] or destroyed[1208]):
                    self.inferences.append(dict(timestamp=timestamp, item=item, reason="support_ward_slot_refresh"))
                    continue
                if self.bag[item] <= 0 and buys[item] > consumed[item] and data.get("consumed"):
                    consumed[item] += 1
                else:
                    self.remove(item, timestamp)
                    pending.append(item)
            elif kind == "ITEM_PURCHASED":
                owned = self.inventory() + list(self.uncertain)
                recipe = self.recipe_parts(item)
                components = [p for p in pending if {2422: 1001, 2421: 2420}.get(p, p) in recipe]
                pending = [p for p in pending if {2422: 1001, 2421: 2420}.get(p, p) not in recipe]
                if consumed[item]:
                    consumed[item] -= 1
                elif item in upgrade_on_buy:
                    self.grant(self.boot_upgrade(item), timestamp, "mid_quest_upgrade")
                elif item in TEAR:
                    # Match-v5 does not reliably emit the stacking transformation time.
                    self.uncertain[item] = {item, TEAR[item]}
                else:
                    self.bag[item] += 1
                if item == 3865:
                    self.uncertain.pop(item, None)
                purchase = dict(timestamp=timestamp, item=item, consumed=components, owned=owned, retained=True)
                self.purchases.append(purchase)
                granted = self.boot_upgrade(item) if item in upgrade_on_buy else item
                self.transactions.append(dict(kind="buy", item=item, granted=granted, consumed=components, purchase=purchase, undone=False))
            elif kind == "ITEM_SOLD":
                self.remove(item, timestamp)
                self.transactions.append(dict(kind="sell", item=item, consumed=[], undone=False))
            elif kind == "ITEM_UNDO":
                before, after = event.get("beforeId", 0), event.get("afterId", 0)
                if (not before or self.auxiliary(before)) and (not after or self.auxiliary(after)):
                    continue
                expected = "buy" if before and not after else "sell" if after and not before else None
                target = before or after
                transaction = next((t for t in reversed(self.transactions)
                                    if not t["undone"] and t["kind"] == expected and t["item"] == target), None)
                if transaction is None:
                    self.issue("unmatched_undo", timestamp, before=before, after=after)
                else:
                    transaction["undone"] = True
                    if expected == "buy":
                        self.remove(transaction["granted"], timestamp)
                        self.bag.update(transaction["consumed"])
                        transaction["purchase"]["retained"] = False
                    else:
                        self.bag[target] += 1
        # Unconsumed destruction events can be supported one-to-one transformations.
        for item in pending:
            if item in (3865, 3866):
                self.grant(item + 1, timestamp, "support_quest_upgrade")
            elif item == 3867:
                self.uncertain[3867] = SUPPORT.copy()
                self.inferences.append(dict(timestamp=timestamp, item=item, reason="unobserved_support_choice"))
            elif item == 2420:
                self.grant(2421, timestamp, "used_armguard")
            elif item in TEAR:
                self.grant(TEAR[item], timestamp, "tear_transform_event")

    def inventory(self):
        return sorted(self.bag.elements())

    def snapshot(self, timestamp):
        boots = next((i for i in self.inventory() if "Boots" in self.catalog[str(i)].get("tags", [])), None)
        return dict(timestamp=timestamp, items=self.inventory(), exact=not self.uncertain and not self.issues,
                    uncertain={str(k): sorted(v) for k, v in self.uncertain.items()},
                    role_slot_boots=boots if self.adc_quest else None)

    def final_check(self, player, timestamp):
        expected = [player.get(f"item{i}", 0) for i in range(7)] + [player.get("roleBoundItem", 0)]
        expected = Counter(i for i in expected if self.tracked(i))
        actual = Counter(self.inventory())
        for item in set(expected) | set(actual):
            if self.catalog[str(item)].get("stacks", 1) > 1:
                expected[item] = min(1, expected[item])
                actual[item] = min(1, actual[item])
        # Resolve only the final compatibility check, never earlier snapshots.
        for choices in self.uncertain.values():
            candidates = sorted(i for i in choices if expected[i] > actual[i])
            if candidates:
                actual[candidates[0]] += 1
            elif 0 not in choices:
                actual[min(choices)] += 1
        if actual != expected:
            self.issue("final_inventory_mismatch", timestamp,
                       missing=list((expected-actual).elements()), extra=list((actual-expected).elements()))
        return not any(i["kind"] == "final_inventory_mismatch" for i in self.issues)


def reconstruct(match, timeline, items):
    participants = {p["participantId"]: p for p in match["info"]["participants"]}
    ledgers = {pid: Ledger(items, player) for pid, player in participants.items()}
    events = [e for f in timeline["info"]["frames"] for e in f.get("events", [])
              if e["type"] in ITEM_EVENTS | {"CHAMPION_KILL", "LEVEL_UP"}]
    events.sort(key=lambda e: e["timestamp"])
    batches = [(timestamp, list(group)) for timestamp, group in groupby(events, key=lambda e: e["timestamp"])]
    index = 0
    for frame in timeline["info"]["frames"]:
        while index < len(batches) and batches[index][0] <= frame["timestamp"]:
            timestamp, batch = batches[index]
            for ledger in ledgers.values():
                ledger.advance(timestamp)
            by_player = {}
            for event in batch:
                if event["type"] in ITEM_EVENTS:
                    by_player.setdefault(event["participantId"], []).append(event)
                else:
                    for ledger in ledgers.values():
                        ledger.public_event(event)
            for pid, player_batch in by_player.items():
                if pid in ledgers:
                    ledgers[pid].apply(player_batch)
            index += 1
        for ledger in ledgers.values():
            ledger.advance(frame["timestamp"])
            ledger.frames.append(ledger.snapshot(frame["timestamp"]))
    results = {}
    for pid, ledger in ledgers.items():
        reconciled = ledger.final_check(participants[pid], timeline["info"]["frames"][-1]["timestamp"])
        results[pid] = dict(valid=not ledger.issues and not ledger.uncertain, reconciled=reconciled,
                            issues=ledger.issues, inferences=ledger.inferences, frames=ledger.frames,
                            purchases=ledger.purchases, final_items=ledger.inventory())
    return dict(players=results)
