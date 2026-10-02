# Live adaptation follow-up — October 2, 2026

The [recent-game review](recent-games-case-review.md) found three constraints that made the
learned build too rigid. This change adjusts those constraints without retraining the model.

## Policy changes

- Visible enemy armor/MR can use the full existing penetration-need score in the conditional
  sequence. Other needs retain the 0.6-nat ceiling; resistance evidence can reach 1.5 nats.
  The learned prior, component investment, affordability and hysteresis still apply.
- In covered standard-mode builds, two direct deaths within six minutes to the same current
  hunter permit an existing defensive promotion after one completed core. The hunter must
  also exceed the player's estimated strength without kill-feed weighting. Ordinary deaths,
  assists alone and composition alone do not unlock this exception. An existing answer has
  the previous three-minute hold; a uniquely invested component preserves its commitment
  while a relevant threat remains. An affordable previous target can still finish first.
- Buying another component after a detour offer defers the detour until the next completed
  non-boot item. This is a purchase-inferred deferral, not an explicit user rejection. Gold
  thresholds, consumables and boot upgrades do not reset it. Older serialized preferences
  acquire a current-stage anchor rather than retaining game-long suppression.

The defensive exception can replace an unstarted fourth/fifth learned item, while protecting
owned items, opening core and invested components. It does not reserve its own slot before
reconstructing the learned path. Separate `last_unpromoted_path` memory prevents promoted
items from feeding back into flexible-tail selection. Early prototypes had 15, then two,
identical-state path changes in full-team validation; the final version has zero.

These are explicit policy choices, not fitted estimates of winning item value. The strength
estimate still uses visible equipment, levels and scoreboard information. It cannot establish
who is actually reachable in a fight, how a player positioned, or the damage a different item
would have prevented. Corpora here are ranked; the seven personal replays are normal Draft Pick.

## Recorded-game evidence

All seven recent Xayah recordings replay with zero illegal recommendations. Their inventories
remain the actual recorded purchases: the replay does not simulate following different advice.

- Rammus game: at 21:36, after IE completes, LDR replaces Navori with the explicit reason
  that Rammus has 180 visible item armor. At 22:58 repeated Trundle deaths justify GA; this
  is a later defensive decision, not proof that penetration would have saved that fight.
- Ahri game: at 15:01, after Yun Tal and the second recent Ahri death, Mercurial becomes
  the target. It remains available through the later deaths instead of being gated behind
  finishing IE. The reason names Ahri and magic resistance, not an unsupported cleanse claim.
- The long 33-kill game reconsiders Executioner's after IE and Navori completions. It does
  not re-offer merely because gold crosses its price while the same item is being built.
- The two short ordinary openings and the 1:25 recording keep their previous targets.

## Validation

Baseline: `4daf886`. Candidate logic: `30538ba`, `fc6ae17`, `b79aad1`.
Both versions use the same cached training-only artifacts, validation cases and Data Dragon
16.19.1 catalog; report fingerprints must match. The separate test split was not used for tuning.
Detailed examples and recordings stay outside Git; aggregate results are in
[live-adaptation-evaluation.json](live-adaptation-evaluation.json).

| Dataset | Games / observations | Next completion agreement | First legendary in path agreement | Target changes without inventory change |
|---|---:|---:|---:|---:|
| kaggle | 3,924 / 88,959 | 50.75% → 50.81% | 59.20% → 59.30% | 860 → 993 |
| riot | 210 / 3,306 | 48.36% → 47.45% | 60.89% → 58.42% | 55 → 102 |

All 92,265 observations have zero invalid recommendations and zero identical-state target/path
changes before and after. Ordinary transition denominators are 42,973 (Kaggle) and 1,390 (Riot).
Riot next-buy agreement falls from 348/1,437 to 341/1,437; component agreement from 219/863 to
211/863. Affordable detour windows increase from 513 to 802 (Kaggle) and 41 to 75 (Riot).
These are material tradeoffs, not an across-the-board benchmark improvement. The changes satisfy
previously missing response contracts; subsequent live review must check whether the additional
responses are useful rather than merely more frequent.

The behavior contracts cover actual target changes under visible armor, no first-item penetration
rush, no defensive override for a single/old/assisted death, defensive component commitment,
affordable core completion, repeat-poll stability during onset/release, staged detour deferral,
and serialization migration. Validation includes 319 core tests, 40 runtime tests, 25 headless
UI tests, core/runtime Clippy and the Windows release check.

## Interpretation and remaining limits

Purchase agreement is imitation, not a win metric. The defensive exception deliberately departs
from the common order in some states, and renewed detour eligibility increases the number of
target transitions. Zero repeated-state changes means stable input produces stable advice; it
does not mean every transition during a changing match is strategically useful. Further work
should evaluate the new exceptions by threat type and item investment rather than optimize
only for agreement or for minimum changes.

The corpus boots model still conditions on draft damage composition rather than the live fed
threat. Sparse champion/role backoff can also propose a poor damage family in uncommon roles;
the Syndra ADC validation diagnostics expose that separate candidate-quality limitation.
Neither issue is resolved by this change. The existing training population, model freshness,
and incomplete opponent coverage also remain limits.

## Installation and Swiftplay follow-up

Installed October 2 after the game guard observed Lobby. The Windows release build and headless
probe passed; one canonical Recall process relaunched. The LeBlanc Mid Swiftplay recording that
ended during deployment preparation has 140 observations and no shop violations under either
binary. Its recorded Banshee's/Zhonya's progression came from the existing fallback engine,
not the new standard-mode exception. The new detour memory reconsiders Orb after later item
completions. This game does not establish that the defensive progression was optimal, and no
additional coefficients or rules were changed in response to the loss.
