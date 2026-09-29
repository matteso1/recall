# Kalista recording review, 2026-09-28

This is one Swiftplay recording on patch 16.19, reviewed as a usability and fallback-planner
failure report. It is not evidence for changing standard-mode item weights or for attributing
a match outcome to a build. Raw captures and match details remain outside Git.

## Ultimate setup

The visible inventory contained Kalista's Black Spear from the first playable observation
until 23:53. R was rank 1 by 5:18. At 23:55 the player completed Terminus into the freed slot.
An incomplete ally bond is a strong explanation for the reported unusable ultimate, but
neither the recording nor Live Client data exposes the bond, cast attempts, or ally range.
The spear disappearing could be a sale or consumption; do not call that a verified bind.

[Riot's Kalista page](https://www.leagueoflegends.com/en-us/champions/kalista/) describes R as
bringing the Oathsworn ally to Kalista and letting that ally launch themselves. The patch's
[Data Dragon item catalog](https://ddragon.leagueoflegends.com/cdn/16.19.1/data/en_US/item.json)
describes consuming Black Spear to form the bond. Item names are resolved from that catalog.

The main panel now shows a short setup reminder whenever the identified Kalista has a
positive inventory count of Black Spear. It works before aggregate build data loads and
disappears when the item leaves inventory or live evidence goes stale. It does not claim
the bond is missing, select an ally, infer R availability, or issue gameplay inputs.

Validation: 314 core tests, 40 runtime tests, 24 browser tests, both Clippy targets, and the
Windows release type-check passed. The recorded replay's 184 purchase/path results were
identical before and after; the reminder appeared in 166 observations with the spear and
vanished when it left inventory. Both replays had zero legality violations. The panel check
also covered build-data loading, stale evidence, clearing the reminder, and a 320×300 viewport.

## Build-order failure still requiring evaluation

The cached provider's most-played core was Statikk Shiv → Guinsoo's Rageblade → Terminus
(17,075 observed core builds). Recall instead promoted two defensive legendaries:

| Recorded time | Observation / recommendation |
| --- | --- |
| 7:28 | Shiv completed; Jak'Sho promoted ahead of boots and Rageblade |
| 12:56 | Jak'Sho completed; Randuin's promoted next |
| 13:36 | Existing boots components made Steelcaps the next target |
| 15:36 | Steelcaps completed; Randuin's next |
| 15:50 | Randuin's completed; Rageblade finally next |
| 21:02 | Rageblade completed |

Replaying all 184 observations reproduced the promotions with zero legality violations.
Legality alone did not catch the quality problem. `decision::promote_answer` enabled a
death-triggered defensive promotion once one core item was finished, then allowed another
defensive promotion before the next core. The explanation explicitly blamed Miss Fortune.
The learning text also described magic protection while that promotion's reason described
armor: mixed-resistance item labeling needs consistent evidence.

Swiftplay bypasses the learned sequence model, so this legacy behavior remains active there
and for uncovered standard-mode champion/roles. It is already disabled for the learned
standard-mode path. The production path/target choices are unchanged in this reminder update.

Next planner experiment: measure fallback core completion and repeated defensive promotions
on held-out players, including sparse champion/roles. Compare disabling automatic kill-feed
promotions against preserving an explicitly requested Survival preference. Evaluate the
whole planner and investment commitments; do not introduce a Kalista-only item order or
assume this game's counterfactual outcome. Keep Swiftplay evidence separate from ranked and
normal-draft evidence.
