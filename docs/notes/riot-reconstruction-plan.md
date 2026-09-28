# Full-team timeline reconstruction and evaluation

The owner approved the next-day plan: turn validated Riot pairs into inventory histories and feed
the assembled planner a richer, reproducible benchmark. Execute inline in the canonical WSL repo.
Raw identities, captures and generated cases remain outside Git. No production model promotion is
part of this data-foundation change; any later behavioral change needs its own measured comparison.

## 1. Inventory ledger

Create `tools/priors/riot_inventory.py` and `test_riot_inventory.py`. Inputs are a paired match and
timeline plus that match patch's Data Dragon item table. Output per-player inventory histories,
purchase transactions, and explicit validation issues. Preserve event order, group same-time
transactions, and restore the actual consumed components when undoing a combined purchase. Selling
and undoing a sale must round-trip. Do not infer the purchased recipe from a completed item alone.

Interface: `reconstruct(match, timeline, items)` returns a serializable dictionary with player
results keyed by participant slot. Each result includes frame inventories, purchase events,
reconciliation status, issues and any explicitly supported inferred grants/transformations.
Validation compares the final multiset with `item0`–`item6` plus a real equipment `roleBoundItem`;
role quest tokens are not equipment. Final items are validation-only, never used to fill past state.
The audited contract excludes trinkets and validates stack presence, because the source omits
implicit trinket grants and final stack quantities. Unobserved transformations remain uncertain.

Tests begin with a small synthetic catalog and anonymous events: combined purchase/undo restores
components, sale/undo, repeated copies, same-time consumption, unsupported removal, mismatched
final bag and no future-state backfill. Add independently reproduced special cases only when
source events/catalog mechanics support them. Run against all 662 pairs and count exclusions.

## 2. Private corpus and evaluation split

Create `tools/priors/riot_corpus.py` and `test_riot_corpus.py`. Validate source manifests/hashes,
resolve exact patch catalogs and cache normalized results with source/code/catalog fingerprints.
Use current patch 16.19 for the first planner comparison; audit 16.18 separately with its catalog.

Assign the saved ladder seed cohort to train/validation/test by a fixed PUUID hash. A held-out
seed player's presence excludes the entire match from training. Matches containing validation
and test seed players together are excluded. Score only held-out seed players in evaluation;
training may use all eligible participants from the remaining matches. Thus evaluation targets
and matches never train the models. Other contextual participants may recur across groups; do
not describe this as every participant being mutually disjoint. No name/PUUID becomes a feature.

Create training-only legendary/boots decisions and answer ownership from validated histories;
write inputs compatible with the existing prior exporters and provider-shaped aggregate builder.
Count every excluded champion-role, invalid player and incomplete full-team frame. Keep both the
old sparse-corpus benchmark and the new source as distinct protocols.

## 3. Planner adapter and metrics

Extend `tools/priors/backtest.py` with an explicit Riot source mode and separate default cache.
Use the existing Rust runner and mandatory training-artifact injection. Build each own-player
minute state from its actual gold, inventory, runes, learned skill ranks and public kill feed;
all nine other players receive only fields the live app can observe. Never pass historical enemy
positions, exact gold, damage totals or hidden combat statistics as live features.

Labels are future purchases after the observation, never purchases already applied to its bag.
Separate legendary/boots completion agreement from component next-buy agreement. Ambiguous
same-minute labels and undone purchases must be counted or excluded explicitly. Every eligible
shopping window can supply an anti-heal/cleanse purchase opportunity, including negative cases;
minute cadence does not establish exact shop gold or optimal timing.

Extend `overlay/core/src/bin/backtest.rs` only as needed for component/answer metrics and actual
minute timestamp jitter. Add focused tests for the new metric contract. Compare the same prepared
cases and artifacts, report legality and identical-state stability, and preserve per-role counts.

## Coverage-driven adjustment

The strict pilot retained only 106 validation observations. Keep that diagnostic, and add an
explicit `known-peers` policy: the active player's inventory must still be exact, while unresolved
peers are omitted and counted. Draft composition remains available. Never fill an unknown peer's
inventory with zero items. The policy is part of the comparison fingerprint and has its own cache.
Both modes must remain separate from the legacy sparse-corpus protocol.

## Completion checks

- [x] Focused regressions fail before implementation and pass after it.
- [x] All local pairs receive an inventory audit with no silent repair from final state.
- [x] Player/match split and chronological-label tests prevent training/future leakage.
- [x] One command builds the private Riot cases and evaluates the actual planner.
- [x] Repeated baseline evaluations use the same fingerprint and produce identical metrics;
      aggregate reports are saved. No production behavior change was selected.
- [x] Python contracts, core tests and Clippy pass for touched behavior; runtime/Windows checks if
      app-facing Rust changes. No app restart is needed for offline-only work.
- [x] Priors README and project log updated; deliver logical commits to both branches.
