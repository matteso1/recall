# Featherstorm

A local League of Legends companion that gives you one clear recommendation:
**what to buy next, what it costs with your inventory, and why.** Learn through
repeated good decisions; explanations and alternatives are optional.

This is a general champion-and-role tool. Xayah is a test fixture and the owner's
main, not the product boundary. Loadouts come from each champion's own role data;
the planner is shared across marksmen, mages, fighters, tanks, supports, and junglers.

## What is implemented

- Current-patch aggregate loadouts: starting items, core, boots, runes, spells, and
  standard ability order. Most-picked builds are the baseline; higher observed win
  rates are not treated as proof that a different build is better.
- Inventory-aware purchasing: recursive component credit, exact combine prices,
  repeated components, legal purchase baskets, save-for amounts, six-slot capacity,
  item-family restrictions, actual Magical Footwear, and support/jungle requirements.
- One compositional comparison of completion value, existing investment, visible
  resistance/healing/dive pressure, coverage, and delay. No ordered slot-swapping
  rules or preferred builds hidden in champion packs.
- A compact next-action panel, with optional **Why & options**, **More protection**,
  target pinning, and return to **Auto**. Player purchases are preserved; no automatic sales.
- Champion-select imports with retry/session guards and in-place rune-page updates.
  Spell imports preserve Flash's key and observe manual spell changes.
- Swiftplay pre-queue preparation for both champion/role choices, with separate saved
  runes, spells, role-specific shop sets, and confirmed per-choice readiness.
- Independent local-data polling and bounded remote refreshes. Stale or unidentified
  live data pauses actionable advice—even if the UI stops receiving updates.
- A bounded, local recommendation/purchase journal and optional post-game feedback.
  This is a decision recap, not a performance grade or a claim about wins.
- Offline replay, real aggregates for eight champion/role fixtures, scenario
  regressions, headless runtime tests, and browser tests.

See [design](docs/design.md), [product principles](PRODUCT.md), and
[verification notes](docs/notes/engine-v2.md).

## Current boundaries

Supported live modes are standard Summoner's Rift, Swiftplay, and Practice Tool.
ARAM/Arena and unknown modes pause recommendations instead of reusing ranked builds.
Missing champion/role data never falls back to another champion or role. A small
sample is shown as weak evidence.

Generic skill-point guidance is deliberately disabled for Aphelios, Udyr, Jayce,
Elise, Nidalee, and Karma until their nonstandard leveling is modeled. Their
itemization still uses the shared planner. Not every champion/passive interaction
has been modeled or tested; recognized item effects are narrow and patch-sensitive.

There is no combat positioning, wave-state inference, enemy cooldown tracking,
recall-timing oracle, or local LLM. Visible equipment value is not enemy gold.
The scoring weights are explicit heuristics, not a trained win-probability model.

In Swiftplay, open the overlay in the lobby and wait for both choices to be ready
before queueing. Loadouts are saved to the two choices, not to a shared active rune
page. Picks, roles, skins, Flash keys, and subsequent manual rune/spell edits are
preserved. Preparation pauses when queueing starts; launching after assignment
cannot repair missed pre-game imports. The live panel follows the actual assigned
champion and adapts after enemies become visible. The shop may not reload an item
set already cached at game start. Only queue 480's pre-queue API is verified.

## Build and run

The Rust core runs in WSL/Linux. The actual overlay runs on Windows with the
Windows Rust/MSVC toolchain, VS C++ Build Tools, and WebView2.

```bash
scripts/cargo-win.sh build --locked --release -p featherstorm
scripts/overlay-run.sh
```

Do not replace the executable while an overlay/game session is running. For a
separate candidate build, pass an absolute Windows directory beneath the mirror's
excluded `overlay/target/` to Cargo's `--target-dir`. Set `FEATHERSTORM_NICE=1`
to run the Windows build at below-normal priority.

The panel is draggable/collapsible. Auto-import switches, source region/tier, and
saved position live in `%LOCALAPPDATA%\Featherstorm\settings.json`.
Caches, logs, and the bounded decision journal also stay in that directory.
The default source is global / emerald-plus; that population is not a personalized
estimate for a beginner.

`featherstorm.exe --demo ingame` (or `champselect`, `idle`) is an explicitly
labeled, read-only staged preview. It may fetch public catalog/aggregate data but
does not connect to the client or import anything. `--probe` writes a diagnostic
report using the actual champion/role when available, otherwise a labeled sample.

## Verification

From the repository root:

```bash
cargo test --manifest-path overlay/Cargo.toml --locked -p featherstorm-core
cargo clippy --manifest-path overlay/Cargo.toml --locked -p featherstorm-core --all-targets -- -D warnings
cargo test --manifest-path tests/runtime/Cargo.toml --target-dir overlay/target --locked
cargo clippy --manifest-path tests/runtime/Cargo.toml --target-dir overlay/target --locked --all-targets -- -D warnings
python3 -m unittest discover -s m0/tests -v
node --check overlay/ui/app.js
```

Browser tests use real serialized Rust plans and mock only the Tauri connection.
They do not contact League:

```bash
cd tests/ui
npm ci
npx playwright install --with-deps chromium
npm test
```

Offline replay (from the repository root):

```bash
cargo run --manifest-path overlay/Cargo.toml --locked -p featherstorm-core --bin replay -- --fixtures
cargo run --manifest-path overlay/Cargo.toml --locked -p featherstorm-core --bin replay -- --fixtures --json
cargo run --manifest-path overlay/Cargo.toml --locked -p featherstorm-core --bin replay -- \
  --session m0/tests/fixtures/captured \
  --items m0/tests/fixtures/item_subset.json \
  --champions m0/tests/fixtures/champion_subset.json \
  --aggregate m0/tests/fixtures/opgg_xayah_adc.json
```

Use a matching catalog, aggregate, and separately captured match directory.
Replay tests legality and consistency on recorded states; it cannot tell you what
would have happened if the player had followed a different recommendation.

## Local APIs and policy boundary

LCU authentication comes from the client's lockfile. Live game observations come
only from Riot's local Live Client Data API: own gold/inventory/abilities, visible
rosters/items/scoreboard, and game time. No memory reading, injection, hidden
positions, enemy gold, or cooldown inference; no automated gameplay.

Public Data Dragon and op.gg aggregates provide the offline knowledge layer.
op.gg's endpoint is not a licensed feed; six-hour caching is not permission to
redistribute its data. Last validated caches can be used with a stale label;
without compatible data, advice pauses.

Featherstorm is an independent prototype, not Riot-approved or endorsed.
Recommendations remain optional and explainable. Local API access alone does
not certify compliance; review and registration are required before wider release.
See [Riot's developer policies](https://developer.riotgames.com/docs/lol/).

## Repository

- `overlay/core/`: catalog, shop, aggregate evidence, planner, lessons, journal,
  session guards, and replay CLI; no window dependency.
- `overlay/src-tauri/`: Windows window, independent polling, local commands,
  guarded imports, and asynchronous journal persistence.
- `overlay/ui/`: compact action-first view; no item selection in JavaScript.
- `m0/tests/fixtures/`: real aggregate/live captures, patch-matched catalog subsets,
  and [source provenance](m0/tests/fixtures/AGGREGATE_SOURCES.md).
- `tests/runtime/`, `tests/ui/`: shell-controller and browser regression harnesses.
- `data/pack/`: factual matchup notes, labels, and weak champion-trait priors.
  Legacy preferred build/rune/spell fields are not used as planner defaults.
- `m0/`: original Python integration probes. `push_itemset.py` writes to the
  client; use its `--dry-run` when only inspecting.
- `docs/notes/`: implementation history and verification limitations.
