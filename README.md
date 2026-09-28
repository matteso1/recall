# Recall

A small always-on-top panel for League of Legends that tells you **what to buy next,
what it costs with the items you already hold, and why**. It prepares your runes, spells
and shop item set in champion select, then follows the game and updates the
recommendation as the enemy team's items become visible.

Recall reads only what the League client and the in-game Live Client Data API already
show you. It never touches memory, packets, or anything hidden, and it never plays for you.

*Recall was called Featherstorm until September 2026.*

<p>
  <img src="docs/images/champ-select.png" width="46%" alt="Champion select: loadout ready, build path, runes and spells imported">
  <img src="docs/images/swiftplay-lobby.png" width="46%" alt="Swiftplay lobby: both champion choices prepared">
</p>

## What it does

- **One recommendation at a time.** The next item, the component to buy right now, the
  remaining price, and a one-line reason. "Why & options" is there if you want it.
- **Builds from real data, not hand-written defaults.** Covered champions use Master+ purchase
  sequences for legendary items and enemy-composition boot choices. Starting items, runes,
  spells, skill order and fallback builds use op.gg's champion API, cached locally. If your champion has no data
  for your assigned role, the same champion's most-played role is used and clearly labelled.
- **Shop-legal purchasing.** Component credit, exact combine prices, six-slot capacity,
  item families that exclude each other, Magical Footwear, support and jungle requirements,
  Swiftplay's shop rules. When you cannot afford the next item, it says how much you are short.
- **Situational adjustments you can see.** Anti-heal against a healer, a cleanse against a
  verified suppression, armor or magic resist against what the enemy actually built. Nothing
  changes for reasons the panel cannot state.
- **Champion-select and Swiftplay preparation.** Rune page, summoner spells and an in-shop
  item set are imported for you; in Swiftplay both of your choices are prepared before you queue.
  Ban suggestions use your champion's matchups, with labelled op.gg coverage when the Master+
  sample is too small, before falling back to generic role advice.
- **Honest states.** Stale data, unknown modes, or a missing build pauses the advice instead
  of guessing. A short post-game recap shows the decisions; it never grades you.

## Getting started (Windows)

There is no installer yet; Recall is built from source. You need the Rust toolchain with the
MSVC target, Visual Studio C++ Build Tools, and WebView2 (already present on Windows 11).

```bash
git clone https://github.com/matteso1/recall.git
cd recall
cargo build --manifest-path overlay/Cargo.toml --release -p recall
overlay/target/release/recall.exe
```

The project is developed from WSL with the build running on the Windows side; the scripts in
`scripts/` (`overlay-build.sh`, `overlay-run.sh`, `overlay-probe.sh`) do that. See
[docs/notes/dev-setup.md](docs/notes/dev-setup.md).

To open Recall like any other app, put a **Recall** shortcut on your Desktop and in the Start menu:

```bash
scripts/shortcut-install.sh           # `remove` undoes it, `status` shows it
```

Clicking it while Recall is already running brings the panel back instead of starting a second
copy; the panel's x button quits. `scripts/overlay-update.sh` rebuilds and relaunches it in one
step, waiting until you are out of queue, champion select and games.

If you would rather have Recall follow the League client by itself, `scripts/autostart-install.sh`
adds `recall.exe --autostart` to your Startup folder instead: the panel then stays hidden until the
client is running and hides again 20 seconds after it closes.

Then:

1. Open Recall and League. The panel says **ready** while you are in the client.
2. Pick a champion. Runes, spells and the item set are imported when the pick locks; the panel
   shows the build path and the matchup notes. Turn any of the imports off in **Why & options**.
3. In game, buy what the panel says when you recall, or pin a different target. Your own
   purchases are always respected; nothing is ever sold.
4. In **Swiftplay**, open Recall in the lobby and wait for both choices to read **Ready** before
   you queue. Its champion select lasts one second, so preparation has to happen beforehand.

Settings (auto-import switches, data region and tier, saved position), caches, logs, the
decision journal and your recorded games live in `%LOCALAPPDATA%\Recall`. Every game Recall watches
is saved in its `games` folder (the newest 40), so a game can be reviewed afterwards.

## Modes and limits

Supported: Summoner's Rift (draft, blind, ranked), Swiftplay, Practice Tool. ARAM, Arena and
unknown modes pause recommendations rather than reuse Rift builds.

Recall never substitutes another champion's data. A small sample is shown as weak evidence.
Skill-point guidance is off for Aphelios, Udyr, Jayce, Elise, Nidalee and Karma until their
levelling is modelled. There is no wave-state inference, cooldown tracking, positioning advice
or trained win-probability model; the scoring weights are explicit, reviewable heuristics.

## Riot policy and privacy

Recall uses two local interfaces Riot provides for this purpose: the League Client API
(authenticated with the client's own lockfile) and the Live Client Data API. From them it reads
your gold, inventory, abilities, the visible rosters and scoreboard, and the game time.

It does not read process memory, capture packets, inject into the game, infer hidden positions,
enemy gold or cooldowns, or automate any gameplay. The only things it writes to your account
are rune pages and item sets named `Recall <champion> <role>`, which it also reuses and replaces
(its own oldest sets are removed when the client's upload limit is near); personal pages and other
apps' sets are never edited or deleted. Recorded games hold only what the scoreboard shows, stay on
your PC and are never uploaded. Nothing leaves your machine except requests for public patch data
and public build statistics.

Recall isn't endorsed by Riot Games and doesn't reflect the views or opinions of Riot Games or
anyone officially involved in producing or managing Riot Games properties. Riot Games and
League of Legends are trademarks or registered trademarks of Riot Games, Inc.

## Roadmap

- macOS support (the brain is portable; the window shell, screenshots and build scripts are Windows-only today).
- A packaged installer and signed release builds.
- More modes as their shops are verified (ARAM first).

Open an issue if you want to help with any of these.

## Development

Layout: `overlay/core` is the platform-independent brain (Rust), `overlay/src-tauri` is the
Windows window shell, `overlay/ui` is the panel (plain HTML, CSS, JS). `m0/` holds the original
stdlib-only Python probes for the local APIs, still used for capturing fixtures. The design doc is
[docs/design.md](docs/design.md); decisions and findings are in [docs/notes](docs/notes).

```bash
cargo test --manifest-path overlay/Cargo.toml --locked -p recall-core
cargo clippy --manifest-path overlay/Cargo.toml --locked -p recall-core --all-targets -- -D warnings
cargo test --manifest-path tests/runtime/Cargo.toml --target-dir overlay/target --locked
cargo clippy --manifest-path tests/runtime/Cargo.toml --target-dir overlay/target --locked --all-targets -- -D warnings
python3 -m unittest discover -s m0/tests -v
node --check overlay/ui/app.js
cd tests/ui && npm ci && npx playwright install --with-deps chromium && npm test
```

Browser tests render real serialized plans from the Rust core and mock only the window bridge.
Nothing in the test suites contacts League or the network.

The [full planner backtest](tools/priors/README.md#full-planner-evaluation) compares changes on
held-out Master+ timelines: purchase agreement, target stability and shop legality. Its local
data and generated cases stay outside Git. The [engine review](docs/notes/engine-review.md)
records measurement limits and the remaining heuristic terms.

Offline replay checks every recorded state of a captured game for legality:

```bash
cargo run --manifest-path overlay/Cargo.toml --locked -p recall-core --bin replay -- --fixtures
cargo run --manifest-path overlay/Cargo.toml --locked -p recall-core --bin replay -- \
  --session m0/tests/fixtures/captured \
  --items m0/tests/fixtures/item_subset.json \
  --champions m0/tests/fixtures/champion_subset.json \
  --aggregate m0/tests/fixtures/opgg_xayah_adc.json
```

Headless checks of the built executable: `recall.exe --probe --champion Irelia --role jungle --swiftplay`
plans a request against the real cache with no client or game; `recall.exe --demo champselect|ingame`
shows the panel with staged data and imports nothing.

## License

[MIT](LICENSE).
