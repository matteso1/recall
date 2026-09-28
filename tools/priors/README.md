# Engine v3 priors: what Master+ players buy next

Builds `data/pack/next_items.json`, the backbone `overlay/core/src/nextprior.rs` reads. These are the scripts as
they were run on 2026-09-27; they need a Python venv with duckdb, pandas, numpy and pyarrow (not the
stdlib-only `m0/` environment).

Source: Kaggle `nathansmallcalder/league-of-legends-ranked-post-match-and-timeline` (MIT; 77k Master+ ranked
matches, patches 16.11-16.18, per-minute inventories), downloaded to `~/data/recall/kaggle/ranked-timeline/`.
The derived decision table is kept at `~/data/recall/derived/dec.parquet` so steps 1-4 only need re-running for a
new dataset.

Run from one work directory (the scripts read and write `rt.duckdb` there; `p01_build.py` expects `rt.duckdb` in the
parent of its own folder and writes `data/dec.parquet`, so copy it and `lib.py` into a subfolder of the work dir):

1. `load.py`: CSV tables into `rt.duckdb`.
2. `mk_items.py`: per-patch Data Dragon item data (recipes, prices) into the database.
3. `derive3.py`: purchase events from per-minute inventory diffs, reconciled against gold spent (97.6% of minutes
   with item changes reconcile to 10 gold; handles the compacted 7-entry item list, transforms and undo).
4. `p01_build.py`: one row per completed item with the owned set, state and both teams' champions (`dec.parquet`).
5. `export_next.py <dec.parquet> <out.json> [holdout]`: weighted next-legendary counts per champion and role, per
   owned legendary set and build step, plus the role's enemy-composition lifts (healer, magic-heavy, tanky, defined
   from `data/pack/champion_traits.json` exactly as `nextprior::Comp` does). With `holdout`, 15% of players are held
   out: owned-set model top-1 0.506 / top-3 0.759 against 0.474 / 0.712 for a static build order.
6. `export_bans.py <players.parquet> <champion.json> <out.json>`: champ select ban suggestions per champion and
   role (`data/pack/bans.json`, read by `core/src/bans.rs`): enemy presence x (base win rate - win rate with that
   enemy in the game, shrunk 30 games toward the base). Champion-roles under 200 games use the role's table.
7. `export_cs.py <MatchTimelineTbl.csv> <players.parquet> <champion.json> <out.json>`: median CS at 10 minutes and
   CS per minute per champion and role (`data/pack/cs_bench.json`, read by `core/src/csbench.rs` for the post-game
   recap). Master+ ADCs: 78 CS at 10, 8.0 per minute; Xayah 80 and 8.7.

Re-run per patch: patches 16.17-16.18 count double (their shop equals 16.19's), 16.13-16.16 half. When the shop
changes, rebuild with the new patch data and re-check the item diff first.
