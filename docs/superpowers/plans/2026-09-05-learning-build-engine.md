# Learning build engine implementation plan

**Goal:** Deliver a beginner-focused itemization companion with correct purchasing, explainable adaptive decisions, player control, and reproducible evaluation.

**Architecture:** Pure Rust catalog/shop/statistics/planner/coaching modules feed the existing Tauri shell. Separate purchase, aggregate, and live-runtime tasks use isolated file ownership; integration is performed by the main agent.

**Tech Stack:** Rust 2021, serde, reqwest, Tauri 2, vanilla JavaScript/CSS, existing fixture JSON.

**Spec:** `docs/superpowers/specs/2026-09-05-learning-build-engine-design.md`

## Global constraints

- Visible-state-only runtime inputs; no hidden enemy information or automated gameplay.
- Recommendations and lessons must be grounded in final decisions and verified mechanics.
- Aggregate-derived base builds; no champion-pack preferred build as an online default.
- Planner performs no I/O and remains inexpensive on a gaming PC.
- Preserve owned items and manual player choices; never imply an automatic sale.
- User approved implementation in the conversation. Execute without repeating design approval gates.

## Task 1: Catalog and shop correctness

Owner: purchase agent. Files: `overlay/core/src/ddragon.rs`, new `overlay/core/src/shop.rs`, purchase tests, `m0/tests/fixtures/item_subset.json`.

Expose `shop::quote(cat: &Catalog, target: u32, inventory: &[InvItem], gold: f64, boots_locked: bool) -> ShopQuote`. The quote contains `remaining_cost: Option<u32>`, `components: Vec<ShopComponent>`, `buy_now: Option<ShopComponent>`, `basket: Vec<ShopComponent>`, `basket_cost: u32`, `affordable: bool`, and `blocked: Option<String>`. A component contains `id`, `name`, `cost`, `owned`. Also expose `shop::compatible(cat, candidate, inventory_ids)` and catalog item effects for the planner. Announce exact additional signatures before integration.

- [x] Add independent-price regression tests and run them red.
- [x] Recursively allocate inventory against recipes, including multiplicity and free footwear equivalence.
- [x] Respect slot capacity after consumption, boots/exclusivity, map/store restrictions, missing data, and exact upgrade costs.
- [x] Generate a small legal basket and prioritize completed upgrades using immediate item stats and deterministic ties.
- [x] Retain numeric item stats and narrow typed effect extraction; document unsupported descriptions as unknown.
- [x] Close fixture recipes using patch-matched official item data and verify targeted tests.

Examples: ER + Long Sword -> 2700g remaining; Navori + two Daggers and Cloak -> 1550g; IE + its three components -> 725g; Steelcaps owned -> Boots purchase blocked; unknown item -> no zero-price purchase.

## Task 2: Honest aggregate evidence

Owner: statistics agent. Files: `overlay/core/src/aggregate.rs`, new `overlay/core/src/statistics.rs`, aggregate/statistics tests.

Keep existing public APIs compatible. Add `Aggregate.core_lines: Vec<Picked>` with serde defaults; preserve the most-picked core as the baseline. Expose Wilson intervals and independent-binomial difference uncertainty, clearly observational. Sort source lists instead of trusting response order. Reject invalid counts/nonfinite rates/invalid rune pages. Time-bound HTTP; validate before promoting a cache; preserve last good cache; retain patch snapshots without adding overlapping sample counts.

- [x] Add real-fixture uncertainty and shuffled-list tests and run red.
- [x] Implement deterministic interval helpers and baseline selection.
- [x] Add alternative line evidence without causal superiority claims.
- [x] Implement bounded HTTP/cache validation and provenance.
- [x] Verify fixture counts (882/1542 vs 384/639) retain the popular order.

## Task 3: Actual state and resilient runtime

Owner: runtime agent. Files: `overlay/core/src/live.rs`, `overlay/core/src/state.rs`, `overlay/src-tauri/src/poller.rs`; only `AggState`/its initialization in `main.rs` if necessary. Coordinate constructor field additions with the main agent. Do not edit UI or command handlers.

Add `Me.rune_ids: Option<Vec<u32>>`, `Me.spell_ids: Vec<u32>`, typed own combat stats, explicit identity failure behavior. Use the observed own position on mid-game start. Decouple remote aggregate refresh from the two-second live loop. Track successful import signatures separately and retry failed imports; respect observed manual spell changes. Surface stale live observations and cancel obsolete fetch results. Reset per-match state correctly.

- [x] Write behavioral tests for actual runes, absent identity, import retry/manual-edit state, and cache expiration.
- [x] Implement pure state helpers as needed and test in WSL.
- [x] Integrate helpers into the shell without holding mutex guards across awaits.
- [x] Verify short champion select, reconnect, same champion across matches, failed remote fetch, and live source age.

## Task 4: Coherent adaptive planner and lessons

Owner: main. Files: `engine.rs`, new `coaching.rs`, `pack.rs`, factual trait data, core module exports, integration tests.

- [x] Add regressions for affordable IE, locked inventory, missing runes, unsafe cleanse claims, differing skill openings, and mode boundaries.
- [x] Generate aggregate-supported candidate paths; preserve owned inventory and enforce shop compatibility.
- [x] Score completion cost, observed resistance/healing/dive needs, overlapping coverage, and opportunity cost in one deterministic comparison.
- [x] Keep the current popular core when no justified change wins; remove KDA price ordering and outdated pack-default insertion.
- [x] Support an explicit survival preference and target pin via `plan_with_preferences`, retaining `plan` as the default wrapper.
- [x] Generate final-action explanations and short reusable lessons; provide the nearest valid alternative.
- [x] Handle ability rank legality and opponent-role ambiguity conservatively.
- [x] Validate the shared planner against real marksman, mage, tank, fighter, support, and jungle aggregates; derive itemization archetypes from data, not Xayah-specific eligibility checks.

## Task 5: Player controls and local learning record

Owner: main after runtime integration. Files: shell state/commands/poller hooks, UI, new journal module.

- [x] Add session-scoped balanced/survival preference, compatible target selection, and return-to-auto commands.
- [x] Keep the primary view focused on purchase, price, and one reason; expand for lesson, basket, and alternatives.
- [x] Record significant decision changes, observed purchases, versions, and feedback locally with bounded retention.
- [x] Show a post-game recap and feedback controls without unsupported performance judgments.
- [x] Verify controls, error states, contrast, keyboard access, long item names, and reduced motion in a browser.

## Task 6: Replay, review, and handoff

Owner: main plus independent reviewer. Files: new replay binary and reviewed scenarios, README, design/log updates.

- [x] Implement an offline replay CLI that accepts saved snapshots/catalog/aggregate and reports changes, invalid actions, and planner latency.
- [x] Run regressions, golden scenarios, captured-session replay, Rust linting, and JavaScript checks.
- [x] Independently review implementation and repair actionable findings.
- [x] Build/check the Windows shell where supported; avoid changing the active game session.
- [x] Record exact verification results and update the product/design documentation.

## Progress

- Design approved through the review and scope discussion; beginner-learning emphasis confirmed.
- Baseline: clean checkout on new branch; previous core verification 36 passing tests.
- Tasks 1–3 have disjoint ownership. Main-owned integration waits for each relevant interface before edits to their files.

## Final verification

- The shared engine passed 208 core/CLI/integration tests, plus 17 headless runtime, 12 browser, and 34 existing Python tests.
- Strict core and headless-runtime Clippy, changed-Rust formatting, JavaScript syntax, and whitespace checks pass.
- Release replay checked 115 saved visible-state snapshots with zero detected violations; p95 planner time was 2.380 ms in WSL. These are unverified match groups, not independent games or win-uplift evidence.
- Independent review findings were repaired with regressions, including preservation of request identity while waiting on the rune-import mutex.
- A separate Windows release candidate was built without replacing or launching the running overlay. Live client behavior and game-resource impact remain manual verification, not automated-test claims.
- Product, design, README, and historical log were updated. Detailed limitations and commands are in `docs/notes/engine-v2.md`.
- Work remains on `feat/learning-build-engine`; no merge, push, game/account write, or deployment is part of this handoff.
