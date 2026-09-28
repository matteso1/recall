# Engine v3 priors: what Master+ players buy next

Builds `data/pack/next_items.json`, the backbone `overlay/core/src/nextprior.rs` reads. These are the scripts as
they were run on 2026-09-27; they need a Python venv with duckdb, pandas, numpy and pyarrow (not the
stdlib-only `m0/` environment).

Source: Kaggle `nathansmallcalder/league-of-legends-ranked-post-match-and-timeline` (MIT; 77k Master+ ranked
matches, patches 16.11-16.18, per-minute inventories), downloaded to `~/data/recall/kaggle/ranked-timeline/`.
The derived decision table is kept at `~/data/recall/derived/dec.parquet` so steps 1-4 only need re-running for a
new dataset.

The [September data-source audit](../../docs/notes/data-source-audit.md) compares richer Kaggle
candidates and records the limits of the current corpus. `audit_corpus.py --output <report.json>`
reproduces coverage checks on the downloaded local tables without exporting identifiers. Kaggle
downloads use the optional `requirements-kaggle.txt`; authentication stays outside the repository.

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
   enemy in the game, shrunk 30 games toward the base). Champion-roles under 200 games first use matching
   op.gg matchup data when available, then the role's table. This is association, not estimated causal win gain.
7. `export_cs.py <MatchTimelineTbl.csv> <players.parquet> <champion.json> <out.json>`: median CS at 10 minutes and
   CS per minute per champion and role (`data/pack/cs_bench.json`, read by `core/src/csbench.rs` for the post-game
   recap). Master+ ADCs: 78 CS at 10, 8.0 per minute; Xayah 80 and 8.7.
8. `export_answers.py <MatchTimelineTbl.csv> <players.parquet> <out.json>`: per answer (anti-heal, cleanse,
   anti-burst) and enemy champion, the odds ratio of owning one of the answer's items (logistic regression on role +
   the five enemies) and a 0-1 weight (`data/pack/answers.json`, read by `core/src/answers.rs` in place of the
   healing, suppression and assassin/burst tags). Anti-heal: Soraka 6.8x, Zac 2.6x, Garen 0.8x. Cleanse: Malzahar
   12x, Lissandra 5.5x (untagged), Warwick 2.1x. Anti-burst: Zed 1.75x, Syndra 1.05x. Tanks (armor pen ~1.0x) and
   poke (ADC boots unchanged by poke) showed no reaction worth modelling.

Re-run per patch: patches 16.17-16.18 count double (their shop equals 16.19's), 16.13-16.16 half. When the shop
changes, rebuild with the new patch data and re-check the item diff first.

## Full planner evaluation

Keep the environment and generated cases outside the repository (they contain corpus identifiers):

```bash
python3 -m venv ~/data/recall/.venv
~/data/recall/.venv/bin/pip install -r tools/priors/requirements.txt
~/data/recall/.venv/bin/python tools/priors/backtest.py --output ~/data/recall/evaluation/before.json
# Make a planner change, then run the same command against the saved report:
~/data/recall/.venv/bin/python tools/priors/backtest.py --baseline ~/data/recall/evaluation/before.json
```

The command builds and calls the production Rust planner, carries its preferences across each
game, and uses the same legality validator as recorded-game `replay`. Every invocation prints a
comparison table. With no baseline, both columns describe the current run. Exit 1 means an invalid
recommendation was found; exit 2 from the Rust runner means an input error. A nonzero result is not
silently accepted as a passing comparison.

The initial run generates cached cases and training artifacts; later engine-only changes reuse
them. Defaults read `~/data/recall/derived/{dec,players}.parquet`, the Kaggle timeline under
`~/data/recall/kaggle/ranked-timeline`, and the local 16.19.1 Data Dragon cache. `--data`, `--timeline`,
`--catalog` and `--cache` override paths. `--limit-games` and `--roles` are debugging filters;
leave them unset for the full benchmark. `--prepare-only` builds inputs without running the planner.

Player identity hashes assign 70%/15%/15% to training/validation/test. Matches containing tracked
players in different splits are excluded entirely. All learned inputs—next-legendary counts,
answer weights, boots, and provider-shaped purchase aggregates—are rebuilt from training games.
The evaluation-only Cargo feature permits explicit one-time installation of those tables; the
Windows app retains embedded production data and does not enable the feature. No evaluation
player's purchases are used to supply their fallback build.

Use validation for choosing changes. `--split test` reserves a separate player group for the final
check. The JSON report contains denominators, source hashes, preparation parameters, excluded games,
model hashes, code revision and tracked diff hash. It rejects comparison reports with different
input fingerprints or splits. If preparation logic or exporters change, archive the evaluator
binary before editing and use `--baseline-bin /path/to/old-backtest` to run both versions on the
identical new inputs. The binary must use the same evaluation protocol.

Metrics are reported overall, per role and for Xayah:

- `start/target_hits`: the displayed target matches the next observed completed item at the
  previous completion's minute (first decision starts at minute 6 or earlier).
- `purchase/target_hits`: the same agreement at the last minute frame before completion.
- `path_hits`: the first unowned legendary on the displayed path matches the next legendary.
- `path_top3_hits`: the actual next legendary occurs within three future path slots. This is
  sequence coverage, **not** top-three alternative classification accuracy.
- `boots_hits`: the planned boots match the first recorded tier-two boots.
- `flips/transitions`: target changes across consecutive minute frames with identical inventory.
  A target change can be justified by gold or state changes; this is a diagnostic, not inherently a bug.
- `repeat_flips/frames`: target changes when the identical state is immediately repeated with the
  returned preferences. `repeat_path_flips` similarly checks the whole path.
- `invalid_frames`: path incompatibilities, false ownership, wrong prices or unaffordable purchases,
  using replay's validator. `missing_target_candidates` counts any unscored future legendary in the
  path, including later items intentionally excluded by eligibility rules; it is not an error count.
- `affordable_detours`: frames offering an affordable anti-heal/cleanse component. Component purchase
  agreement is not scored: the derived decision table contains boots/legendary completions only.

Ambiguous same-horizon/type labels and labels already in inventory are excluded and counted. Frame
generation begins at minute 6. This skips some early decisions; denominators, not the source row
count, describe actual coverage. Inventories are compact lists, not Live Client slot positions;
trinkets are assigned slot 6 and real equipment to bag slots. Overfull inventories (more than six
non-trinkets, sometimes including consumed elixirs) are omitted and counted, rather than silently
hiding real equipment in the trinket slot. Previously confirmed ADC boots completions can restore
quest-hidden boots to slot 9. This inference can also retain boots after an unobserved ADC boot sale,
so the report includes its frequency. Other roles do not receive inferred hidden boots.

**Limits:** snapshots have one-minute cadence and track only some participants. Enemy names come
from the draft; enemy equipment is supplied only when that participant has an exact-time observed
frame. Missing equipment, kill feeds, complete rune pages and ability ranks are not fabricated.
The planner's own missing-data fallbacks still apply. Historical op.gg payloads are unavailable,
so training purchase aggregates supply its interface. Skill advice, rune choices and sub-minute
flips are outside this benchmark. Use real recordings for those integration checks. The default
scoring patches are 16.17–16.18 with the compatible 16.19 catalog. This measures imitation and
runtime constraints; it does not establish win-rate improvement or optimal detour timing.

## Corpus boots

`export_boots.py <dec.parquet> <champion_traits.json> <out.json>` exports the first confirmed
tier-two boots per player-game. Its champion/role prior is adjusted by observed role-level enemy
magic-count frequencies. Sparse composition buckets shrink toward the role prior; champion-roles
with fewer than 30 observed boot purchases keep the existing fallback. Phantom completions are
excluded. The live reader is `core/src/bootsprior.rs`; `data/pack/boots.json` is embedded and needs
a rebuild. Swiftplay and incomplete enemy drafts retain their existing provider/fit behavior.

The model uses draft damage tags, not inferred enemy builds or lane-specific damage. Its smoothing
and sample floors remain policy parameters; further changes require validation against the full
planner. The export does not prove that its most common boots are optimal in every matchup.
