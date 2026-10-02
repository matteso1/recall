# Repeated advice review — 2026-10-02

## Evidence and scope

Seven recent Xayah normal Draft Pick recordings (queue 400), 965 observations, replayed
without invalid recommendations or skipped captures. Targets and components progressed;
this does not by itself prove the window rendered every state. The shared early core is
Yun Tal, Infinity Edge, Navori. The embedded model has 497 Xayah ADC games and conditions
primarily on owned legendaries, with coarse composition adjustments and bounded live needs.
It is a purchase model, not proof that one build maximizes the chance of winning.

The current cached provider favors Navori before Infinity Edge, whereas the embedded
Master+ model favors Infinity Edge first. These are different populations and dates;
changing item weights just to create variety or match that provider is not justified.

Confirmed defects:

- The recap takes the last eight component-level decisions. One long game consequently
  displays eight Bloodthirster entries, hiding the earlier build decisions.
- The conditional, smoothed model score is described as an exact share of Master+ players.
- Quest-slot boots consume one of the planner's six commitments. After selling the starter
  with five legendaries and hidden boots, the planner says the build is complete despite
  an empty normal slot. Six legendaries then hide the owned boots from the displayed path.

The brief Executioner's recommendations in two games ended after a Pickaxe purchase.
This is the existing declined-detour policy, not delayed recognition of the boots purchase.
That policy suppresses the detour for the remainder of the game; revisiting its duration
requires a separate policy evaluation.

## Controlled composition checks

Sixteen offline cases held champion, gold, level, provider data and owned build stages
constant while replacing the enemy draft. Kill events were cleared; the armor scenario
also equipped the three frontline enemies with 150 visible bonus armor each. These are
sensitivity probes, not evidence that the resulting purchases win more games.

| Opponents | After Yun Tal | After Yun Tal + IE | Later path |
| --- | --- | --- | --- |
| Physical damage | IE | Navori | LDR, Bloodthirster |
| Magic damage | IE | Navori | LDR, Bloodthirster |
| Heavy healing | Executioner's detour | Executioner's detour | Mortal Reminder, GA |
| Visible armor | IE | LDR | Navori after penetration |

The model responds to healing and armor but is conservative about magic protection.
`V3_NUDGE * V3_NEED_CAP` permits at most 0.6 added to log probability, a relative
multiplier of about 1.82. A much less common item cannot catch up through this nudge.
The next-purchase stage also caps situational score at 1, below the 1.5 gap between
the first and second pending items; completion/progress can still change their order.
This explains the repeated core without proving it is always the best choice.

Keep these limits explicit. A future scoring experiment must evaluate responsiveness
alongside legality, stability and purchase agreement on full-team timelines. The large
Kaggle benchmark has little enemy-inventory coverage; agreement alone cannot validate
stronger situational advice. No scoring weights or trained artifacts change in this fix.

## Implementation and validation plan

1. Preserve every raw journal decision and feedback ID, but summarize consecutive work on
   one target as one build step. Keep the first recommendation of each step and retain
   returns after a different target. Show the target name in the recap.
2. Describe learned choices as recommendations from the Master+ purchase model, conditioned
   on the preceding legendary items. Remove the misleading empirical percentage.
3. Derive the path capacity from observed quest-slot boots: six normal slots plus those
   boots. Retain all owned commitments, never assume quest completion from role alone,
   and preserve shop legality/full-bag checks. Keep the five-legendary model horizon; the
   extra slot uses the existing flexible-tail policy.
4. Reproduce the slot and recap defects in behavioral tests, run core/runtime/UI checks,
   compare all seven recordings and held-out validation against the archived evaluator,
   then build/install with the existing game-phase guard.

Raw recordings, journal, replay reports and evaluator binaries remain outside Git under
`~/data/recall/review/repetition-20261002/`.

## Results

The slot and recap regression tests failed before the fix and pass afterwards. Core tests,
40 runtime tests, 25 browser tests, Clippy on both Rust targets, and the Windows release
type-check pass. The browser check includes all seven owned items fitting the build row.

All 965 recorded observations still replay with no invalid recommendations. Six games
have identical next targets and purchases throughout. The long game's only 27 action
changes are after its fifth legendary: 26 correctly blocked future targets while the
starter fills the last bag slot, then an affordable Guardian Angel after that slot opens.
The planner stops after the sixth legendary and retains the quest-slot boots in the path.

Both validation comparisons use the same cases and training artifacts on each side,
with the original evaluator archived before implementation. The test split stays unscored.

| Validation measure | Before | After |
| --- | ---: | ---: |
| Kaggle observations / games | 88,959 / 3,924 | same |
| Next completed-item agreement | 6,915 / 13,694 | 6,950 / 13,694 |
| Next legendary on path agrees | 6,094 / 10,352 | 6,128 / 10,352 |
| Changed target across consecutive minutes with unchanged inventory | 782 / 42,973 | 860 / 42,973 |
| Invalid recommendations | 0 | 0 |
| Identical-state target / path changes | 0 / 0 | 0 / 0 |
| Frozen Riot observations / games | 3,306 / 210 | same |
| Next completed-item agreement | 263 / 550 | 266 / 550 |
| Next legendary on path agrees | 243 / 404 | 246 / 404 |
| Next purchase agreement | 346 / 1,437 | 348 / 1,437 |
| Next component agreement | 220 / 863 | 219 / 863 |
| Changed target across consecutive minutes with unchanged inventory | 49 / 1,390 | 55 / 1,390 |
| Invalid recommendations | 0 | 0 |
| Identical-state target / path changes | 0 / 0 | 0 / 0 |

The larger build horizon raises ordinary minute-to-minute target changes (Kaggle 1.82%
to 2.00%; Riot 3.53% to 3.96%). Gold and enemy state can change in those comparisons.
It introduces no identical-state oscillation. The small agreement gains and one fewer
matched Riot component are purchase-imitation results, not evidence of improved win rate.
The composition sensitivity limitation described above remains open; this change does
not claim to solve matchup-dependent build optimization.
