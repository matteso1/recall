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
