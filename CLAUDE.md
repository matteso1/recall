# Featherstorm - notes for Claude Code sessions

Smart build overlay for League of Legends. The design doc at `docs/design.md` is the
source of truth for scope; `docs/notes/` holds decisions and API findings. Update the
status table in `README.md` and `docs/notes/m0-log.md` when milestones move.

## Environment
- Repo lives in WSL (Ubuntu 24.04) on a Windows 11 machine. League, its local APIs and
  the Rust MSVC toolchain are on the Windows side; Python 3.12 and Node 24 are in WSL.
- WSL runs in mirrored networking mode (`C:\Users\nilsm\.wslconfig`, active since 2026-09-05),
  so Windows `127.0.0.1` is reachable directly and the doctor reports transport `direct`.
  In NAT mode `m0/transport.py` falls back to Windows `curl.exe` via interop; the scripts
  detect either mode (`wslinfo --networking-mode`).
- The League client (`LeagueClientUx.exe`) is often running while working; the LCU is
  reachable then, the Live Client API only during a game (Practice Tool works).

## Rust overlay (M1+)
- `overlay/` is a cargo workspace: `core` (brain, platform-independent) and `src-tauri` (Windows shell).
  Test the brain in WSL with `cd overlay && cargo test -p featherstorm-core` (rustup lives in ~/.cargo).
  Build/run the shell on Windows with `scripts/cargo-win.sh build --release` / `scripts/cargo-win.sh run`;
  it mirrors the sources to `C:\Users\nilsm\code\featherstorm-win`. Never build from the WSL path.
- The Tauri crate cannot be type-checked from WSL (needs MSVC `lib.exe`), so keep logic in `core`.
- `scripts/overlay-probe.sh` runs the exe headless (`--probe`): use it before any on-screen test, and
  whenever the user may be gaming (see the shared-machine rule: no windows/League/screenshots then).
- Never hold a `std::sync::Mutex` guard across an `.await` (clone out, then await).
- The data pack (`data/pack/*.json`) is embedded with `include_str!`; a pack change needs a rebuild.
- Base builds (runes, spells, skill order, items) are not hand-tuned: `core/aggregate.rs` fetches op.gg's
  champion API per champion + position at champ select (cached 6 h under `%LOCALAPPDATA%\Featherstorm\aggregate`).
  A real response is the fixture `m0/tests/fixtures/opgg_xayah_adc.json`. The pack is rules + offline fallback.
  Do not "fix" a recommendation by editing the pack's defaults; fix the rule or the source.
- Panel look: `overlay/ui/` uses League's Hextech palette and Data Dragon icons (tokens at the top of
  `style.css`; notes in `docs/notes/m1-overlay.md`). Check UI changes with `scripts/overlay-demo-shots.sh`
  (`featherstorm.exe --demo champselect|ingame`), which stages real data without a game. Not while gaming.

## Conventions
- `m0/` is stdlib-only Python so it also runs under a bare Windows Python. Scripts import
  sibling modules directly; run them as `python3 m0/<script>.py` from the repo root.
- Tests: `python3 -m unittest discover -s m0/tests -v`. Fixtures in `m0/tests/fixtures/`.
  Capture real payloads with `watch_champselect.py --dump DIR` / `watch_live.py --dump DIR`
  and prefer them over hand-written fixtures.
- Data files name items/champions by Data Dragon *name*; never hardcode numeric ids.
- Riot compliance (design doc section 8): only the LCU and Live Client Data APIs, only
  information visible to the player, recommend-don't-dictate wording. No memory reading,
  no packet capture, no enemy cooldown/ult timers.
- Never print the lockfile password; use `Lockfile.masked()`.
- Item-set pushes modify the user's account item sets. `push_itemset.py --remove` undoes them.
