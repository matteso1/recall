# Riot reconstruction and planner evaluation — September 28, 2026

The new offline command can audit paired Riot matches, reconstruct causal inventory histories,
fit isolated training artifacts and run the assembled planner. It exposes a useful limitation:
complete ten-player timeline frames do not necessarily contain complete inventory history.
No production scoring rule, embedded model or installed overlay changed in this work.

## Inventory audit

| Check | Result |
|---|---:|
| Paired matches audited with patch-specific catalogs | 662 |
| Player histories / minute player frames | 6,620 / 184,250 |
| Final combat inventory compatible with the ledger | 6,544 / 6,620 |
| Histories without unexplained events or final mismatches | 6,427 / 6,620 |
| Histories also ending with no unresolved item identities | 4,950 / 6,620 |

Compatibility is weaker than a fully observed history. The final response exposes inventory slots,
not stack quantities, and some free grants/upgrades have no purchase event. Trinkets are outside
this combat-inventory contract. Unresolved support choices, tear transformations and automatic
skill-elixir consumption stay explicit; they are never filled backward from final equipment.
Even resolved intermediate bags must pass slot-capacity checks before planner evaluation.

The ledger supports purchase/sale/undo, actual consumed recipe components, duplicate components,
instant consumables, timed biscuit/footwear grants, level-based tonic grants, observed support
quest transitions, used Armguard and mid/ADC quest boots. Gunmetal Greaves lacks the ordinary
Data Dragon Boots tag, so its free upgrade is resolved through the recipe relationship.
Same-time unrelated consumption is not resurrected by purchase undo. Unknown events, mismatches
and Viego possession histories are excluded and counted. There remain 76 final mismatches,
3,774 unexplained removal events and 41 unsupported possession histories; counts overlap.

Riot's [role-quest documentation](https://www.leagueoflegends.com/en-sg/news/game-updates/patch-26-1-notes/)
and the exact patch catalogs support the quest rules. Missing-event behavior above was measured
in the preserved local responses, not assumed to be an API guarantee.

## Leakage control and coverage

The 48 saved current ladder seeds split into 33 training, eight validation and seven test players
by stable identity hash. Among 562 patch-16.19 matches, 337 train, 119 validate and 105 test; one
mixed validation/test seed match is excluded. A held-out seed's presence removes the entire match
from training. Only held-out seed players are scored. Other contextual players may recur across
partitions. Seed rank is verified at collection; contextual players' ranks are not verified.

Training uses 3,283 eligible player-games from 2,042 unique players and 11,366 legendary/boots
purchase decisions. It supports 20 champion-role next-item tables and 15 boots tables. No enemy
champion meets the unchanged 500-appearance floor for answer weights, so those tables are empty
and the planner uses its ordinary trait fallback. The 100 older 16.18 pairs are audited but not
used in this patch's training or evaluation. The old Kaggle priors are not mixed into this new
held-out protocol: their pseudonymous player IDs have no verified bridge to Riot PUUIDs.

| Validation diagnostic | Exact team | Exact self, known peers |
|---|---:|---:|
| Target player-games before exclusions | 122 | 122 |
| Unsupported champion-role games | 87 | 87 |
| Games / observations evaluated | 29 / 106 | 33 / 566 |
| Known opponent observations / possible | 530 / 530 | 2,279 / 2,830 (80.5%) |
| Observations with all ten inventories | 106 | 106 |
| Invalid recommendations | 0 | 0 |
| Identical-state target / path changes | 0 / 0 | 0 / 0 |
| First legendary agreement | 4 / 9 | 37 / 72 |
| Next purchase agreement | 6 / 43 | 56 / 244 |
| Next component purchase agreement | 4 / 28 | 41 / 155 |

These columns use different observations; their agreement rates are not a before/after engine
comparison. The strict mode is strongly biased toward early game because both supports' final
quest choices are unobserved. The broader mode still requires an exact, reconciled active-player
inventory; unresolved peers are omitted and counted while draft composition remains known.
Both modes include own gold, runes, spells, observed skill ranks and public kill-feed history.
No historical enemy positions, exact gold or combat/damage statistics enter the live inputs.

In the broader run, there are 244 following-minute shopping windows, with only one observed
anti-heal purchase and one cleanse purchase. Ten affordable anti-heal suggestions occur in
windows without an observed anti-heal purchase; this is a diagnostic mismatch, not proof those
suggestions were wrong. Minute snapshots do not establish exact shop gold or access. The pilot
cannot justify new defense weights or detour gates. There are also 11 frames without a target;
retain these in denominators instead of silently dropping them.

## Reproducibility and verification

[Aggregate results](riot-reconstruction-evaluation.json) contain per-role numerators/denominators,
exclusion counts, model hashes and both validation protocols. Raw matches, identities, cases,
training tables and diagnostic examples remain outside Git. Input/source/artifact fingerprints
reject incompatible before/after comparisons. Repeating each validation run produces identical
metrics and fingerprints. This is a reproducibility baseline, not an improvement claim. Test
cases have been prepared but the separate test split has not been scored or used for selection.

Verification: 37 priors/collector Python tests, 313 core Rust tests with the evaluation feature,
core Clippy with warnings denied, formatting and diff checks pass. The refactored answer fitter
reproduces the existing legacy training-cache artifact exactly. No app-facing Rust code changed,
so a Windows rebuild or overlay restart is not required for this offline work.

Run instructions and metric definitions are in the [priors README](../../tools/priors/README.md).
Next research priority is more independent held-out players and better coverage of unobserved
item transitions. Keep the large legacy benchmark for broad regressions and these fresh protocols
for richer-context checks; neither purchase imitation alone nor a few played games proves that a
recommendation improves winning chances.
