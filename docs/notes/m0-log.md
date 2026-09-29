# M0 log

## 2026-09-05
- Repo created at `~/code/recall` (WSL). Design doc v0.1 committed verbatim.
- Environment: WSL2 NAT mode; League client running on Windows. Direct 127.0.0.1 from WSL
  refused as expected; Windows `curl.exe` via interop reaches the LCU in ~50 ms.
- LCU verified live: gameflow phase `Lobby`, summoner matteso#NA1, one existing item set
  (`OP.GG Xayah`) read back. Dumped `/help` to confirm item-set and gameflow schemas.
- Data Dragon 16.17.1: all names in `data/itemsets/xayah.json` resolve. Notable: the
  string is `B. F. Sword`; Essence Reaver builds from Sheen + Caulfield's + Cloak; two
  entries are named "The Collector" (6676 is the shop one).
- Wrote `.wslconfig` with mirrored networking; not yet activated (needs `wsl --shutdown`).
- Pending real-game verification: champ select watcher and live poller (unit-tested on
  hand-written fixtures only).
- `push_itemset.py` run for real: `Recall Xayah` (14 blocks) written next to `OP.GG Xayah`
  and read back from the client. `push_itemset.py --remove` undoes it.
- 30 unit tests green (`python3 -m unittest discover -s m0/tests`). Doctor: LCU pipe OK.
- Champ select fixture field names checked against `/help` types `ChampSelectSession`,
  `ChampSelectPlayerSelection`, `ChampSelectTimer`, `LolChampSelectLegacyChampSelectAction`.
- 12:35 `wsl.exe --shutdown` run to activate mirrored networking (at the user's request).
  Next session: confirm `wslinfo --networking-mode` = mirrored and re-run the doctor.
- 12:41 WSL back up in mirrored mode. Doctor: transport direct, LCU OK without curl.exe.
  Watchers restarted in the background (`--dump m0/tests/fixtures/captured`).
- 12:44-12:47 first Practice Tool game with both watchers running (direct transport):
  champ select captured hover -> lock -> FINALIZATION -> GAME_STARTING; live poller saw gold
  ticking, a Q point and a Biscuit purchase. Scrubbed copies are now fixtures
  (`*_practicetool*.json`, raw `--dump` output is gitignored: it carries a chat JWT).
- Shop feedback: block titles were cut off. Titles now <= 30 chars (`MAX_BLOCK_TITLE`), the
  per-item "why" stays in the spec for the overlay. Set re-pushed; check in the next game.
- op.gg desktop app autostarts from `HKCU\...\Run` (`electron.app.OP.GG`) and used 1.8 GB RAM
  across 9 processes during the game; Overwolf autostarts too. Neither is ours.
- User confirmed the re-pushed set renders correctly in the shop ("item sets look great").
  **M0 complete.**
- At the user's request the op.gg desktop app (2.5.5) and Overwolf (0.309) were uninstalled
  via their registered uninstallers (Overwolf's needed a UAC elevation through PowerShell
  `Start-Process -Verb RunAs`; WSL interop cannot launch elevated processes directly).
  Dangling `Run` entry and leftover AppData folders removed. Backup of the Run key was not
  taken (the entries pointed at now-deleted executables). The account's "OP.GG Xayah" item
  set is untouched; `push_itemset.py --remove-title 'OP.GG Xayah'` removes it if wanted.

## 2026-09-27 — Full planner evaluation and sparse-champion bans

- Added one-command, player- and match-disjoint full-planner evaluation with training-only inputs,
  before/after tables, stability diagnostics and shared replay legality checks. Python dependencies
  now have a persistent, pinned environment. Raw cases stay outside Git.
- Added corpus boot selection, repaired missing learned purchase candidates, and connected cached
  champion matchup data to ban advice when the Master+ champion sample is too small (including Nilah).
- Compared 7,970 held-out games / 181,106 observations: zero invalid recommendations. Separate test
  boot agreement improved 54.13% → 65.34%, next-buy agreement 50.80% → 51.88%; first-legendary path
  agreement fell 0.66 points. Full findings, remaining constants and limits: `engine-review.md`.
- 309 core tests, 39 runtime tests, 23 UI tests, five evaluator contracts, Clippy and Windows checks
  pass. Ten local recorded games replay 1,660 observations without invalid advice.
- Installed through `overlay-update.sh` while League was in Lobby; the Windows release build and
  headless catalog probe passed, and one canonical Recall process relaunched.

## 2026-09-27 — Richer Kaggle source audit

- Installed the authenticated Kaggle client in the persistent research environment and screened
  newer timeline/full-match sources. Confirmed the Recall GitHub repository is public.
- Downloaded and profiled 2,108,090 ten-player snapshots (39,512 matches with frames) and a separate
  1,087-match full-rune sample. Snapshot age, missing stable identities, coarse cadence and the
  published collector's undo handling prevent treating it as a drop-in training upgrade.
- Added a reproducible aggregate-only audit and corrected the scope of the earlier 1.2% coverage
  statement: raw recent-patch opponent-frame coverage is 14.14% before evaluation exclusions.
  Findings and next data requirements: `data-source-audit.md`. Production models/app unchanged.

## 2026-09-28 — Planner stability

- Separated conditional-model sequence memory from the reordered display path. A small failing
  regression reproduces the previous feedback loop; no weights were changed.
- Same-input comparisons over 181,106 observations remove all 18 repeated-state target changes
  and 378 path changes, with zero invalid recommendations before or after. Purchase agreement
  remains effectively unchanged. Full denominators and limits: `planner-stability.md`.
- 310 core tests with evaluation enabled, 39 runtime tests, Clippy and Windows type-check pass.
  Recorded-game replay is legal but sparse legacy captures can miss hidden-boot transitions.

## 2026-09-28 — Update guard

- Fixed the updater treating any failed client API request as `NoClient`. It now requires a known
  idle phase and a successful process check, or verified absence of both client and game. Unknown
  states wait; a running game overrides a stale Lobby response. Process command lines are not read.
- All 39 Python probe tests pass, including five guard contracts; shell syntax check passes. The
  live read-only check correctly holds in champion select. No forced in-game restart was performed.

## 2026-09-28 — Faithful hidden-boot recording

- Reviewing a completed game exposed a replay-only divergence: 77 gold earned between two saved
  observations obscured a 300-gold hidden-boot upgrade. The live panel had correctly moved to the
  next legendary, while replay still wanted boots. No live item rule was changed for this game.
- Record the live role-slot result (including explicit absence) and trigger a line when it changes.
  Replay restores that result after validating the item; legacy files retain best-effort inference.
- The reproduction failed before the fix. 311 core tests and 40 runtime tests pass, including
  sparse-observation restoration and recording a hidden upgrade without a visible bag change.
  Core/runtime Clippy and Windows release type-check pass.

## 2026-09-28 — Official match/timeline pilot

- Added bounded ladder discovery, paired Match-v5 collection and a reproducible aggregate audit.
  Credentials, player identities and raw responses stay outside Git; source manifests carry hashes.
- Collected 308 NA and 254 EUW patch-16.19 matches from a 48-player current Master+ seed cohort,
  plus 100 historical patch-16.18 pairs. All pairs validated ten-player minute coverage; all 6,620
  player records include complete runes, spells and roles. Rank is only verified for seed players.
- Full offline resumption reused all 562 current-patch pairs without network requests; 130
  out-of-patch exclusions were reproduced. 18 priors/collector tests pass. The fresh data is
  preserved for reconstruction/evaluation and has not replaced production weights.
- The final recording pass covers 12 games / 1,819 observations with zero shop-legality violations.
  The last completed game's exact post-match record also confirms the known ten-CS quantization
  of the live recap; keep private game reviews and raw IDs outside Git.

## 2026-09-28 — Verified Windows installation

- The previously queued updater was no longer running when the next game ended; it had not
  installed the changes. Reran the guarded update while League was in Lobby and monitored it
  through completion. Both stop and relaunch occurred in Lobby.
- Release code `44d355b` built successfully in 1m38s. The headless probe was refreshed from the
  new executable; one process runs from the canonical `recall-win` release path. Artifact SHA-256:
  `5eb55257c645b21dd3666b12da3eede191cd1cb323dba22018b8a819cf4d38b4`.
- One additional completed-game replay covers 223 observations without invalid recommendations
  or missing aggregates, bringing reviewed recordings to 13 / 2,042 observations. This is a
  legality check, not a claim of optimal advice. Individual game reviews remain private.

## 2026-09-28 — Reconstructed Riot evaluation foundation

- Added causal purchase/sale/undo reconstruction, supported rune/quest transitions, explicit
  uncertainties and final combat-inventory reconciliation. Audited all 662 pairs / 6,620 players:
  6,544 final inventories compatible, 6,427 histories without unexplained events/mismatches.
- Added `backtest.py --source riot`, isolated train/validation/test seed and match handling,
  training-only prior exports, causal runes/skills/kill-feed inputs, chronological purchase labels,
  component agreement and positive/negative answer shopping windows. No final-item backfill.
- Strict full-team validation: 29 games / 106 observations. Optional `known-peers`: 33 / 566,
  80.5% observed opponent coverage. Both report zero invalid recommendations and zero changes
  on repeated identical states; repeated runs reproduce the same metrics. Test split unscored.
- Sparse coverage (20 next-item champion-roles; no answer table reaches its sample floor) and only
  one observed anti-heal/cleanse purchase each prohibit tuning conclusions. Full details and
  aggregate results: `docs/notes/riot-reconstruction.md`.
- 37 priors Python tests, 313 core tests and Clippy pass; legacy answer export reproduced exactly.
  Offline tooling only: no production models changed and no overlay restart performed.

## 2026-09-28 — Expanded evaluation and defensive-model experiment

- Recovered skill-elixir consumption from spent bonus-point evidence, excluded ability evolution
  from ordinary ranks, and preserved observed support ward slots. On the fixed original pilot,
  recovered 3,364 exact player observations with no new unexplained events; known opponent
  coverage rises from 80.5% to 84.7%. Earlier uncertain frames are not filled from later evidence.
- Expanded to 1,556 validated pairs / 150 ladder seeds. Rejected one final-frame cadence outlier;
  the collector now quarantines fresh rejected timelines and continues its bounded batch, while
  still stopping on API/authentication or cached-corruption failures.
- Added immutable private dataset snapshots. Expanded validation covers 210 player-games / 3,306
  observations, with zero illegal recommendations or identical-state changes. Repeated runs give
  identical metrics and fingerprints. Only seven anti-heal/three cleanse purchase windows remain.
- Added reproducible defensive-answer model comparison on 4,257 held-out games / 157 players.
  Own-champion/patch controls improve ownership prediction; role interactions show no clear gain.
  Tested the controlled weights through the planner on 3,924 games / 88,959 identical observations:
  four additional next-completion matches, one additional legendary-path match, no legality or
  repeated-state regressions. Large weight changes are not justified by that tiny benefit; no
  production model was promoted and no test partition was scored.
- 48 priors Python tests, 313 core tests with evaluation enabled and core Clippy pass. All work is
  offline; no installed executable change or overlay restart. Reports: `riot-inventory-recovery.md`,
  `riot-expanded-evaluation.md`, `answer-model-evaluation.md` and their aggregate JSON files.

## 2026-09-28 — Standard-mode product priority

- Owner clarified that normal Draft Pick should take priority over casual Swiftplay tuning.
  Standard Summoner's Rift is the primary quality target, with ranked as the longer-term goal.
  Swiftplay retains compatibility support and can still expose shared implementation bugs.
- Verified the current separation: Swiftplay bypasses the learned item-sequence branch;
  official corpus collection/evaluation requires ranked-solo queue 420. Keep normal draft,
  ranked and Swiftplay evidence distinct rather than relabelling ranked scores as draft results.
- Priorities: coherent core/boots progression, justified defensive detours, reliable draft
  imports, and useful explanations. Updated the design and session guidance; corrected the
  design's stale statement that no fitted models existed. Documentation only, no app restart.

## 2026-09-28 — Kalista setup reminder and fallback review

- The Kalista recording retained Black Spear until 23:53 despite learning R at 5:18. Added a
  main-panel reminder based on current inventory; it also works while build data loads and
  hides when the item is gone or live data is stale. No bond/range inference or gameplay input.
- Reproduced the fallback planner promoting Jak'Sho and Randuin's ahead of Rageblade after
  deaths, despite the provider's damage core. All 184 observations were legal: legality did
  not detect the build-quality problem. No item weights/order changed in this update.
- `kalista-review.md` records the sanitized evidence and the broader fallback evaluation
  needed before changing promotion policy; the standard-mode priority still applies.
- Passed 314 core, 40 runtime and 24 browser tests, both Clippy targets and the Windows check.
  Replay kept all 184 purchase/path results unchanged; the reminder appeared on the 166
  observations where Black Spear was present. No legality violations before or after.
