#!/usr/bin/env python3
"""Push (or remove) a Recall item set in the League client so it shows in the in-game shop.

Reads a names-based spec (default: data/itemsets/xayah.json), resolves names through
Data Dragon, merges the set into the account's existing sets and writes them back.
Re-running replaces the previous Recall set of the same title.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

import ddragon
import itemsets
from lcu import LCU, LCUError
from transport import ConnectionFailed
from winenv import ClientNotRunning

REPO = Path(__file__).resolve().parents[1]


def remove_title(title: str) -> int:
    try:
        lcu = LCU.connect()
        summoner_id = lcu.current_summoner()["summonerId"]
        current = lcu.item_sets(summoner_id)
        before = [s.get("title") for s in current.get("itemSets", [])]
        if title not in before:
            print(f"no item set titled {title!r}; current sets: {before}")
            return 1
        lcu.put_item_sets(summoner_id, itemsets.remove(current, title))
        after = [s.get("title") for s in lcu.item_sets(summoner_id).get("itemSets", [])]
    except ClientNotRunning as e:
        print(f"League client not running: {e}")
        return 1
    except (ConnectionFailed, LCUError, RuntimeError) as e:
        print(f"LCU error: {e}")
        return 1
    print(f"removed {title!r}; item sets now: {after}")
    return 0 if title not in after else 1


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--spec", type=Path, default=REPO / "data" / "itemsets" / "xayah.json")
    ap.add_argument("--remove", action="store_true", help="remove the set with this spec's title")
    ap.add_argument("--remove-title", metavar="TITLE",
                    help="remove the set with exactly this title (e.g. an old 'OP.GG Xayah') and exit")
    ap.add_argument("--dry-run", action="store_true", help="build and print the set; do not touch the client")
    ap.add_argument("--offline", action="store_true", help="use cached Data Dragon data only")
    args = ap.parse_args(argv)

    if args.remove_title:
        return remove_title(args.remove_title)

    spec = itemsets.load_spec(args.spec)
    items, champs, version = ddragon.load_indexes(offline=args.offline)
    built, warnings = itemsets.build_item_set(spec, items, champs)
    print(f"Data Dragon {version}")
    print(itemsets.describe(built, items))
    for w in warnings:
        print(f"warning: {w}")
    if warnings and not args.remove:
        print("some names did not resolve; fix the spec (names must match Data Dragon) and retry")
        return 2
    if args.dry_run:
        print(json.dumps(built, indent=1))
        return 0

    try:
        lcu = LCU.connect()
        me = lcu.current_summoner()
        summoner_id = me["summonerId"]
        current = lcu.item_sets(summoner_id)
        before = [s.get("title") for s in current.get("itemSets", [])]
        payload = itemsets.remove(current, built["title"]) if args.remove else itemsets.upsert(current, built)
        lcu.put_item_sets(summoner_id, payload)
        after = [s.get("title") for s in lcu.item_sets(summoner_id).get("itemSets", [])]
    except ClientNotRunning as e:
        print(f"League client not running: {e}")
        return 1
    except (ConnectionFailed, LCUError, RuntimeError) as e:
        print(f"LCU error: {e}")
        return 1

    print(f"{'removed' if args.remove else 'pushed'} '{built['title']}' for {me.get('gameName')}#{me.get('tagLine')} "
          f"via {lcu.transport.name}")
    print(f"item sets before: {before}")
    print(f"item sets after : {after}")
    present = built["title"] in after
    if present == args.remove:
        print("verification FAILED: the client does not show the expected state")
        return 1
    print("verified: the client returned the updated list")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
