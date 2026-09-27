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

## Update 2026-09-26: resistances valued against what you already have

In a draft game Malphite Top followed op.gg's all-armor line (Sunfire Aegis, Plated Steelcaps,
Thornmail, Frozen Heart) against a Teemo top lane; at 22:54 the panel still said "Thornmail: armor
for Yone's damage profile" with 259 armor and 50 magic resist, and magic resist first appeared as
the fifth item. Three causes: a resist's value divided by resists from owned items only (Malphite's
natural armor was invisible), the damage split ignored who you actually face in lane, and a
situational magic-resist item could not outrank the next core item.

- **Damage removed, not stat totals.** Armor or magic resist is valued as the share of all incoming
  damage it removes: each damage type's share after current mitigation times the part the new
  resistance stops. In game the base is the client's measured armor, magic resist and health; before
  a game, Data Dragon base stats at level 9 plus items (`Champion::stat_at`). One documented scale
  (`RESIST_SCALE`) keeps early single items near the old scores. When the measured balance drives a
  choice the reason gives the numbers: "Rookern: 259 armor vs 50 MR; Teemo deals magic damage".
- **Lane phase.** The lane opponent (both bot-lane enemies for ADC and support) counts x3 through
  10:00, fading to x1 at 20:00. Draft hides enemy positions, so before the game the opponent is only
  weighted when exactly one enemy can play the lane.
- **Defensive order.** Unowned defensive items on the path (no damage stats) are ordered greedily by
  effective health per remaining gold against the enemy mix, owned components counted; damage items,
  boots and the first core item keep their places, and an item moves ahead only by a 15% margin.
- **Boots.** Among the aggregate's boots of the build's family bought by at least 5% of players, the
  pick-rate prior is weighed against three times the defensive fit: Mercury's Treads in lane against
  Teemo, Plated Steelcaps against an all-physical lineup.

Replayed on the Malphite states (`core/tests/resist_balance.rs`, rebuilt by hand from the scoreboard):

| State | Before | After |
|---|---|---|
| Champion select | Sunfire > Steelcaps > Thornmail > Frozen Heart > Rookern > Jak'Sho | Sunfire > Steelcaps > Rookern > Jak'Sho > Thornmail > Frozen Heart |
| 8:00 in lane | boots Plated Steelcaps | boots Mercury's Treads |
| 15:30 | next Thornmail ("reduces Yone's healing") | next Kaenic Rookern ("200 armor vs 55 MR; Teemo deals magic damage") |
| 22:54 | next Thornmail ("armor for Yone's damage profile") | next Kaenic Rookern ("259 armor vs 50 MR"), then Thornmail |

All earlier recorded-game regressions pass unchanged.

## Update 2026-09-26: who is actually killing you, and a steadier panel

In a draft game Lux Mid won lane against Azir while Darius went 1/0 at 1:24, 6/0 at 13:54 and 14/1
by 20:24; he killed Lux at 13:48 (Azir assisting), assisted at 16:12 and killed her again at 19:36.
The panel kept the core line (Luden's, boots, Stormsurge) with Zhonya's fifth all game, and its tags
and tail order flickered: 27 polls flipped a tag and the path reordered 11 times.

- **Kill feed and scoreboard.** `LiveSnapshot::my_deaths` reads the active player's deaths from the
  kill feed (killer and assisters mapped from Riot game names to enemy champions; a name two rows
  share is dropped). An enemy's threat now also rises with a kill lead (8% per kill ahead, up to six)
  and with recent involvement in your deaths (killer 1, assister 0.5, fading over six minutes, x0.4
  threat per point), and that involvement counts as dive: an enemy who reached and killed you is
  diving you, assassin trait or not.
- **Detour timing.** With that evidence, the planned defensive answer with a real need (`DETOUR_NEED`)
  and tied to the enemy (the buffer the kill feed calls for, or the resistance against that enemy's
  damage) moves ahead of the core, after the first core item; the reason names the evidence
  ("Zhonya's: Darius (6/0) killed you; stasis stops the all-in"). It stays promoted until its need
  drops below 1.2.
- **Stability.** The cause of the flicker was the threat weight's ratio of levels (level 3 against 2
  counted 50% more dangerous), which swung pressure between 0.07 and 0.61 on early level-ups and moved
  defensive scores across the tag threshold. Levels now count 10% per level of difference; the tail
  keeps the previous plan's choice unless a candidate is 0.25 better, tags turn off 0.15 below their
  threshold, and a target keeps the tag its path entry had.

Replay of the recorded game (185 observations): tag flips 27 to 1, path reorders 11 to 3, target
changes 4 to 6 (Zhonya's at 13:51, boots when affordable at 15:13, Zhonya's again at 15:27), Zhonya's
first the target at 13:51 instead of never; 0 invalid recommendations before and after. Tests are in
`core/tests/threat_awareness.rs`; the replay tool now rebuilds the whole kill feed on every line of
a recorded game.
