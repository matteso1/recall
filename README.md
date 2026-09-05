# Featherstorm

A smart build overlay for League of Legends: one on-screen panel that says what to buy
next and why, adapted to the actual lobby (enemy comp, lane matchup, how the game is
going), with one-click import of runes, spells and the ordered item set into the client.

The full design is in [docs/design.md](docs/design.md). Working name, placeholder.

## Status

**M0 - prove the pipe** (started 2026-09-05). Python, stdlib only, no UI.

| Piece | State |
|---|---|
| Find the client, read its lockfile, talk to the LCU API | done, verified against the live client from WSL |
| Champ select watcher (prints hovers, bans, lock-ins) | verified in a Practice Tool champ select (hover, lock, phase changes); real captures are test fixtures. Enemy picks/bans still need a draft game |
| Live Client Data poller (gold, items, level, abilities, enemy items) | verified in a Practice Tool game (gold ticks, skill point, item purchase); real capture is a test fixture |
| Push a hardcoded Xayah item set into the client | done, visible in the in-game shop; block titles kept to 30 chars because the shop panel truncates |
| Data Dragon name -> id resolution with local cache | done |

Next: M1, the Tauri overlay (see [docs/notes/dev-setup.md](docs/notes/dev-setup.md) for the build plan).

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
python3 m0/push_itemset.py           # pushes data/itemsets/xayah.json into the client (--remove to undo)
python3 -m unittest discover -s m0/tests -v
```

Useful flags: `--once` (single poll), `--offline` (cached Data Dragon only),
`--dry-run` on push_itemset, `FEATHERSTORM_TRANSPORT=direct|curl` to force a transport,
`FEATHERSTORM_LEAGUE_DIR` / `FEATHERSTORM_LOCKFILE` for non-standard installs.

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
data/itemsets/          build specs by item *name* (ids resolved per patch)
data/cache/             Data Dragon cache (gitignored)
```
