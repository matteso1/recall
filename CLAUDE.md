# Recall - notes for Claude Code sessions

The project was called Featherstorm until 2026-09-06. The WSL checkout is still `~/code/featherstorm`;
everything else says Recall. `core/brand.rs` owns the name: rune pages and item sets are written as
`Recall <champion> <role>`, and the old `Featherstorm ...` ones are recognised as ours (reused, replaced,
never treated as personal pages). `%LOCALAPPDATA%\Featherstorm` is adopted as `%LOCALAPPDATA%\Recall`
on first start. The stop/run scripts also kill a leftover `featherstorm.exe`.

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
  Test the brain in WSL with `cargo test --manifest-path overlay/Cargo.toml --locked -p recall-core`
  (rustup lives in ~/.cargo); the headless shell tests are `cargo test --manifest-path tests/runtime/Cargo.toml
  --target-dir overlay/target --locked`; browser tests `cd tests/ui && npm test`; Python `python3 -m unittest
  discover -s m0/tests`. Clippy with `-D warnings` on core and tests/runtime is part of "passes".
- There is exactly one Windows executable: `scripts/overlay-build.sh` builds it (mirrors sources to
  `C:\Users\nilsm\code\recall-win`, BelowNormal priority, `--target-dir ...\overlay\target\swiftplay`) at
  `C:\Users\nilsm\code\recall-win\overlay\target\swiftplay\release\recall.exe`, and
  `scripts/overlay-run.sh` / `overlay-probe.sh` / `overlay-demo-shots.sh` use that same path. The build refuses
  to run while the exe is running (cargo cannot replace it). Never build from the WSL path.
- `scripts/overlay-probe.sh --champion Irelia --role jungle --swiftplay` plans an offline request on the real
  cache from the built exe, no client or game needed: use it to verify an artifact before handing it over.
- The Tauri crate cannot be type-checked from WSL (needs MSVC `lib.exe`), so keep logic in `core`.
- `scripts/overlay-probe.sh` runs the exe headless (`--probe`): use it before any on-screen test, and
  whenever the user may be gaming (see the shared-machine rule: no windows/League/screenshots then).
- Never hold a `std::sync::Mutex` guard across an `.await` (clone out, then await).
- The data pack (`data/pack/*.json`) is embedded with `include_str!`; a pack change needs a rebuild.
- Base builds (runes, spells, skill order, items) are not hand-tuned: `core/aggregate.rs` fetches op.gg's
  champion API per champion + position at champ select (cached 6 h under `%LOCALAPPDATA%\Recall\aggregate`).
  A real response is the fixture `m0/tests/fixtures/opgg_xayah_adc.json`. The pack is rules + offline fallback.
  Do not "fix" a recommendation by editing the pack's defaults; fix the rule or the source.
- A champion with no data for the assigned role (Irelia Jungle) gets the same champion's most-played role as an
  explicitly labelled fallback (`Aggregate.requested_position`, `Plan.source_position`); the assigned role keeps
  driving the role rules (Smite, jungle companion, support quest) and the label shows on the panel, in Swiftplay
  and in the item set. Never relabel one role's data as another, never invent a build for another champion.
- Swiftplay (queue 480, `gameMode` SWIFTPLAY) is played on map 11 but starts at level 3 with 1400 gold, has
  Doran's items disabled and Guardian's items enabled (patch 26.1, observed on 26.17). `shop::ShopContext.swiftplay`
  and `engine::GameMode` carry that; Data Dragon's per-map flags cannot. Its champ select lasts one second and
  enemies are unknown before the game. `docs/notes/swiftplay.md` has the research and the verified LCU contract.
- Panel look: `overlay/ui/` uses League's Hextech palette and Data Dragon icons (tokens at the top of
  `style.css`; notes in `docs/notes/m1-overlay.md`). Check UI changes with `scripts/overlay-demo-shots.sh`
  (`recall.exe --demo champselect|ingame`), which stages real data without a game. Not while gaming.

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
