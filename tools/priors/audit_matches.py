#!/usr/bin/env python3
"""Write aggregate-only coverage for validated local Match-v5 pairs, without player IDs."""
import argparse
from collections import Counter, defaultdict
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path


def audit(root):
    groups = defaultdict(Counter)
    identities = set()
    for manifest_path in sorted(root.glob("*/*/complete.json")):
        manifest = json.loads(manifest_path.read_text())
        folder = manifest_path.parent
        responses = {}
        for name in ("match.json", "timeline.json"):
            raw = (folder / name).read_bytes()
            if hashlib.sha256(raw).hexdigest() != manifest["sha256"][name]:
                raise ValueError("A completed pair changed after validation; revalidate before auditing")
            responses[name] = json.loads(raw)
        group = groups[f"{folder.parent.name}/{manifest['patch']}"]
        group["matches"] += 1
        group["frames"] += manifest["frames"]
        group["player_frames"] += manifest["frames"] * manifest["participants"]
        for name, count in manifest["event_counts"].items():
            group[f"events/{name}"] += count
        for player in responses["match.json"]["info"]["participants"]:
            identities.add(player["puuid"])
            group["players"] += 1
            group["known_roles"] += player.get("teamPosition") in {"TOP", "JUNGLE", "MIDDLE", "BOTTOM", "UTILITY"}
            group["two_summoner_spells"] += all(player.get(key, 0) > 0 for key in ("summoner1Id", "summoner2Id"))
            perks = player.get("perks", {})
            styles = perks.get("styles", [])
            group["full_rune_pages"] += (len(styles) == 2 and
                sorted(len(s.get("selections", [])) for s in styles) == [2, 4] and
                len(perks.get("statPerks", {})) == 3)
        for frame in responses["timeline.json"]["info"]["frames"]:
            for player in frame["participantFrames"].values():
                group["frames_with_position"] += all(k in player.get("position", {}) for k in ("x", "y"))
                group["frames_with_champion_stats"] += bool(player.get("championStats"))
                group["frames_with_damage_stats"] += bool(player.get("damageStats"))
    return {
        "schema": 1, "audited_at": datetime.now(timezone.utc).isoformat(),
        "source": "Paired official Riot Match-v5 responses with verified source hashes",
        "unique_players": len(identities),
        "groups": {key: dict(sorted(value.items())) for key, value in sorted(groups.items())},
        "limits": [
            "Collection is seeded by tracked historical players or current Master+ ladder players; not every participant has a verified rank",
            "Minute frames plus timestamped events; no sub-minute gold/stat snapshots",
            "Complete raw events are preserved; inventory reconstruction is not yet a training contract",
            "Historical ten-player information must be filtered to features available to the live app",
            "New pairs have not been used to fit production models",
        ],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.home() / "data/recall/riot")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    report = audit(args.root)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print("Audited", sum(g["matches"] for g in report["groups"].values()), "complete match/timeline pairs.")


if __name__ == "__main__":
    main()
