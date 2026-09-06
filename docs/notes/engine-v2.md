# Engine v2: implementation and verification

Date: 2026-09-05 (local development date). Engine decision version: `2.0`.
App/package version remains `0.1.0`. Branch: `feat/learning-build-engine`.

## Product change

The default experience is one concrete recommendation, not a lesson to complete.
The primary view shows a legal shop purchase or saving step, price, one reason,
the compact path, and supported skill-point guidance. **Why & options** contains
optional explanation, tradeoff, alternative, protection preference, and target pin.

The Impeccable interface pass kept the existing compact navy/gold/teal identity,
reduced default information load, preserved focused controls across live updates,
and added keyboard/reduced-motion and stale/unknown-price handling. There is no
generated-image dependency, frontend framework, or local model.

This is not Xayah-specific. Real role aggregates for Xayah, Ahri, Ornn, Darius,
Lulu, Lee Sin, Aphelios, and Udyr exercise the same engine. Special champions retain
item guidance while unsupported ability-rank patterns are explicitly withheld.
See [fixture provenance](../../m0/tests/fixtures/AGGREGATE_SOURCES.md).

## What changed under the hood

- `shop.rs` allocates owned recipe nodes once, quotes exact remaining costs,
  simulates capacity after consumption, respects item families/map/store/loadout,
  and constructs a small legal basket. Unknown price is an error, not zero.
- `decision.rs` compares aggregate-backed candidates compositionally. Popularity,
  completion, investment, situational coverage, and delay contribute to a trace.
  Owned finished items/quests are commitments. There is no automatic selling.
- Item effects use numeric stats and narrow verified descriptions. Physical and
  magical pressure are equipment/level weighted; visible resistance amounts replace
  two-item tag counts. Champion booleans remain uncertain priors.
- `statistics.rs` supplies Wilson and Newcombe intervals. Popular core order
  survives the old Xayah 57.2% versus 60.1% threshold example; the observed
  difference interval is approximately −1.66 to +7.37 percentage points.
- Aggregate responses are sorted/validated, time/size bounded, and independently
  cached by champion/role/source with provenance. Overlapping snapshots are not pooled.
- Actual own rune/spell/ability state drives purchase/skill constraints. Known
  opposing roles outrank ambiguous trait-based lane guesses.
- Shell session/source guards prevent stale state, obsolete fetches, wrong-match
  acknowledgments, and stale local preferences. Network waits do not block local
  planning. The UI has its own source-age watchdog.
- Rune refreshes PUT one existing editable app page in place. A failed update does
  not delete the page or fall through to POST. Non-app pages are never deleted.
- `journal.rs` and the asynchronous store keep bounded decision/purchase receipts
  in `%LOCALAPPDATA%\Recall\decisions.json`. Corruption is preserved before
  new writes; failures are visible. This is not a training dataset with outcome labels.
- `replay` exercises production plans on offline fixtures/captures and emits
  machine-readable JSON when requested, including score traces and data-gap counts.

## Verified behavior

Fresh core verification:

```sh
cargo test --manifest-path overlay/Cargo.toml --locked -p recall-core
cargo clippy --manifest-path overlay/Cargo.toml --locked -p recall-core --all-targets -- -D warnings
```

**208 passing Rust core/CLI/integration tests:** 146 library, 16 replay, 23 planner
regressions, 16 purchase-context, and 7 universal-planner tests. Strict core Clippy
passes. All 34 existing Python M0 tests pass; JavaScript syntax checks pass.

The independent review found and drove regressions for components mistaken for
finished first items, full inventories blocking legal support transformations,
blocked choices outranking purchasable combines, starter-item exclusions, cheap
finished items such as Mejai's, and known support roles being overridden by trait
guesses. Further regressions cover zero-cost pins and non-destructive rune refreshes.

The browser suite has **12 passing tests** using actual Rust replay output and
mocked Tauri commands: primary/optional hierarchy, command arguments, focus across
updates, stale/unknown data, saving amounts, errors, narrow/long-name layout,
escaped markup, recap feedback, keyboard/reduced motion, event-stream expiry,
read-only labeled demo, and all eight champion fixture renderings. Some of those
checks share a single test case.

```sh
cd tests/ui
npm ci
npx playwright install --with-deps chromium
npm test
```

Development-container detail: Chromium needed missing NSPR/NSS/ALSA libraries.
They were downloaded into a task-owned temporary directory and supplied through
`LD_LIBRARY_PATH`; no system package or application setting was changed. Browser
tests use a local file URL and abort external image/API requests. That proves
fallback readability, not successful CDN image delivery.

All **17 headless runtime tests pass**, with strict Clippy passing. They compile
the actual controller, poller, probe, queue, and
journal-store modules with a window-emission stub and inert import wrappers:

```sh
cargo test --manifest-path tests/runtime/Cargo.toml --target-dir overlay/target --locked
cargo clippy --manifest-path tests/runtime/Cargo.toml --target-dir overlay/target --locked --all-targets -- -D warnings
```

They cover local preference changes, completion/unpin, stale identity, match
boundaries, feedback persistence, corrupt-journal preservation, coalesced writes,
actual-champion probe targeting, and a queued rune import retaining its original
generation after Ahri → Lulu → Ahri. That last regression reproduced the race
before the fix; independent review confirmed it closed afterward. These tests do
not run the poller or replace a Windows/Tauri build.

## Replay measurements

Release-mode command, from `overlay/`:

```sh
cargo run --release --locked -p recall-core --bin replay -- \
  --session ../m0/tests/fixtures/captured \
  --items ../m0/tests/fixtures/item_subset.json \
  --champions ../m0/tests/fixtures/champion_subset.json \
  --aggregate ../m0/tests/fixtures/opgg_xayah_adc.json
```

Recorded-state result: **115 snapshots / 115 plans / 115 quote checks**, with zero
detected invalid recommendations, blocked targets, or unpriced targets. Nearest-rank
planner latency in this WSL release run: p50 **2.037 ms**, p95 **2.380 ms**, p99
**2.471 ms**, max **2.500 ms**. This excludes network, UI, and Windows game performance.

There were 31 target changes and 66 action changes. The directory combines
captures with **unverified match boundaries**: do not call it one match, 115 games,
or use its transitions to claim reduced in-match churn. Eight irrelevant JSON and
four non-JSON files were skipped explicitly. Baseline deviation was 27/91 eligible
decisions; that is descriptive, not a quality objective.

`--fixtures` separately produced 24 plans across eight real aggregate fixtures,
with 16 explicitly synthetic live states and zero detected violations. Synthetic
state construction is disclosed in replay output; these are not recorded matches.

The replay validator reuses the production shop quote, so it is a consistency
check, not an independent shop implementation. Regression tests separately assert
known arithmetic (for example IE's 725g combine) and legal/illegal inventory cases.
Neither test method proves winning builds or causal benefit.

## Windows handoff and limitations

The Windows Tauri shell has compiled and a separate optimized candidate executable
has been built beneath the mirror's excluded `overlay/target/candidate/release/`.
The active release executable has not been replaced; neither the candidate nor an
overlay window has been launched during this work. No game/account writes were
used for verification. The read-only LCU help metadata lists the page-update
operation; mock HTTP tests verify the PUT path/body and preservation on failure.

Actual rune selection after PUT, one-second-select behavior, shop refresh behavior,
Windows memory/FPS impact, overlay focus while playing, and other real-client
interactions still need a controlled manual check between games. The existing
automated tests cannot certify them.

Before making tactical effectiveness claims: collect separately identified
sessions, review decisions blind to engine version where practical, measure
wrong/unsupported advice and unnecessary target changes, and annotate which
recommendations were usable at a real shop opportunity. Do not treat following a
recommendation as proof it caused a win. Match-v5 learning and causal comparison
are future data work, not hidden functionality in this build.

## Update 2026-09-06: assigned-role fallback and Swiftplay shop

The first real Swiftplay game (Irelia assigned Jungle, op.gg data only for Top and Mid) left the
panel waiting for build data. The engine now plans from a labelled same-champion fallback
(`Plan.source_position`), keeps the assigned role for every role rule, and knows the Swiftplay shop
(`engine::GameMode`, `shop::ShopContext.swiftplay`). Details, research and the offline verification
are in [swiftplay.md](swiftplay.md). Counts after the change: 233 core tests, 35 runtime, 20 browser,
34 Python; Clippy clean on core and the runtime crate.
