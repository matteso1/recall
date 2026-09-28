# Engine review — September 27, 2026

Reviewed baseline `64ad7a8`, the production planner, its data exports and recorded-game adapter.
The local corpus contains 107,312 player-games, 1,132 tracked players and 390,781 item decisions.
Purchase agreement measures imitation of these players. It does not establish that advice causes
more wins, and game win rates do not measure lane difficulty.

## Findings and priorities

1. **There was no held-out check of the assembled planner.** Testing `nextprior` alone misses
   candidate filtering, path reordering, component detours, boots and state retention. The new
   evaluator calls the actual planner and replay's legality checks. Its training inputs exclude
   evaluation players and mixed-split matches. Validation and final-test players remain separate.
2. **A learned legendary could be absent from the purchase candidates.** The path model and
   provider pool had separate sources. A Xayah regression with the provider omitting Infinity Edge
   reproduced a skipped learned target. The immediate learned target now contributes a purchase
   candidate; later chain entries retain the usual candidate sources instead of becoming new detours.
   This is a consistency fix, not a guarantee of higher aggregate imitation accuracy; inspect both.
3. **Boots had useful corpus evidence but still used provider popularity plus fit constants.**
   The earlier claim that they ignored enemy damage entirely was incorrect: resist fit already
   affected them. The new model uses champion/role boot frequencies and role-level magic-count
   adjustments, with smoothing and a provider fallback for missing coverage or incomplete drafts.
   It still uses damage tags and does not model lane opponent, CC type or champion build variant
   (for example, an AP Shaco build).
4. **Sparse champions lost their ban matchups.** Nilah's 82 corpus games missed the 200-game
   threshold. Her cached provider response contains 9,759 ADC games and usable matchup counts.
   The new fallback uses only the same champion and role, excludes picked/banned champions, requires
   50 matchup games, shrinks toward the champion baseline, and labels its op.gg source. Corpus
   champion tables retain priority. A difficult individual lane does not become a universal ban rule.
5. **Detour timing needs its own endpoint.** ADC Executioner's purchases number 1,268 out of
   21,620 ADC player-games; median first purchase is minute 22. Before the purchase minute, 577
   buyers had two completed legendaries and 372 had three (74 had none). That supports measuring
   timing, but a buyer-only histogram cannot justify a mandatory two-item delay. The engine already
   requires at least one completed core item for these detours; the earlier claim of no gate was
   incorrect. The new completion benchmark cannot measure component-buy timing. Retain the gate
   until component purchase opportunities, enemy composition and non-buyers are evaluated together.
6. **Answer weights are not fully role-specific.** The regression has role intercepts but shared
   enemy coefficients, without own-champion or role-by-enemy interactions. Against Lissandra,
   ADC QSS/Mercurial ownership is 40/407 (9.8%) versus 632/21,213 (3.0%) otherwise; jungle is
   1/424 (0.24%) versus 39/19,630 (0.20%). Fit regularized role interactions and own-champion/build
   controls on training players, judge calibration and purchases on validation. Do not interpret
   an odds-ratio floor as a statistical significance test.
7. **Tags remain active.** `nextprior::Comp` still uses healer, tank and magic-heavy trait buckets.
   Garen's direct anti-heal weight is zero, but his legacy healing tag still affects that model's
   composition bucket. The tank tag is therefore not universally harmless. Use held-out feature
   ablations before removal. Current evidence on poke-driven boot purchases does not establish
   that all sustain items or all poke situations have no response.

## Every named tuning constant in decision.rs

These are grouped by what could validate them; numeric values are the reviewed baseline.
Changing many together would obscure cause and effect. Tune on validation, then run the untouched
test group once per selected design. Retain role/champion denominators and regressions.

| Terms | Current values | Measurement or contract |
|---|---|---|
| `ORDER_PRIOR`, `FINISH_NOW`, `OWNED_CREDIT` | 3, 3, 3 | Next purchase conditional on owned components, gold and model probability. Completion and component opportunities need separate endpoints. |
| `MAX_NEED_SCORE`, `DETOUR_NEED`, `RESIST_SCALE` | 5, 1.5, 5 | Calibrate answer purchases by role, own champion, enemy items and game time. Enemy equipment coverage is insufficient in this corpus alone. |
| `MIN_LATE_PICK`, `MIN_BOOTS_PICK` | .02, .05 | Coverage/precision tradeoff by sample size; assess exclusions as well as accuracy. |
| `ORDER_MARGIN`, `TAG_MARGIN`, `TAIL_MARGIN`, `TARGET_MARGIN` | .15, .15, .25, .75 | Accuracy/stability tradeoff using minute observations plus recorded poll cadence. |
| `BOOTS_FIT_WEIGHT`, `BOOTS_HOLD` | 3, .5 | Superseded on covered complete drafts by measured boots; still active in fallback/Swiftplay. Sparse modes need their own evidence. |
| `PLANNING_LEVEL`, `LEVEL_STEP`, `FED_STEP`, `FED_LEAD_CAP` | 9, .1, .08, 6 | Threat proxy calibration against visible combat state. Pregame level is a convention; do not call it learned evidence. |
| `HUNT_MEMORY_SECONDS`, `HUNT_CAP`, `HUNT_THREAT`, `HUNT_DIVE` | 360, 2, .4, .5 | Requires timestamped kill-feed observations. Not identifiable from the current minute corpus. |
| `PROMOTE_KEEP`, `PROMOTE_HOLD_SECONDS`, `COMMIT_MARGIN`, `COMMIT_THREAT` | 1.2, 180, 1, 1 | Stateful target/promotion persistence. Test identical-state idempotence and live-poll changes; three-minute hold is a policy choice. |
| `V3_NUDGE`, `V3_TIE_MARGIN`, `V3_NEED_CAP`, `V3_SITUATION_CAP` | .3, .25, 2, 1 | Ablate each atop the same trained prior. Compare next-item agreement, legality, role slices and stability. |
| `V3_LEGENDARIES` | 5 | Build-path capacity convention, not a fit parameter. Keep shop/quest capacity tests distinct from preference learning. |
| `TYPED_ANSWER_NEED`, `TYPED_MARGIN` | 1, 1 | Evaluate mandatory damage-type overrides only on adequately observed enemy equipment. |

Inline coefficients also matter: sustain-vs-poke 1.7; poke contribution .3; dive .25; healing
.55/.8; anti-heal item fit 3.4; cleanse fit 3.2; defense .9 + .9×pressure (2.8 in Survival mode);
armor penetration normalization .35 and MR penetration .4; resist saturation 100 armor / 70 MR;
damage mix .6×champion tag + .4×visible equipment; level clamp .6–1.5; relative-gold offset 1,000
and clamp .5–2; lane focus 3× before minute 10 tapering to minute 20; missing armor/MR 60/40;
delay and blocked-purchase penalties .4/4. Additional terms include the 1.5× observed damage-type
dominance threshold, a 2× visible sustain contribution to healing need, .65× frontline scaling
for reactive anti-heal, boot/tail popularity at 2×sqrt(pick), .2×sqrt(pick) for off-path candidates,
the .15 sustain denominator and 1.5 cap, .5 dive discount on sustain, .3 minimum kill-feed evidence,
and .25/.35 item-fit thresholds. These span empirical preferences, defensive mechanics,
missing-data defaults and explicit user policy. They should not all be optimized to one imitation
score. In particular, Survival preference is deliberate user intent.

The models also contain policy choices: answer odds floors/references (anti-heal 1/3, cleanse
1.5/5, burst 1/1.75), prior smoothing/sample floors, and trait buckets. Moving a number out of
`decision.rs` does not make it measured. Export versions and input hashes accompany comparisons.

## Coverage and limits

The evaluator trains on 58,211 player-games from 800 players. Its validation cases cover 3,924
games; final-test cases cover 4,046. Only about 1.2% of possible enemy minute inventories are
available **in those evaluation cases after the split exclusions**. This is not the raw corpus's
coverage: before exclusions, exact-minute opponent coverage from minute 6 is 10.64% overall and
14.14% on patches 16.17–16.18. Most enemy builds and all kill feeds are absent from the evaluator.
The [data-source audit](data-source-audit.md) records denominators and richer candidates.
Rune pages and ability ranks are
also unavailable. Historical provider inputs are replaced by training-only purchase aggregates.
These restrictions make it unsuitable for calibrating every live threat/defense term.

The raw inventory arrays are compact lists, not real shop slots. The adapter separates trinkets,
counts and omits ambiguous overfull inventories, and restores past ADC boot completions as virtual
quest boots. It does not infer boots for other roles. An unobserved ADC boot sale can still be
mistaken for quest hiding. Counts of all exclusions/restorations appear in the manifest. Minute
flips are not polling flips, and affordable detours are a diagnostic rather than purchase labels.

The baseline answer-weight change passed the original nine game replays (1,547 observations,
zero invalid recommendations) and changed none of their paths relative to `c1e37a6`. That shows
why these recordings alone cannot decide model quality.

## Next work

- Extract component purchase opportunity labels; compare a calibrated role/time/owned-item detour
  model with the current gate, including false positives among players who never buy the answer.
- Add regularized role-specific answer effects and ablate legacy healer/tank composition tags.
- Investigate the identical-state repeat flips reported by the full planner; retain recorded
  poll-cadence evaluation alongside the corpus.
- Replace remaining provider dependencies for runes, spells and skills with independently sourced
  match data; verify source terms before making distribution or commercial claims.
- Add an end-to-end raw-data rebuild entry point and shop-patch compatibility guard. The persistent
  requirements file and cached evaluator solve only the derived-data-to-evaluation portion.
- Verify live ban display and CS recap on the next naturally occurring game. Owned-boot display
  position and unknown-role CS comparison remain lower-priority UI questions.

## Final measured comparison

Baseline engine `64ad7a8` versus `16ecdb3`, with identical training artifacts and
observations for each comparison. The validation run contains 88,959 frames; the separate test
group contains 92,147. No engine weights were selected using test-group results.

| Metric | Validation before → after | Test before → after |
|---|---|---|
| Next-buy target | 49.91% → 50.44% | 50.80% → 51.88% |
| First legendary on path | 59.52% → 58.80% | 61.89% → 61.23% |
| Boot type | 58.05% → 67.41% | 54.13% → 65.34% |
| Minute target flips, unchanged bag | 1.80% → 1.83% | 1.88% → 1.91% |

Both versions have **zero invalid frames** in both groups. Repeat-state target flips are 8 → 11
on validation and 8 → 7 on test; these rare state-retention cases remain an explicit follow-up.
The source report stores raw numerators and denominators, every role, Xayah, training/input hashes
and omission counts in [engine-evaluation.json](engine-evaluation.json).

Boot matching and overall next-buy agreement improve. First-legendary path agreement falls by
0.71 percentage points on validation and 0.66 on test. The missing-candidate fix is retained for
its reproduced consistency contract, scoped to the immediate learned target. Do not present this
as uniform improvement of every metric or as win-rate uplift. The first broader candidate-union
implementation and the scoped fix produced the same aggregate purchase metrics; narrowing limits
the change's scope rather than claiming a measured accuracy recovery.

Verification: 309 core/replay/integration tests, 39 runtime tests, 23 Playwright tests and five
Python evaluator contracts pass; core and runtime Clippy pass with warnings denied; Windows
release type-check passes. Ten recorded games replay 1,660 observations with zero invalid
recommendations and no missing aggregates. The original nine retain their previous paths.
Nilah's actual cached response now produces champion-specific, source-labelled ban suggestions.

Reproduce the full validation comparison after environment setup in the priors README:

```bash
~/data/recall/.venv/bin/python tools/priors/backtest.py \
  --baseline ~/data/recall/evaluation/final-validation.baseline.json \
  --output ~/data/recall/evaluation/latest.json
```

The cached full validation run takes roughly four minutes on this development machine at normal
CPU speed. The initial data preparation is additional; raw cases and local recordings remain
outside the repository.
