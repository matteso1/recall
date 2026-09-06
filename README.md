# Featherstorm

A smart build overlay for League of Legends: one on-screen panel that says what to buy
next and why, adapted to the actual lobby (enemy comp, lane matchup, how the game is
going), with one-click import of runes, spells and the ordered item set into the client.

The full design is in [docs/design.md](docs/design.md). Working name, placeholder.

## Status

**M0 - prove the pipe: complete** (2026-09-05). Python, stdlib only, no UI. Every integration the
overlay needs was exercised against the real client and a real game.

| Piece | State |
|---|---|
| Find the client, read its lockfile, talk to the LCU API | done, verified against the live client from WSL |
| Champ select watcher (prints hovers, bans, lock-ins) | verified in a Practice Tool champ select (hover, lock, phase changes); real captures are test fixtures. Enemy picks/bans still need a draft game |
| Live Client Data poller (gold, items, level, abilities, enemy items) | verified in a Practice Tool game (gold ticks, skill point, item purchase); real capture is a test fixture |
| Push a hardcoded Xayah item set into the client | done, visible in the in-game shop; block titles kept to 30 chars because the shop panel truncates |
| Data Dragon name -> id resolution with local cache | done |

**M1 - Xayah overlay: in progress** (started 2026-09-05). Rust workspace in `overlay/`: a
platform-independent brain crate (`core`, 24 tests, runs in WSL) and a Tauri shell (`src-tauri`)
built on the Windows side. Builds clean; the headless probe passes and the panel renders on the target
machine (position remembered, validated against the monitors); the Practice Tool dogfood is next. See [docs/notes/m1-overlay.md](docs/notes/m1-overlay.md)
and [docs/notes/m1-log.md](docs/notes/m1-log.md).

## Setup (WSL + Windows)

League and its two local APIs live on Windows. Development happens in WSL. The two meet
like this:

- The LCU (client) and Live Client Data (in-game) APIs listen on **Windows** `127.0.0.1`.
  In WSL's default NAT mode that address is the Linux VM, so the scripts route requests
  through Windows' built-in `curl.exe` (about 50 ms per call). No setup needed.
- For direct access, enable WSL mirrored networking: `C:\Users\<you>\.wslconfig` with
  `[wsl2]` / `networkingMode=mirrored`, then `wsl --shutdown` from Windows. The scripts
  detect the mode and switch to direct HTTPS automatically.
- The client's lockfile is read from `C:\Riot Games\League of Legends\lockfile`
  (located via `C:\ProgramData\Riot Games\RiotClientInstalls.json`), no memory reading,
  no injection. See [docs/design.md](docs/design.md) section 8 for the Riot-compliance stance.

## Running M0

All scripts are stdlib-only Python 3.10+. From the repo root:

```bash
python3 m0/doctor.py                 # environment + connectivity report; run this first
python3 m0/watch_champselect.py      # prints champ select as champs lock in (--dump DIR saves raw JSON)
python3 m0/watch_live.py             # prints your gold/items/level every 2s once in a game
python3 m0/push_itemset.py           # pushes data/itemsets/xayah.json into the client (--remove to undo,
                                     #   --remove-title 'OP.GG Xayah' to drop another app's set)
python3 -m unittest discover -s m0/tests -v
```

Useful flags: `--once` (single poll), `--offline` (cached Data Dragon only),
`--dry-run` on push_itemset, `FEATHERSTORM_TRANSPORT=direct|curl` to force a transport,
`FEATHERSTORM_LEAGUE_DIR` / `FEATHERSTORM_LOCKFILE` for non-standard installs.

## Building the overlay

```bash
scripts/cargo-win.sh build --release   # Windows build via a mirrored copy; needs VS Build Tools (C++) on Windows
scripts/overlay-run.sh                 # launch it; overlay-log.sh / overlay-stop.sh / win-screenshot.sh / win-rect.sh alongside
cd overlay && cargo test -p featherstorm-core   # the brain's tests, in WSL
```

## Layout

```
docs/design.md          the design doc (source of truth for scope)
docs/notes/             dev setup decisions, API notes, M0 log
m0/                     "prove the pipe" scripts and library modules
  winenv.py             WSL/Windows detection, League install + lockfile discovery
  transport.py          HTTPS to the local APIs: direct urllib, or Windows curl.exe via interop
  lcu.py                LCU client (gameflow, summoner, champ select, item sets, runes)
  liveclient.py         Live Client Data client + snapshot/diff logic
  champselect.py        champ select session -> state, diffs, team summary
  ddragon.py            Data Dragon fetch/cache, item + champion name resolution
  itemsets.py           names-based spec -> LCU item set; upsert/remove
  tests/                unit tests with JSON fixtures
overlay/                M1 Tauri overlay: core/ (brain), src-tauri/ (window + poller + commands), ui/ (panel)
data/pack/              the data pack: xayah.json (build, runes, spells, matchups), champion_traits.json
data/itemsets/          M0 item-set spec by item *name* (superseded by data/pack for the overlay)
data/cache/             Data Dragon cache (gitignored)
scripts/cargo-win.sh    run cargo for the overlay on the Windows toolchain
```
