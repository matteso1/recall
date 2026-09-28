#!/usr/bin/env python3
"""Discover recent ranked matches from a sampled current Master+ ladder cohort.

The private sidecar keeps seed-player rank at collection time. It does not claim
that all ten players, or every historical game, had the seed player's current rank.
"""
import argparse
from datetime import datetime, timezone
from pathlib import Path
import random
import sys
import time
from urllib.parse import quote, urlencode

from collect_matches import ApiError, MATCH_ID, PLATFORMS, RiotClient, load_key, private_output, write_json, write_text


def discover(ladder, history, output, *, players=24, count=20, start_time, seed=20260928):
    output = private_output(output)
    if output.suffix != ".txt":
        raise ValueError("Use a .txt seed file; the .json sidecar stores private provenance")
    if not 1 <= players <= 300 or not 1 <= count <= 100 or start_time < 0:
        raise ValueError("Use 1–300 seed players, 1–100 matches each and a nonnegative start time")
    rng = random.Random(seed)
    pools = {}
    for tier in ("master", "grandmaster", "challenger"):
        data = ladder.get(f"/lol/league/v4/{tier}leagues/by-queue/RANKED_SOLO_5x5")
        entries = data.get("entries", [])
        if not entries or any(not isinstance(p.get("puuid"), str) or not p["puuid"] for p in entries):
            raise ValueError("Ladder response is missing stable player identities")
        pools[tier] = sorted(entries, key=lambda p: p["puuid"])
        rng.shuffle(pools[tier])
    cohort, seen = [], set()
    while len(cohort) < players and any(pools.values()):
        for tier, pool in pools.items():
            if not pool or len(cohort) >= players:
                continue
            player = pool.pop()
            if player["puuid"] in seen:
                continue
            seen.add(player["puuid"])
            cohort.append({"puuid": player["puuid"], "tier": tier,
                           "league_points": player.get("leaguePoints")})
    query = urlencode({"queue": 420, "count": count, "startTime": start_time})
    matches = []
    for player in cohort:
        ids = history.get(f"/lol/match/v5/matches/by-puuid/{quote(player['puuid'], safe='')}/ids?{query}")
        if not isinstance(ids, list) or any(not isinstance(i, str) or not MATCH_ID.fullmatch(i) for i in ids):
            raise ValueError("Match history contains invalid match IDs")
        player["matches"] = ids
        matches.extend(ids)
    matches = list(dict.fromkeys(matches))
    # Avoid taking only the first few seed players if the collector uses a smaller limit.
    rng.shuffle(matches)
    write_text(output, "\n".join(matches) + ("\n" if matches else ""))
    write_json(output.with_suffix(".json"), {
        "schema": 1, "source": "Riot ranked solo Master+ ladder seed cohort",
        "discovered_at": datetime.now(timezone.utc).isoformat(), "start_time": start_time,
        "random_seed": seed, "cohort": cohort,
    })
    return {"players": len(cohort), "matches": len(matches)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=sorted(PLATFORMS), required=True)
    parser.add_argument("--players", type=int, default=24)
    parser.add_argument("--matches-per-player", type=int, default=20)
    parser.add_argument("--days", type=int, default=7)
    parser.add_argument("--output", type=Path, required=True, help="Private seed .txt file outside Git")
    parser.add_argument("--key-file", type=Path, default=Path.home() / ".config/recall/riot-api-key")
    args = parser.parse_args()
    if not 1 <= args.days <= 30:
        parser.error("Use a 1–30 day discovery window")
    key = load_key(args.key_file)
    result = discover(RiotClient(key, args.platform), RiotClient(key, PLATFORMS[args.platform]),
                      args.output, players=args.players, count=args.matches_per_player,
                      start_time=int(time.time()) - args.days * 86400)
    print(f"Discovered {result['matches']} unique matches from {result['players']} seed players.")
    print(f"Private seeds saved. Collect with --region {PLATFORMS[args.platform]} and explicit --patches.")


if __name__ == "__main__":
    try:
        main()
    except (ApiError, ValueError, OSError) as error:
        print(f"Discovery stopped: {error}", file=sys.stderr)
        raise SystemExit(1) from None
