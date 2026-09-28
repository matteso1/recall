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

## Complete Match-v5 collection

`discover_matches.py`, `collect_matches.py` and `audit_matches.py` are stdlib-only. They collect
paired official match/timeline responses for research outside the model pack. Full rune pages,
summoner spells, skill-up events and ten-player minute frames are retained. Production models
still use the earlier corpus until event reconstruction and a new held-out evaluation are verified.

The local development credential is read from `~/.config/recall/riot-api-key` or `RIOT_API_KEY`.
Keep the file private (mode 600). It is sent only in a header to the selected Riot API host,
never included in a command-line key, raw response, model export or app build. Development keys
expire; renewal replaces the local file and the same collection command resumes its cache.

```bash
python3 tools/priors/discover_matches.py --platform na1 --players 24 \
  --matches-per-player 20 --days 7 --output ~/data/recall/riot/seeds/current-na1.txt
python3 tools/priors/collect_matches.py \
  --match-ids ~/data/recall/riot/seeds/current-na1.txt --region americas \
  --patches 16.19 --limit 500
python3 tools/priors/audit_matches.py --output ~/data/recall/riot/coverage.json
```

For EUW use `--platform euw1`, `--region europe` and a separate seed file. Use one collector per
regional route; the development rate budget is shared on that route. Discovery samples the current
Master, Grandmaster and Challenger ladders evenly, fetches ranked-solo histories within the time
window, deduplicates and shuffles match IDs, and saves the cohort/rank provenance in a private
sidecar. This verifies the **seed player's rank at collection**, not all ten participants' ranks
or their historical ranks. A seed cohort is not a uniform sample of all ranked matches.

Collection examines a bounded number of seeds, filters queue 420 and explicit internal gameVersion
patches before fetching timelines, and spaces requests by at least 1.25 seconds. It honors
`Retry-After`, bounds retries, stops on authentication failures and does not follow redirects.
Run the same command after an interruption: valid cached match responses are reused; cached
timelines are validated again. A 404 is counted as unavailable. A newly downloaded timeline
that fails the corpus contract is counted as rejected; its raw response and reason are saved
separately as `rejected-timeline.json` / `rejected.json`, and the batch continues. It never gets
a `complete.json` marker. A later invocation retries it. Authentication/network failures,
malformed cached JSON and cached timelines failing validation preserve progress and stop.

Each private match folder holds `match.json`, `timeline.json` and a `complete.json` manifest with
source hashes and coverage counts. Validation requires matching IDs and identities, ten unique
player slots, five players per team, all ten numeric player frames, chronological minute cadence,
an initial frame and a final `GAME_END` event. Exact purchase/sale/undo event payloads are kept;
the collector does not reconstruct inventories by guessing from final builds. Files are written
atomically with private permissions, and the commands reject raw output inside Git checkouts.
The audit checks hashes and emits counts only, so its report may be committed without raw IDs.

Tests: `~/data/recall/.venv/bin/python -m unittest discover -s tools/priors -p 'test_*.py' -v`.
Collector tests use fictional responses and injected HTTP; no network or real key is required.
The [collection report](../../docs/notes/riot-collection.md) records the real pilot and limitations.

API contracts: [Match-v5](https://developer.riotgames.com/apis#match-v5),
[League-v4](https://developer.riotgames.com/apis#league-v4), and
[Riot portal documentation](https://developer.riotgames.com/docs/portal).

## Reconstructed Riot timeline evaluation

The paired pilot now has a separate evaluator. It runs the actual Rust planner with isolated
training artifacts; it does **not** change the embedded production pack. Both commands work
entirely offline once pairs, seed sidecars and patch catalogs have been collected:

```bash
# Strict diagnostic: all ten combat inventories must be known at an observation.
~/data/recall/.venv/bin/python tools/priors/backtest.py --source riot \
  --output ~/data/recall/evaluation/riot-validation.json

# Broader diagnostic: own inventory exact, only reconstructable peers supplied.
~/data/recall/.venv/bin/python tools/priors/backtest.py --source riot \
  --inventory-policy known-peers \
  --output ~/data/recall/evaluation/riot-known-peers-validation.json

# After an engine change, compare the same cases and training artifacts.
~/data/recall/.venv/bin/python tools/priors/backtest.py --source riot \
  --inventory-policy known-peers \
  --baseline ~/data/recall/evaluation/riot-known-peers-validation.json \
  --output ~/data/recall/evaluation/riot-known-peers-after.json
```

`riot_inventory.py` applies timestamped purchases, component destruction, sales and undo. Undo
restores only the components actually consumed by that purchase; unrelated same-time consumption
is not reversed. Supported free rune grants and role-quest boots transitions are explicit.
Final `item0`–`item6` and equipment in `roleBoundItem` validate the result, never fill past frames.
Final slots expose stack presence, not stack quantities. Trinkets and recall/quest tokens are
outside the combat-inventory contract. Unknown item events and unexplained final differences
exclude the entire affected history. Viego possession histories are currently unsupported.

Some support grants/choices, tear transformations and automatic skill-elixir consumption lack
reliable event times. Their possible identities remain explicit uncertainties. A compatible final
inventory does not make those earlier frames exact. `exact-team` excludes any such observation;
`known-peers` excludes it when it affects the active player, otherwise omits that peer's entire
inventory/player observation. The five draft champion names remain available. Missing peers are
counted, never represented as champions with empty inventories. Overfull bags are excluded.

`riot_corpus.py` uses the saved `riot/seeds/current-*.json` cohort and a fixed PUUID hash. A held-out
seed's presence excludes the **whole match** from training, even if that seed is only contextual.
Mixed validation/test seed matches are excluded. Only the held-out seeds are evaluation targets.
Non-seed contextual players can recur across partitions; this is target-player and match isolation,
not a claim that every contextual participant is disjoint. Only seed ranks at collection are verified.

The first protocol trains/evaluates patch 16.19; the additional 16.18 pairs are audited with their
own catalog. All priors and provider-shaped aggregates train on eligible training histories.
Champion/role and answer sample floors remain unchanged, so sparse tables use the existing fallback.
No old-corpus learned artifact is injected into this evaluation: its pseudonymous player IDs have
no PUUID bridge proving disjointness. Next-item exports weight 16.19 at 1.0, as for 16.17–16.18.

Frames contain own observed gold, runes, spells, learned skill ranks and public kill/death/assist
history. Enemy positions, exact gold, combat stats and damage totals are not exposed to the planner.
Own combat stats are currently omitted as well. Physical inventory slots are compacted except for
known ADC role-slot boots. A future purchase label must be strictly after the observation and no
later than the following frame; undone purchases are excluded. Multiple completions of the same
kind, or tied first purchases, are counted as ambiguous rather than scored individually.

Additional report metrics:

- `all/shop/buy_hits / buy_decisions`: affordable buy-now item matches the next retained purchase.
  Includes consumables; duplicate component buys may match even when a copy is already owned.
- `component_hits / component_decisions`: the same comparison for purchases classified as unfinished
  equipment. Boots and legendary completion agreement keep their separate denominators.
- `antiheal_*` and `cleanse_*`: observed following-minute shopping windows, with positive purchases,
  negative purchases, suggested answers, true positives and false positives. A window is not proof
  of shop access or equal gold at observation time. An unobserved purchase does not prove bad advice.
- Minute stability accepts 59–61 second spacing for Riot timestamp jitter. Repeated-state stability
  still passes the identical snapshot back to the planner and should not change its target/path.

Caches default to `~/data/recall/evaluation/riot-cache` and `riot-known-peers-cache`. Raw response
hashes are verified on every run. Code, catalogs, seed sidecars, policy, filters and artifact hashes
protect comparisons; different fingerprints are rejected. Private caches are rejected inside Git.
Preparation streams matches instead of retaining all raw timelines in memory. `--prepare-only`
creates the cache/audit; `--split test` is reserved for final checks after choosing changes on
validation. The first pilot scored validation only.

See the [reconstruction report](../../docs/notes/riot-reconstruction.md) and its aggregate JSON
for measured coverage, exclusions and the deliberately limited conclusions.
