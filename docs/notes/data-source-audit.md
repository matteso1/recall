# Richer match data — September 27, 2026

Recall is public at [matteso1/recall](https://github.com/matteso1/recall), verified through
GitHub's API. Kaggle 2.2.4 is installed in the persistent research environment and authenticates
with the owner's existing local token. Raw downloads, credentials and player identifiers remain
outside this repository. This investigation does not change the running overlay or its models.

## What the current coverage number means

The earlier approximately 1.2% enemy-inventory figure describes **held-out evaluator cases after
player/match exclusions**, not the original dataset. The raw derived corpus covers:

| Scope, minute 6 onward | Observed enemy-frame pairs | Possible pairs | Coverage |
|---|---:|---:|---:|
| All derived patches, 16.13–16.18 | 1,297,602 | 12,196,710 | 10.64% |
| Evaluation patches, 16.17–16.18, before exclusions | 1,169,168 | 8,270,440 | 14.14% |

A focal player-minute has five possible opponent observations. For an exact match-minute with
`b` observed blue players and `r` observed red players, the numerator is `2*b*r` and denominator
`5*(b+r)`. This measures observed rows, not correctness of reconstructed item ownership.

The evaluator currently excludes every match containing tracked players assigned to different
splits. That substantially reduces opponent coverage. Do not reuse that rule unchanged for a
fully observed ten-player dataset: independently assigning players 70/15/15 would retain roughly
`0.7^10` of such matches for training and only `0.15^10` for each evaluation split, under the
independent-player approximation. Design evaluation around focal players and match separation;
document whether opponent identities may occur as context across splits. Strict separation of
every participant is a different, much harder constraint in a connected matchmaking population.

## Kaggle search and actual downloads

Searched six pages of the newest “league of legends” results plus targeted searches for LoL,
timelines, item events, Match-v5 and 2026. Inspected candidate descriptions and file manifests;
downloaded the strongest snapshot source, one full-match source, and two small raw-match samples.
This is a screened search, not a claim that every Kaggle dataset was inspected.

| Source | Evidence inspected | Useful content | Main limitation |
|---|---|---|---|
| [Current ranked timeline](https://www.kaggle.com/datasets/nathansmallcalder/league-of-legends-ranked-post-match-and-timeline) | Existing local CSVs and derived tables; MIT listed | Recent Master+ purchases and one-minute focal-player state | Sparse opponent observations; inventory diffs rather than preserved raw events |
| [Match interval snapshots](https://www.kaggle.com/datasets/nathansmallcalder/league-of-legends-match-interval-snapshots-2026) | Downloaded all three match/participant/interval tables | 39,512 matches with ten-player checkpoints; levels, KDA, equipment, objectives | Mostly 16.1/16.2, five-minute cadence, no stable player identity, inventory reconstruction concern; license listed as “other” without terms in description |
| [GM/Chall EUW + esports](https://www.kaggle.com/datasets/bwifterino/1-day-gmchall-euw-soloq-league-of-legends) | Downloaded the solo-queue JSON | 1,087 matches / 10,870 participants, all patch 16.3 and queue 420; complete rune pages and final match fields; CC0 listed | No timeline frames; cannot recover purchase or skill-level order |
| [Raw EUW matches](https://www.kaggle.com/datasets/omaracornejo/matches-raw-euw) | First 6,000 file entries and two downloaded match samples | Samples have ten-player Match-v5 results on 16.1; MIT listed | Samples have no timeline frames; listing is incomplete, so absence across the entire package is not established |
| [Grandmaster Worlds patch](https://www.kaggle.com/datasets/krzsztfwtk/lol-grandmaster-soloq-matches-worlds-2025-patch) | Description and manifest | Publisher reports 92,105 matches on 25.20 in Parquet | Described as end-of-game Match-v5 data; old patch and unspecified license |
| [Patch 25.14 ranked games](https://www.kaggle.com/datasets/californianbill/patch-25-14-lol-league-of-legends-ranked-games) | Description and manifest | Publisher reports 101,843 Platinum+ NA matches; CC0 listed | Raw JSONL is described as getMatch responses, not timelines; 8.2 GB uncompressed |
| [Summoners and match data](https://www.kaggle.com/datasets/chiniczr/league-of-legends-summoners-and-match-data) | Description and manifest | Publisher reports 260,367 matches and final match statistics; MIT listed | Updated in 2023; description explicitly says not timeline data |
| [Fine-grained datasets](https://www.kaggle.com/datasets/walagooose/league-of-legends-lol-fine-grained-datasets) | Description and manifest | Pro match/statistics tables | Scraped op.gg source; not an independent live-inventory source |

### Verified snapshot source

The [aggregate report](data-source-audit.json) records file hashes and query results. The three
downloaded CSVs total approximately 307 MB uncompressed:

- 39,954 match rows, dated February 21, 2024 through January 30, 2026; all ranked solo queue.
  Of those, 31,603 are 16.2 and 6,639 are 16.1. Older games must be filtered explicitly.
- 399,540 participant rows, exactly ten per match. These are **player-game rows**, not evidence
  of 399,540 distinct people. The available columns contain no stable account identifier.
- 2,108,090 player snapshots across 39,512 matches and 210,809 match-time checkpoints. Every
  checkpoint joins to five blue and five red participants. All snapshot keys are unique and all
  seven item columns are populated, with zero used for empty entries.
- 24,705 of the matches with snapshots have the source's `average_rank` label Master,
  Grandmaster or Challenger. This does not establish that every participant is Master+.
- The correct join is `intervals.player_id = processed_summoner_data.id`, also matching
  `match_id`. `participant_id` is just the in-game 1–10 slot and is not the foreign key.

Coverage is not a guarantee of accurate inventory contents. The dataset links to a collector;
its [published version](https://github.com/NathanSmallcalder/LeagueFiveMinIntervals/blob/13ba4532e38795374836719f1268bbb2e2726de0/DataCollector.py#L56)
handles `ITEM_UNDO` by removing the last inventory entry, without using the undo's item IDs.
For an undone sale, that removes another owned item instead of restoring the sold one. It also
limits purchases to a seven-entry list, which is not equivalent to shop slots with stacks and
role-quest storage. The inspected code was published after the dataset update; the exact export
revision and prevalence of affected rows are unknown. These are reasons to validate against raw
events, not a measured estimate of corruption in the CSVs.

Five-minute snapshots also hide multiple purchases, sales and undos between checkpoints. They
can support coarse state research but cannot become exact next-purchase labels by assumption.
Observed levels extend to 20; do not reject everything above 18 using an obsolete level cap.

## Recommended use

1. Keep the recent corpus as the current production prior. Use the downloaded rich snapshots for
   schema/feature experiments and coarse contextual stress cases, with their age and reconstruction
   limitations explicit. Do not present a match/time split on this source as player-disjoint.
2. Obtain fresh **paired Match-v5 match and timeline responses** as the durable training source.
   Keep ten participants, minute frames, ordered purchase/sale/undo events, kills and skill-level
   events. The match response supplies runes, spells, draft, patch and stable participant identity.
   Retain stable identity privately for evaluation grouping; exclude names/IDs from model features
   and public artifacts. Preserve raw events so reconstruction can be corrected without recollecting.
3. Match training features to what Recall can actually observe at inference: own current gold,
   owned items, visible opponent equipment/levels/KDA, draft, role and elapsed time. Exact enemy
   gold/XP in a research table must not silently become required live inputs. Use only state
   available before each purchase; never use final builds, final damage or outcome as input features.
4. Compare a contextual purchase ranker with the current conditional-count baseline. Measure
   calibration, role/champion coverage, rare defensive purchase precision, legality, and stability
   separately. Expert purchase agreement remains imitation, not proof of win-rate improvement.
   Keep deterministic shop legality and state handling around any learned ranking model.

Riot documents the [Match-v5 API](https://developer.riotgames.com/apis#match-v5/GET_getTimeline)
and the [Live Client API](https://developer.riotgames.com/docs/lol#game-client-api_live-client-data-api).
Fresh collection needs a valid Riot API credential, separate from Kaggle authentication. It is
not started by this audit. The ordinary [personal key limit](https://developer.riotgames.com/docs/portal#web-apis_api-keys)
is 100 requests per two minutes: two requests per match is about 25 matches/minute before history
lookups, retries and other constraints. A large new corpus is an ongoing collection job, not a
GPU training task. No candidate verified here combines current-patch exact events, complete team
state and stable player IDs in a clearly reusable package.

## Reproduce

From the real WSL repository, using the saved Kaggle credential automatically (never paste it):

```bash
~/data/recall/.venv/bin/pip install -r tools/priors/requirements.txt
~/data/recall/.venv/bin/pip install -r tools/priors/requirements-kaggle.txt
~/data/recall/.venv/bin/kaggle datasets download \
  -d nathansmallcalder/league-of-legends-match-interval-snapshots-2026 \
  -p ~/data/recall/kaggle/intervals-2026 --unzip
~/data/recall/.venv/bin/python tools/priors/audit_corpus.py \
  --output ~/data/recall/kaggle-audit/latest.json
```

The audit reads the existing current corpus plus the three interval CSVs and outputs only
aggregate counts, field names and hashes. `--intervals`, `--players` and `--timeline` override
locations. It does not train models, alter the data pack or restart the app.
