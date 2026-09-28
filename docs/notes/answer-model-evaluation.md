# Defensive-answer model experiment — September 28, 2026

Adding own-champion and patch controls substantially improves held-out defensive-item
ownership predictions. Adding role-by-enemy interactions on top does not show a clear
additional benefit. This supports testing better controls before introducing separate
role weights; it does not establish better item recommendations.

`tools/priors/evaluate_answers.py` makes the comparison reproducible on the existing
Kaggle corpus. It uses the established player/match-disjoint split, trains on 58,210
player-games from 800 players, and validates on 4,257 games from 157 different players
on patches 16.17–16.18. Games shorter than 15 minutes are excluded. One training game
has no timeline rows and is excluded, rather than labelled as a nonbuyer. The test
partition is not scored.

All models predict whether the player ever owns an answer item. The existing model
structure uses role and five enemy champion indicators. The controlled variant also
includes the player's champion and patch. The third variant adds role-by-enemy
interactions. Vocabulary and coefficients are fitted on training rows only; player
identity, final outcomes and item builds never become predictors.

| Answer | Validation buyers | Existing structure, C=1 | Best controlled | Best role interactions |
|---|---:|---:|---:|---:|
| Anti-heal | 495 | 0.329571 | 0.314502 | 0.312496 |
| Cleanse | 51 | 0.054411 | 0.051589 | 0.051800 |
| Anti-burst | 686 | 0.392250 | 0.302817 | 0.305581 |

Values are log loss; lower is better. Controlled/interaction regularization is selected
from C = 0.1, 1 and 10 by validation loss. Own-champion and patch controls are introduced
together, so this experiment does not separate their effects. At fixed C=1, the controlled
losses are 0.317693, 0.051750 and 0.303282 respectively: the gain is not solely a change
of regularization strength. The anti-burst controlled model's average precision is
0.584 versus 0.323 for the existing structure.

Paired loss differences are bootstrapped 500 times by whole held-out player. For the
best role-interaction model minus the best controlled model, exploratory 95% intervals are:

| Answer | Loss difference | Interval |
|---|---:|---:|
| Anti-heal | -0.002007 | -0.005413 to +0.001828 |
| Cleanse | +0.000211 | -0.000578 to +0.001541 |
| Anti-burst | +0.002764 | -0.000582 to +0.006383 |

All include zero. Regularization selection and these intervals reuse validation, so
they are exploratory diagnostics, not untouched final-test inference. Per-role effects
may still matter with more independent buyers or a different model. In particular,
51 total cleanse buyers cannot support arbitrary role/champion subdivisions.

## Full-planner check

The selected controlled coefficients were also converted to experimental enemy weights
using the existing sample floor, odds normalization and weight mapping. One training game
without a timeline is excluded from the candidate; all other training memberships follow
the existing split. On identical validation cases, catalogs, next-item/boots priors and
fallback aggregates, the current Rust planner produces:

| Check | Existing weights | Controlled candidate |
|---|---:|---:|
| Games / observations | 3,924 / 88,959 | 3,924 / 88,959 |
| Next displayed completion agrees | 6,915 / 13,694 | 6,919 / 13,694 |
| Next legendary on path agrees | 6,094 / 10,352 | 6,095 / 10,352 |
| Boots agree | 2,253 / 3,342 | 2,253 / 3,342 |
| Target changes over unchanged-inventory minute transitions | 782 / 42,973 | 750 / 42,973 |
| Frames offering an affordable answer detour | 517 | 465 |
| Invalid recommendations | 0 | 0 |
| Identical-state target / path changes | 0 / 0 | 0 / 0 |

Next-completion agreement rises just 0.029 percentage points; legendary-path agreement
rises 0.010 points. Xayah improves by one of 48 legendary decisions, while mid loses one
of 1,691. These are descriptive validation results, not evidence of a reliable gain.
The legacy observations still have sparse opponent inventories and no component labels,
so fewer answer detours are not established as better timing.

Some proposed weight changes are large despite the near-zero aggregate benefit:
Lissandra's cleanse weight falls from 0.988 to 0.360, Fiddlesticks' from 1 to 0.447,
while Vi's anti-burst weight rises from 0.557 to 0.950. Do not promote this candidate.
Ownership prediction can improve by learning which champions normally buy each item;
that does not prove its opponent coefficients are calibrated as situational need weights.
The current odds-to-weight floors, references and detour timing remain policy choices.
Further work should assess how these weights map to actual purchase decisions, with
positive/negative shopping windows, before another production change. No test split was
used and no model from the experiment was exported to the app.

The [planner comparison](answer-planner-evaluation.json) records both metric reports,
runner hash and artifact provenance. Answer artifacts intentionally differ; every other
cached artifact was checked to be byte-identical before and after the experiment.
The versioned optional exporter reproduces every tested enemy weight.

Run the command in the [priors README](../../tools/priors/README.md#answer-model-experiments).
The [aggregate result](answer-model-evaluation.json) includes source/code hashes, sample
counts, all model metrics, chosen strengths and comparison intervals without player IDs.
