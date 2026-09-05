# M0 log

## 2026-09-05
- Repo created at `~/code/featherstorm` (WSL). Design doc v0.1 committed verbatim.
- Environment: WSL2 NAT mode; League client running on Windows. Direct 127.0.0.1 from WSL
  refused as expected; Windows `curl.exe` via interop reaches the LCU in ~50 ms.
- LCU verified live: gameflow phase `Lobby`, summoner matteso#NA1, one existing item set
  (`OP.GG Xayah`) read back. Dumped `/help` to confirm item-set and gameflow schemas.
- Data Dragon 16.17.1: all names in `data/itemsets/xayah.json` resolve. Notable: the
  string is `B. F. Sword`; Essence Reaver builds from Sheen + Caulfield's + Cloak; two
  entries are named "The Collector" (6676 is the shop one).
- Wrote `.wslconfig` with mirrored networking; not yet activated (needs `wsl --shutdown`).
- Pending real-game verification: champ select watcher and live poller (unit-tested on
  hand-written fixtures only).
- `push_itemset.py` run for real: `Featherstorm Xayah` (14 blocks) written next to `OP.GG Xayah`
  and read back from the client. `push_itemset.py --remove` undoes it.
- 30 unit tests green (`python3 -m unittest discover -s m0/tests`). Doctor: LCU pipe OK.
- Champ select fixture field names checked against `/help` types `ChampSelectSession`,
  `ChampSelectPlayerSelection`, `ChampSelectTimer`, `LolChampSelectLegacyChampSelectAction`.
- 12:35 `wsl.exe --shutdown` run to activate mirrored networking (at the user's request).
  Next session: confirm `wslinfo --networking-mode` = mirrored and re-run the doctor.
- 12:41 WSL back up in mirrored mode. Doctor: transport direct, LCU OK without curl.exe.
  Watchers restarted in the background (`--dump m0/tests/fixtures/captured`).
- 12:44-12:47 first Practice Tool game with both watchers running (direct transport):
  champ select captured hover -> lock -> FINALIZATION -> GAME_STARTING; live poller saw gold
  ticking, a Q point and a Biscuit purchase. Scrubbed copies are now fixtures
  (`*_practicetool*.json`, raw `--dump` output is gitignored: it carries a chat JWT).
- Shop feedback: block titles were cut off. Titles now <= 30 chars (`MAX_BLOCK_TITLE`), the
  per-item "why" stays in the spec for the overlay. Set re-pushed; check in the next game.
- op.gg desktop app autostarts from `HKCU\...\Run` (`electron.app.OP.GG`) and used 1.8 GB RAM
  across 9 processes during the game; Overwolf autostarts too. Neither is ours.
- User confirmed the re-pushed set renders correctly in the shop ("item sets look great").
  **M0 complete.**
- At the user's request the op.gg desktop app (2.5.5) and Overwolf (0.309) were uninstalled
  via their registered uninstallers (Overwolf's needed a UAC elevation through PowerShell
  `Start-Process -Verb RunAs`; WSL interop cannot launch elevated processes directly).
  Dangling `Run` entry and leftover AppData folders removed. Backup of the Run key was not
  taken (the entries pointed at now-deleted executables). The account's "OP.GG Xayah" item
  set is untouched; `push_itemset.py --remove-title 'OP.GG Xayah'` removes it if wanted.
