# Recovering observed inventory evidence — September 28, 2026

Two omitted pieces of timeline evidence reduced the first pilot's usable coverage:
spent bonus skill points and support control wards in the role quest slot. These fixes
affect offline reconstruction only; they do not change live advice or production models.

Triple Tonic's level-nine elixir grants a skill point, but its consumption event is
sometimes absent. Spending more ordinary skill points than the player's current level
proves that the elixir was consumed. The ledger now resolves that uncertainty from the
proof's timestamp onward. Earlier observations remain uncertain. Events at the same
timestamp are considered together, and an explicit consumption in that batch is not
double counted. Aphelios, Viego and histories with a nonstandard early skill budget are
excluded from this inference. The rune's mechanics are documented in Riot's
[14.1 patch notes](https://www.leagueoflegends.com/en-sg/news/game-updates/patch-14-1-notes/).

`SKILL_LEVEL_UP` events with `levelUpType=EVOLVE` improve an ability without spending an
ordinary rank. They previously inflated reconstructed ability ranks. The pilot contains
727 such events: Viktor 390, Kha'Zix 177 and Kai'Sa 160. Both the inventory proof and the
planner adapter now ignore these for ordinary rank counting.

An observed support ward/quest-token refresh identifies the separate ward slot. The
adapter now preserves six ordinary equipment slots alongside that ward. The exact 16.19.1
Data Dragon catalog's support quest text confirms the slot behavior; final inventory is
still validation-only and never fills past state.

## Fixed-cohort audit

The comparison uses the original 662 match pairs, not the concurrently expanding corpus.

| Check | Before | After |
|---|---:|---:|
| Player histories ending fully resolved | 4,950 | 5,159 |
| Exact minute player inventories | 148,511 | 151,875 |
| Histories with an unexplained event or final mismatch | 193 | 193 |
| Known opponent inventories in planner validation | 2,279 / 2,830 (80.5%) | 2,474 / 2,920 (84.7%) |
| Evaluated games / observations | 33 / 566 | 33 / 584 |
| Observations with all ten inventories | 106 | 134 |
| Invalid recommendations | 0 | 0 |
| Identical-state target / path changes | 0 / 0 | 0 / 0 |

There are 239 bonus-point consumption proofs. All previously exact player observations
retain identical equipment; there are no new unexplained events. The four fitted training
artifacts are byte-identical to the earlier pilot. A second recovered validation run
reproduces all metrics and its input fingerprint, while collection continues separately.
The changed observation set means these rates are coverage measurements, not a model
improvement comparison. Only one anti-heal and one cleanse purchase window remain: this
pilot still cannot calibrate answer timing.

Regression contracts cover causal resolution, evolution events, same-timestamp consumption,
nonstandard skill budgets, six bag slots plus a ward, and no final-state backfill. The
aggregate [inventory audit](riot-inventory-recovery.json) contains no player identifiers.
