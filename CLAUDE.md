# Featherstorm - notes for Claude Code sessions

Smart build overlay for League of Legends. The design doc at `docs/design.md` is the
source of truth for scope; `docs/notes/` holds decisions and API findings. Update the
status table in `README.md` and `docs/notes/m0-log.md` when milestones move.

## Environment
- Repo lives in WSL (Ubuntu 24.04) on a Windows 11 machine. League, its local APIs and
  the Rust MSVC toolchain are on the Windows side; Python 3.12 and Node 24 are in WSL.
- WSL NAT mode: Windows `127.0.0.1` is unreachable from WSL directly. `m0/transport.py`
  falls back to Windows `curl.exe` via interop. Mirrored networking was configured in
  `C:\Users\nilsm\.wslconfig` and activated with `wsl --shutdown` on 2026-09-05; verify
  with `wslinfo --networking-mode` (expect `mirrored`, doctor then says transport direct).
  The scripts detect either mode.
- The League client (`LeagueClientUx.exe`) is often running while working; the LCU is
  reachable then, the Live Client API only during a game (Practice Tool works).

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
