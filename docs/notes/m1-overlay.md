# M1 overlay: how it is put together

Two Rust crates in `overlay/` (a cargo workspace) plus a static web UI.

## `overlay/core` - the brain (platform-independent, tested in WSL)
| Module | Job |
|---|---|
| `lcu.rs` | lockfile discovery + authenticated HTTPS to the client: gameflow phase, champ select session, summoner, item sets, rune pages, summoner spells |
| `live.rs` | Live Client Data (`:2999`): `summarize()` -> me (gold, items, ability levels, KDA), allies, enemies |
| `champselect.rs` | session -> `Lobby` (my cell/champion/position, ally + enemy champion ids, bans) |
| `ddragon.rs` | Data Dragon catalog cached under `%LOCALAPPDATA%\Featherstorm\ddragon\<patch>`: items, champions, runes, by id and by normalised name |
| `pack.rs` | the data pack, embedded at compile time from `data/pack/` (`xayah.json`, `champion_traits.json`) |
| `engine.rs` | rules -> `Plan`: ordered path with tags/why, NEXT item with components and "buy now", skill point, matchup line |
| `itemset.rs` | `Plan` -> LCU item set (block titles capped at 30 chars) |
| `runes.rs` | pack rune page -> LCU perk page; summoner spell ids |
| `state.rs` | `PanelState`, the JSON the panel renders |

Run the tests from WSL: `cd overlay && cargo test -p featherstorm-core`.

## Rules (engine.rs), in the order they run
1. Lane matchup from the pack (line, optional first item / start / spells).
2. Healing on their team -> anti-heal item replaces the armor-pen slot (`Mortal Reminder over LDR: Soraka heals`).
3. Two or more tanks -> armor pen one slot earlier.
4. Lockdown ult -> the defensive slot becomes the cleanse item (Mercurial).
5. Mostly magic damage among their carries -> Maw instead of GA.
6. Poke lane without assassins -> Bloodthirster instead of GA.
7. Live: an enemy stacking armor items -> armor pen earlier.
8. Live: behind (3+ deaths, <=1 kill) -> Navori before IE.
9. Two assassins, or one fed assassin (live) -> defensive item at slot 4.

Champion traits come from `data/pack/champion_traits.json` (164 champions); unknown champions fall
back to Data Dragon class tags. Every rule that changes the path pushes one line to `plan.why`.

## `overlay/src-tauri` - the shell (Windows only)
- `main.rs`: window setup (380x300, transparent, always on top, no decorations, remembers position),
  shared `App` state, logging to `%LOCALAPPDATA%\Featherstorm\featherstorm.log`.
- `poller.rs`: 1 s loop. Finds the client, follows the gameflow phase, polls champ select (1 s)
  or live data (2 s), runs the engine, publishes `PanelState` on the `state` event when it changed.
  Flashes the recommended skill for 3.5 s on level-up.
- `commands.rs`: `get_state`, `import_item_set`, `import_runes`, `import_spells`, `set_collapsed`, `quit`.
- `probe.rs`: `featherstorm.exe --probe` runs the pipeline once without a window and prints JSON
  (also saved to `%LOCALAPPDATA%\Featherstorm\probe.json`).
- `ui/`: plain HTML/CSS/JS, no bundler. `window.__TAURI__` (withGlobalTauri) for events and commands.

## Building and running (from WSL)
```bash
scripts/cargo-win.sh build --release      # mirrors overlay/ + data/pack/ to C:\Users\<you>\code\featherstorm-win and builds there
scripts/overlay-probe.sh [debug|release]   # headless pipeline check, no window (Data Dragon, client, engine)
scripts/overlay-run.sh                    # launch the release exe on Windows (detached)
scripts/overlay-log.sh 40                 # tail %LOCALAPPDATA%\Featherstorm\featherstorm.log
scripts/win-screenshot.sh                 # full-DPI screenshot into .screens/ to eyeball the panel from WSL
scripts/overlay-stop.sh                   # kill it
```

## Dogfooding checklist (M1)
1. Start the overlay with the client open: panel shows "In lobby" and the summoner name.
2. Practice Tool as Xayah: champ select shows the path, matchup line (none in Practice Tool), the
   Runes / Spells / Item set buttons; each button turns green with a check when the client accepted it.
3. In game: NEXT shows the first path item with components, "Buy now" flips to affordable
   components as gold comes in, bought components get a check, the path line checks off finished
   items, the skill key flashes on level-up.
4. Draft game: enemy locks change the path (Soraka -> Mortal Reminder with a `(Soraka)` tag and a why line).
5. Position survives a restart (settings.json), collapse button shrinks to one line.
The binary lands in `C:\Users\<you>\code\featherstorm-win\overlay\target\release\featherstorm.exe`.
Requirements on Windows: Rust MSVC toolchain, Visual Studio Build Tools with the "Desktop development
with C++" workload (the MSVC linker), WebView2 (ships with Windows 11).

Why the mirror: cargo cannot build from a `\\wsl.localhost` path (lock files fail), and even
`cargo check --target x86_64-pc-windows-msvc` from WSL needs `lib.exe`, so the Windows side owns the
build. The WSL repo stays the source of truth.

## Panel state contract (what `ui/app.js` renders)
`phase` (noclient | idle | champselect | loading | ingame), `summoner`, `champion`, `supported`,
`lobby {allies, enemies, my_position}`, `plan` (see `engine::Plan`), `live {game_time, gold, level, kda}`,
`flash {skill, until_ms}`, `imports {itemset, runes, spells}` (idle | working | done | error: ...),
`message`, `collapsed`, `ddragon`, `version`.
