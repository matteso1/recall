# M1 overlay: how it is put together

Two Rust crates in `overlay/` (a cargo workspace) plus a static web UI.

## `overlay/core` - the brain (platform-independent, tested in WSL)
| Module | Job |
|---|---|
| `lcu.rs` | lockfile discovery + authenticated HTTPS to the client: gameflow phase, champ select session, summoner, item sets, rune pages, summoner spells |
| `live.rs` | Live Client Data (`:2999`): `summarize()` -> me (gold, items, ability levels, KDA), allies, enemies |
| `champselect.rs` | session -> `Lobby` (my cell/champion/position/spells, ally + enemy champion ids, bans) |
| `aggregate.rs` | what players run on this patch, per champion + position, from op.gg's champion API: summoner spells, rune page, skill order, starters, core items, boots, late items, counters; cached 6 h under `%LOCALAPPDATA%\Featherstorm\aggregate`, stale cache used offline |
| `ddragon.rs` | Data Dragon catalog cached under `%LOCALAPPDATA%\Featherstorm\ddragon\<patch>`: items, champions, runes, by id and by normalised name |
| `pack.rs` | the data pack, embedded at compile time from `data/pack/` (`xayah.json`, `champion_traits.json`) |
| `placement.rs` | panel geometry: the default position (bottom-right, left of the minimap, above the taskbar), and whether a saved position is still usable (its header must be entirely on a monitor) |
| `engine.rs` | aggregate base + pack rules -> `Plan`: ordered path with tags/why, NEXT item with components and "buy now", skill point, rune page, spells, matchup line, source line |
| `itemset.rs` | `Plan` -> LCU item set (block titles capped at 30 chars) |
| `runes.rs` | pack rune page -> LCU perk page; summoner spell ids |
| `state.rs` | `PanelState`, the JSON the panel renders |

Run the tests from WSL: `cd overlay && cargo test -p featherstorm-core`.

## Rules (engine.rs), in the order they run
0. Base build: the aggregate's start, core items, boots, skill order, rune page and spells (any champion). The
   pack's non-damage slots (armor pen, defensive) fill the path to six, else the most popular finished items
   (never a component, an alternative first item, or a second armor-pen item). No aggregate: the pack's path.
1. Lane matchup from the pack (line, optional first item / start / spells, with a why line for a spell change).
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
- `main.rs`: window setup (380x300, transparent, always on top, no decorations), shared `App` state,
  logging to `%LOCALAPPDATA%\Featherstorm\featherstorm.log`. The position is remembered in `settings.json`
  but used only while the header is still on a monitor (else the bottom-right default, via `core::placement`);
  the panel always starts expanded, collapsing is not persisted.
- `poller.rs`: 1 s loop. Finds the client, follows the gameflow phase, polls champ select (1 s)
  or live data (2 s), runs the engine, publishes `PanelState` on the `state` event when it changed.
  Flashes the recommended skill for 3.5 s on level-up. Fetches the aggregate for (champion, assigned
  position) when that changes (retry every 30 s on failure) and auto-imports: rune page + summoner spells
  once per champion as soon as it is picked or hovered, the item set once locked and again when enemy
  locks change the path (`auto_*` settings).
- `commands.rs`: `do_import_*` (used by the poller's auto-import and by the buttons): rune page from the plan's
  ids (replaces its own `Featherstorm <champion> <position>` page), spells with Flash kept on the player's key,
  one item set per champion. Import results, gameflow changes and plan changes are logged at info level.
- `probe.rs`: `featherstorm.exe --probe` runs the pipeline once without a window and prints JSON
  (also saved to `%LOCALAPPDATA%\Featherstorm\probe.json`).
- `ui/`: plain HTML/CSS/JS, no bundler. `window.__TAURI__` (withGlobalTauri) for events and commands.

## Settings (`%LOCALAPPDATA%\Featherstorm\settings.json`)
`x`, `y` (written on drag), `auto_runes`, `auto_spells`, `auto_itemset` (default true), `region` (`global`, or
`na`, `euw`, `kr`, ...), `tier` (`emerald_plus`, `diamond_plus`, `all`, ...). Unknown keys are ignored.

## Building and running (from WSL)
```bash
scripts/cargo-win.sh build --release      # mirrors overlay/ + data/pack/ to C:\Users\<you>\code\featherstorm-win and builds there
scripts/overlay-probe.sh [debug|release]   # headless pipeline check, no window (Data Dragon, client, engine)
scripts/overlay-run.sh                    # launch the release exe on Windows (detached)
scripts/overlay-log.sh 40                 # tail %LOCALAPPDATA%\Featherstorm\featherstorm.log
scripts/win-screenshot.sh                 # full-DPI screenshot into .screens/ to eyeball the panel from WSL
scripts/win-rect.sh [process]             # where the window really is (physical pixels), e.g. after a placement change
scripts/overlay-stop.sh                   # kill it
scripts/capture.sh [start|stop|status]    # M0 watchers dumping raw champ-select / live payloads for fixtures while dogfooding
```

## Dogfooding checklist (M1)
Run `scripts/capture.sh start` first so the raw payloads land in `m0/tests/fixtures/captured/` (gitignored;
scrub the interesting ones into named fixtures afterwards).
1. Start the overlay with the client open: panel shows "In lobby" and the summoner name.
2. Practice Tool as Xayah: champ select shows the path, the source line (op.gg, games, patch), the
   Runes / Summoner spells / Item set buttons turning green on their own (auto-import at pick, item set at
   lock); clicking re-imports. Pick a non-Xayah champion: same, without matchup line or why lines.
3. In game: NEXT shows the first path item with components, "Buy now" flips to affordable
   components as gold comes in, bought components get a check, the path line checks off finished
   items, the skill key flashes on level-up.
4. Draft game: enemy locks change the path (Soraka -> Mortal Reminder with a `(Soraka)` tag and a why line).
5. Position survives a restart (settings.json) as long as it is still on a monitor; an off-screen saved
   position falls back to bottom-right; the panel always starts expanded; collapse shrinks to one line.
The binary lands in `C:\Users\<you>\code\featherstorm-win\overlay\target\release\featherstorm.exe`.
Requirements on Windows: Rust MSVC toolchain, Visual Studio Build Tools with the "Desktop development
with C++" workload (the MSVC linker), WebView2 (ships with Windows 11).

Why the mirror: cargo cannot build from a `\\wsl.localhost` path (lock files fail), and even
`cargo check --target x86_64-pc-windows-msvc` from WSL needs `lib.exe`, so the Windows side owns the
build. The WSL repo stays the source of truth.

## Panel state contract (what `ui/app.js` renders)
`phase` (noclient | idle | champselect | loading | ingame), `summoner`, `champion`, `supported`,
`lobby {allies, enemies, my_position}`, `plan` (see `engine::Plan`: `source`, `position`, `note`, `start`, `path`,
`options`, `next`, `skill`, `why`, `matchup`, `runes` (ids), `runes_summary`, `spells`, `spell_ids`), `live {game_time, gold, level, kda}`,
`flash {skill, until_ms}`, `imports {itemset, runes, spells}` (idle | working | done | error: ...),
`message`, `collapsed`, `ddragon`, `version`.
