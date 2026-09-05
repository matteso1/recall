"""Build an LCU item set from a names-based spec (data/itemsets/*.json) and merge it into
the client's list, so the ordered build shows up inside the in-game shop.

Specs name items and champions by their Data Dragon *names*; ids are resolved at push
time, so the spec survives item-id churn between patches.
"""
from __future__ import annotations

import json
import re
import time
import uuid
from pathlib import Path
from typing import Any

from ddragon import ChampionIndex, ItemIndex

TITLE_PREFIX = "Featherstorm"
UID_NAMESPACE = uuid.UUID("6f1c3d8e-0a2b-4c5d-9e7f-1234567890ab")
_COUNT_RE = re.compile(r"^(.*?)\s*[x×]\s*(\d+)$")


class SpecError(Exception):
    pass


def load_spec(path) -> dict:
    return json.loads(Path(path).read_text(encoding="utf-8"))


def parse_item_entry(entry: Any) -> tuple[str, int, str]:
    """'Health Potion x2' -> ('Health Potion', 2, ''); {'item': ..., 'why': ...} -> (item, count, why)."""
    if isinstance(entry, dict):
        return str(entry["item"]), int(entry.get("count", 1)), str(entry.get("why", ""))
    m = _COUNT_RE.match(str(entry))
    if m:
        return m.group(1), int(m.group(2)), ""
    return str(entry), 1, ""


def _block(title: str, entries: list, items: ItemIndex, warnings: list[str]) -> dict:
    merged: dict[str, int] = {}
    for e in entries:
        name, count, _ = parse_item_entry(e)
        iid = items.id_for(name)
        if iid is None:
            warnings.append(f"unknown item name {name!r} (block {title!r})")
            continue
        merged[iid] = merged.get(iid, 0) + count
    return {
        "type": title,
        "hideIfSummonerSpell": "",
        "showIfSummonerSpell": "",
        "items": [{"id": iid, "count": n} for iid, n in merged.items()],
    }


def build_item_set(spec: dict, items: ItemIndex, champs: ChampionIndex) -> tuple[dict, list[str]]:
    """Returns (LCU item set, warnings). Warnings list every name that did not resolve."""
    warnings: list[str] = []
    champ_name = spec.get("champion")
    if not champ_name:
        raise SpecError("spec needs a 'champion'")
    key = champs.key_for(champ_name)
    if key is None:
        raise SpecError(f"unknown champion {champ_name!r}")
    title = spec.get("title") or f"{TITLE_PREFIX} {champ_name}"

    blocks = [_block(b["type"], b["items"], items, warnings) for b in spec.get("blocks", [])]

    core_names: list[str] = []
    component_order = spec.get("component_order", {})
    for n, entry in enumerate(spec.get("core", []), 1):
        name, _, why = parse_item_entry(entry)
        core_names.append(name)
        iid = items.id_for(name)
        if iid is None:
            warnings.append(f"unknown core item {name!r}")
            continue
        comps = component_order.get(name) or [items.name(c) for c in items.components(iid)]
        heading = f"{n}. {name}" + (f" - {why}" if why else "")
        blocks.append(_block(heading, comps + [name], items, warnings))
    if core_names:
        blocks.append(_block("Full build (in order)", core_names, items, warnings))

    for sit in spec.get("situational", []):
        blocks.append(_block(f"If {sit['when']}", sit["items"], items, warnings))
    if spec.get("consumables"):
        blocks.append(_block("Vision & consumables", spec["consumables"], items, warnings))

    return {
        "uid": str(uuid.uuid5(UID_NAMESPACE, title)),   # stable, so re-pushing replaces in place
        "title": title,
        "mode": "any",
        "map": "any",
        "type": "custom",
        "sortrank": int(spec.get("sortrank", 0)),
        "startedFrom": "blank",
        "associatedChampions": [key],
        "associatedMaps": [int(m) for m in spec.get("maps", [])],
        "blocks": blocks,
        "preferredItemSlots": [],
    }, warnings


def upsert(payload: dict, item_set: dict) -> dict:
    """New payload for PUT: existing sets kept, any set with the same title replaced."""
    others = [s for s in payload.get("itemSets", []) if s.get("title") != item_set["title"]]
    return {**payload, "itemSets": others + [item_set], "timestamp": int(time.time() * 1000)}


def remove(payload: dict, title: str) -> dict:
    others = [s for s in payload.get("itemSets", []) if s.get("title") != title]
    return {**payload, "itemSets": others, "timestamp": int(time.time() * 1000)}


def describe(item_set: dict, items: ItemIndex) -> str:
    lines = [f"{item_set['title']}  (champions {item_set['associatedChampions']}, "
             f"{len(item_set['blocks'])} blocks)"]
    for b in item_set["blocks"]:
        names = ", ".join(items.name(i["id"]) + (f" x{i['count']}" if i["count"] > 1 else "") for i in b["items"])
        lines.append(f"  {b['type']}: {names}")
    return "\n".join(lines)
