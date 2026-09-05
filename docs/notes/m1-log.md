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
- Pending: on-screen test with the League client open (user was gaming), Practice Tool dogfood, draft game.
