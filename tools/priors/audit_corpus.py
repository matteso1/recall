#!/usr/bin/env python3
"""Profile local Kaggle candidates without exporting match or player identifiers.

This measures table coverage, not inventory reconstruction correctness. No download,
credential access, training, or changes to the embedded production pack occur here.
"""

import argparse
import hashlib
import json
from pathlib import Path

import duckdb


def rows(connection, sql):
    result = connection.execute(sql)
    columns = [column[0] for column in result.description]
    return [dict(zip(columns, row)) for row in result.fetchall()]


def fingerprint(path):
    with path.open("rb") as source:
        digest = hashlib.file_digest(source, "sha256").hexdigest()
    return {"bytes": path.stat().st_size, "sha256": digest}


def audit_intervals(connection, directory):
    files = {
        "matches": directory / "matches.csv",
        "participants": directory / "processed_summoner_data.csv",
        "frames": directory / "intervals.csv",
    }
    for name, path in files.items():
        connection.read_csv(str(path)).create_view(name)
    queries = {
        "matches": """
            SELECT count(*) AS row_count, count(DISTINCT match_id) AS matches,
                   min(game_date) AS first_game, max(game_date) AS last_game
            FROM matches
        """,
        "patches": """
            SELECT split_part(game_version, '.', 1) || '.' ||
                   split_part(game_version, '.', 2) AS patch, count(*) AS matches
            FROM matches GROUP BY 1 ORDER BY 2 DESC, 1
        """,
        "queues": """
            SELECT queue_id, game_mode, count(*) AS matches
            FROM matches GROUP BY 1, 2 ORDER BY 3 DESC, 1, 2
        """,
        "participants": """
            SELECT count(*) AS row_count, count(DISTINCT id) AS unique_row_ids,
                   count(DISTINCT match_id) AS matches FROM participants
        """,
        "frames": """
            SELECT count(*) AS row_count, count(DISTINCT match_id) AS matches,
                   count(DISTINCT (match_id, player_id, minute)) AS unique_keys,
                   count(DISTINCT (match_id, minute)) AS checkpoints,
                   min(minute) AS first_minute, max(minute) AS last_minute,
                   min(level) AS min_level, max(level) AS max_level
            FROM frames
        """,
        "join": """
            SELECT count(*) AS matched_frames FROM frames f JOIN participants p
              ON f.player_id = p.id AND f.match_id = p.match_id
        """,
        "checkpoint_teams": """
            SELECT blue, red, count(*) AS checkpoints FROM (
                SELECT f.match_id, minute,
                       count(*) FILTER (WHERE team_id = 100) AS blue,
                       count(*) FILTER (WHERE team_id = 200) AS red
                FROM frames f JOIN participants p
                  ON f.player_id = p.id AND f.match_id = p.match_id
                GROUP BY 1, 2
            ) GROUP BY 1, 2 ORDER BY 1, 2
        """,
        "cadence": """
            SELECT gap_minutes, count(*) AS transitions FROM (
                SELECT minute - lag(minute) OVER (
                    PARTITION BY match_id, player_id ORDER BY minute
                ) AS gap_minutes FROM frames
            ) WHERE gap_minutes IS NOT NULL GROUP BY 1 ORDER BY 1
        """,
        "roles": """
            SELECT individual_position, count(*) AS participants
            FROM participants GROUP BY 1 ORDER BY 2 DESC, 1
        """,
        "master_plus": """
            SELECT count(DISTINCT f.match_id) AS matches_with_frames
            FROM frames f JOIN matches m USING (match_id)
            WHERE average_rank IN ('MASTER', 'GRANDMASTER', 'CHALLENGER')
        """,
        "missing_inventory": """
            SELECT count(*) AS frames FROM frames WHERE item_0 IS NULL OR
                item_1 IS NULL OR item_2 IS NULL OR item_3 IS NULL OR
                item_4 IS NULL OR item_5 IS NULL OR item_6 IS NULL
        """,
    }
    return {
        "source": "nathansmallcalder/league-of-legends-match-interval-snapshots-2026",
        "files": {path.name: fingerprint(path) for path in files.values()},
        "participant_columns": connection.table("participants").columns,
        **{name: rows(connection, sql) for name, sql in queries.items()},
    }


def audit_current(connection, players, timeline):
    connection.read_parquet(str(players)).create_view("current_players")
    connection.read_csv(str(timeline)).create_view("current_timeline")
    # Every focal player-minute could observe five opponents. Count directed
    # opposite-team pairs with observations at the same minute before split filters.
    query = """
        WITH counts AS (
            SELECT p.mid, t.Minute,
                   count(*) FILTER (WHERE p.blue = 1) AS blue,
                   count(*) FILTER (WHERE p.blue = 0) AS red
            FROM current_timeline t JOIN current_players p
              ON t.SummonerMatchFk = p.smid
            WHERE t.Minute >= 6 {patch_filter}
            GROUP BY 1, 2
        ) SELECT sum(2 * blue * red) AS observed_enemy_frame_pairs,
                 sum(5 * (blue + red)) AS possible_enemy_frame_pairs,
                 count(*) AS match_minutes, sum(blue + red) AS player_minutes,
                 sum(2 * blue * red)::DOUBLE /
                     nullif(sum(5 * (blue + red)), 0) AS coverage FROM counts
    """
    return {
        "files": {path.name: fingerprint(path) for path in [players, timeline]},
        "all": rows(connection, query.format(patch_filter=""))[0],
        "recent_16_17_16_18": rows(connection, query.format(
            patch_filter="AND p.patch IN ('16.17', '16.18')"
        ))[0],
    }


def main():
    root = Path.home() / "data/recall"
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--intervals", type=Path, default=root / "kaggle/intervals-2026")
    parser.add_argument("--players", type=Path, default=root / "derived/players.parquet")
    parser.add_argument("--timeline", type=Path,
                        default=root / "kaggle/ranked-timeline/MatchTimelineTbl.csv")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with duckdb.connect() as connection:
        connection.execute("SET threads = 4")
        report = {
            "schema_version": 1,
            "intervals": audit_intervals(connection, args.intervals),
            "current_raw_corpus": audit_current(connection, args.players, args.timeline),
        }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, default=str) + "\n")
    print(json.dumps(report["intervals"]["frames"][0], sort_keys=True))
    for split in ["all", "recent_16_17_16_18"]:
        coverage = report["current_raw_corpus"][split]["coverage"]
        print(f"Current raw corpus ({split}): {coverage:.2%} enemy-frame coverage")
    print(f"Aggregate report: {args.output}")


if __name__ == "__main__":
    main()
