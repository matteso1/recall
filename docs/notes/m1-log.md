# M1 log

## 2026-09-05
- 13:xx Rust workspace written: `core` (brain) + `src-tauri` (shell) + `ui/`. Core compiled first try
  in WSL, 16 tests green.
- VS Build Tools 2022 (C++ workload) installed after three UAC attempts (the prompt times out in 120 s).
- 14:19 First Windows debug build of the whole Tauri app: clean, 41 s, zero errors. 22 MB debug exe.
- 14:23 `--probe` headless mode added (no window; for checks while the screen is busy). Output on this
  machine with the client closed:
  ```
  ddragon 16.17.1 (868 items, 173 champions, 62 runes)
  client: no lockfile (client not running)
  engine vs Tristana/Soraka/Malphite/Ornn/Thresh:
    path: ER, Greaves, IE, Mortal (Soraka), Navori, GA
    next: Essence Reaver -> buy Sheen
    why: "Mortal Reminder over LDR: Soraka heals", "Malphite and Ornn both build armor: armor pen earlier"
    matchup: vs Tristana line
  ```
- 14:27 Release build (opt-level s, LTO, strip) at BelowNormal priority with -j 6 while the user was
  gaming: 3 m 47 s, 9.6 MB `featherstorm.exe`. Headless probe passes on the release exe as well.
- Pending: on-screen test with the League client open (user was gaming), Practice Tool dogfood, draft game.
- 17:53 First on-screen run (release exe): launched, connected to the client (`connected to client on
  port 51493 as matteso#NA1`), panel rendered as a collapsed 380x64 strip at the top-middle of the
  screen instead of expanded bottom-right. Cause: `%LOCALAPPDATA%\Featherstorm\settings.json` held
  `{"x":3803,"y":322,"collapsed":true}` from an earlier run, and the saved position is honoured
  as-is. Fix next: validate saved position against the monitor, do not start collapsed unless the
  user collapsed it this session (or drop persisting `collapsed`), delete the stale settings.json.
  Monitor: 5120x2160 physical at 150% (Windows reports 3413x1440 logical to non-DPI-aware code).
  Window exstyle 0x8040118 (TOPMOST + NOACTIVATE present).
- `scripts/overlay-run.sh` used to hang its caller: the detached exe inherited the stdout pipe.
  Fixed with full stdio redirection + nohup.
- `scripts/overlay-visual-test.sh` never completed because of that hang; re-test after the fix.
- Session moved to the terminal TUI at the user's request (handoff prompt given).
- 18:05 Startup fix. `%LOCALAPPDATA%\Featherstorm` turned out to be empty already (settings.json, log
  and the Data Dragon cache all gone), so there was nothing stale left to delete; the fix makes the
  stale case impossible anyway. New `core::placement` (pure geometry, 9 tests): the saved position is
  used only when the panel's header is entirely on some monitor, otherwise the bottom-right default,
  which now also stays above the taskbar (the taskbar is a topmost window too and covered the bottom
  54 px of the old default on the desktop). `collapsed` is no longer persisted: the panel always
  starts expanded. `Settings` is position-only; unknown keys in an old file are ignored.
  The main.rs `Moved` handler still saves drags, but a programmatic `set_position` at startup did not
  raise `Moved` (no settings.json after the first run), so the placement is saved explicitly.
- 18:13 Release rebuild 1 m 45 s. Probe passes (client `None`, matteso#NA1, Data Dragon 16.17.1 re-cached).
  On-screen: window at (3884,1692) 570x450 physical = exactly the computed default, expanded, header +
  `idle` pill + "Ready. Start a game." + "matteso#NA1 · patch 16.17.1". Verified with the new
  `scripts/win-rect.sh` (GetWindowRect in physical pixels) rather than eyeballing a 5120x2160 PNG;
  `scripts/win-screenshot.sh out.png x,y,w,h` now crops. The user dragged the panel to (4448,149)
  during the test and settings.json followed (`{"x":4448,"y":149}`), so drag persistence works.
- `scripts/capture.sh start|stop|status`: both M0 watchers detached with `--dump` into
  `m0/tests/fixtures/captured/`, tracked by pidfile (a `pgrep -f` pattern matched the very shell that
  launched it, the gotcha from the notes). Running since 18:13 so the Practice Tool / draft payloads
  are captured whenever the user plays.
- 18:21 Rebuilt (1 m 23 s; the running exe has to be stopped first or cargo cannot replace it) and
  verified both placement paths on the real machine with `win-rect.sh`: restart with the user's saved
  (4448,149) -> kept (`[saved position]`); planted `{"x":9000,"y":9000,"collapsed":true}` -> panel at
  (3884,1620) = default above the 72 px taskbar, expanded (`[default placement], saved was Some((9000,
  9000))`); restored the user's position afterwards. Log now prints the monitor and work area
  (`5120x2160 ... work area 5120x2088`). Step 1 + 2 of the handoff done; the overlay is running at the
  user's chosen spot with the client open, capture.sh is recording. Next: Practice Tool dogfood (needs
  the user at the keyboard), then a draft game for enemy-driven swaps.
