# Expanded frozen Riot evaluation — September 28, 2026

The official store now contains 1,556 validated match/timeline pairs: 1,456 from patch
16.19 and 100 older 16.18 pairs retained for inventory audits. The combined current ladder
seed cohort has 150 players. Two bounded collections examined 600 seeds per region:

| Region | Valid current-patch pairs encountered | Other queue/patch | Rejected timeline |
|---|---:|---:|---:|
| Americas | 526 | 73 | 1 |
| Europe | 555 | 45 | 0 |

These counts include cache hits and overlap the earlier pilot. The rejected timeline has
a 62.373-second final interval, outside the collector's minute-cadence tolerance. It is
quarantined for inspection, not called corrupt and not admitted by relaxing validation.
The collector now continues to other seeds after rejecting a newly downloaded timeline;
cached corruption and API/authentication failures still stop the run.

## Fixed membership and splits

`riot_dataset.py` freezes pair membership, raw response hashes and the saved ladder cohort
in a private, non-overwritable snapshot. `backtest.py --source riot --dataset FILE` uses
those captured inputs even if more pairs or ladder sidecars arrive later. It checks raw
hashes on every run, validates match IDs against folder IDs and requires ranked-solo
responses with the recorded patch. No identities or raw responses are committed.

The expanded cohort assigns 96 seed players to training, 27 to validation and 27 to test.
Current-patch matches split into 816 training, 310 validation and 308 test, with 22 mixed
validation/test seed matches excluded. Evaluation target players and their matches never
train the model. Non-seed contextual players can recur across groups. Seed rank is verified
at collection; this is not a claim that every participant is Master+.

Training supplies 7,921 usable player-games from 4,147 players, with 27,566 legendary/boots
decisions. Next-item tables cover 88 champion-roles, boots cover 75, and 12 enemy champions
reach the unchanged 500-appearance floor for each answer table. The original pilot covered
20, 15 and zero respectively. The fresh model pack remains evaluation-only.

## Expanded validation

| Check | Result |
|---|---:|
| Target player-games before exclusions | 322 |
| Unsupported champion-role games | 109 |
| Evaluated player-games / observations | 210 / 3,306 |
| Known opponent inventories | 13,867 / 16,530 (83.9%) |
| Observations with all ten inventories | 796 |
| Invalid recommendations | 0 |
| Identical-state target / path changes | 0 / 0 |
| Next legendary on path agrees | 243 / 404 (60.1%) |
| Boots agree | 80 / 146 (54.8%) |
| Next purchase agrees | 346 / 1,437 (24.1%) |
| Next component agrees | 220 / 863 (25.5%) |

The expanded observations, split membership and fitted models differ from the original
pilot. Their agreement rates must not be presented as an engine improvement. A repeated
expanded validation run gives identical metrics and fingerprints. The test split remains
unscored. The full [aggregate report](riot-expanded-evaluation.json) preserves denominators,
exclusions, model hashes and limitations.

There are seven observed anti-heal and three cleanse shopping windows. The planner suggests
anti-heal in three of those seven and in 29 of the 1,430 negative windows; it suggests no
cleanse in these windows. An unobserved purchase is not proof of poor advice, and minute
cadence does not establish shop access or exact purchase-time gold. These small positive
counts still cannot calibrate a detour gate. Xayah has just 11 evaluated observations and
one legendary completion; the legacy benchmark remains necessary for champion-specific work.

Inventory audit: 15,372/15,560 final bags are compatible; 15,101 histories have no unexplained
events/final mismatch; 12,150 also finish fully resolved. The 436,900 minute player frames
include 360,145 causally exact inventories. Unsupported possession histories, missing item
events and unresolved transformations remain explicit exclusions. See the earlier
[recovery report](riot-inventory-recovery.md) for the fixed-cohort inventory improvement.

The new Python contracts and the actual planner validation cover the offline changes.
No embedded production data, app-facing Rust code or installed executable changed.
