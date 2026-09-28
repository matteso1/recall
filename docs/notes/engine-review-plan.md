# Engine review and offline evaluation

The brief is `engine-v3-design.md` and the owner's September 27 handoff. Work inline in
the WSL checkout; Claude is paused. Do not interrupt a live game.

- [x] Verify `64ad7a8`: core/runtime tests, Clippy, Windows check, recorded-game replay.
- [x] Build one offline comparison command that calls the production Rust planner,
  carries preferences between observations, and shares replay's legality validator.
  Train next-item, answer and aggregate inputs without held-out players or matches.
  Keep validation and final test players separate. Record input hashes, exclusions,
  missing features and denominators. Compare identical cases and report failures.
- [x] Test and fix disagreement between learned path candidates and purchase candidates.
- [x] Evaluate corpus boot selection against the existing heuristic; ship only if the
  validation comparison supports it. Assess detour timing separately from eventual ownership.
- [x] Document remaining constants, evidence, measurement limits and next priorities.
- [x] Verify final changes, commit each independently, push the two requested branches,
  and install only when League is idle.

The corpus has minute inventories for tracked players, not full Live Client captures.
The evaluator must identify missing enemy equipment, full rune pages and kill-feed events.
It must report minute-to-minute flips separately from identical-state repeat flips and
real recorded poll flips. Agreement measures imitation of observed purchases, not win uplift.
Use the recent shop-compatible patches for scoring; do not invent missing opponent stats.
Retain the existing recorded games for runtime behavior and live observation gaps.

Regression checks: intentional held-out labels must not alter training inputs; player and
match sets must be disjoint; artifacts must fail closed if training metadata is missing;
preferences reset at game boundaries; target-owned observations cannot count as predictions;
an engine selected path item must be eligible for its purchase scorer.
