# Dev setup: WSL + Windows (decided 2026-09-05)

## Decision
Hybrid. The repo, docs, Python prototyping, data pipeline and tests live in WSL. Only the
final overlay binary (Tauri, M1+) is built and run on Windows, because it is a Windows
program (WebView2, always-on-top transparent window) and League's local APIs are only on
Windows localhost.

## Observed machine facts
- Windows 11 build 26200; WSL 2.7.11, distro `Ubuntu-24.04`, user `nilsm`, **NAT** networking.
- League installed at `C:\Riot Games\League of Legends` (from `C:\ProgramData\Riot Games\RiotClientInstalls.json`).
- Windows side: Rust stable MSVC 1.97 (`C:\Program Files\Rust stable MSVC 1.97\bin`), built-in
  `curl.exe`, PowerShell. No Windows Python (Store stub only), no Windows Node.
- WSL side: Python 3.12.3, Node 24.19, git 2.43, gh 2.97 (logged in as matteso1). No Rust.

## The localhost problem and its two answers
The LCU (`https://127.0.0.1:<port>`, port from the lockfile) and the Live Client Data API
(`https://127.0.0.1:2999`) bind to Windows loopback only. From WSL in NAT mode a connection
to 127.0.0.1 hits the Linux VM and is refused (confirmed).

1. **curl.exe interop (works now, no restart).** WSL can launch Windows executables; a
   request issued by `C:\Windows\System32\curl.exe` originates on Windows and reaches the
   client. Measured ~50 ms per call. `m0/transport.py` uses this automatically in NAT mode.
2. **Mirrored networking (direct, cleaner).** `C:\Users\nilsm\.wslconfig`:
   ```
   [wsl2]
   networkingMode=mirrored
   ```
   Written and activated on 2026-09-05 (`wsl --shutdown` from a Windows terminal
   (this kills every WSL session, including Claude Code). Afterwards
   `wslinfo --networking-mode` prints `mirrored` and 127.0.0.1 is shared both ways.
   Revert by deleting the file. Known caveats: some VPN clients dislike mirrored mode.

## M1 build setup (settled 2026-09-05)
- Rust in WSL too (rustup, ~/.cargo) for the brain crate's tests. Cross-checking the Tauri crate
  from WSL fails (`cc-rs: failed to find tool "lib.exe"`), so the shell is Windows-only.
- Visual Studio Build Tools 2022 with the C++ workload provide `link.exe` (installed 2026-09-05:
  MSVC 14.44.35207, Windows SDK 10.0.26100); the standalone Rust MSI does not include them. Installed via `vs_BuildTools.exe --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended`
  (elevated through `Start-Process -Verb RunAs`; the UAC prompt must be clicked on the Windows side
  within 120 s or Windows cancels it - it took three tries).
- `scripts/cargo-win.sh` mirrors `overlay/`, `data/pack/` and the test fixtures to
  `C:\Users\nilsm\code\recall-win` with rsync and runs `cargo.exe` there.

## M1 build plan (Tauri) - original options, kept for the record
- Build on Windows with the existing Rust MSVC toolchain; `cargo install tauri-cli` there.
  WebView2 ships with Windows 11. Plain HTML/CSS/JS frontend means no Node is needed on
  Windows (WSL Node can prebuild assets if we ever want a bundler).
- Source location options: (a) keep the repo in WSL and run `cargo.exe` via interop on the
  `\\wsl.localhost\...` path (untested; UNC paths and MSVC link steps may misbehave, and
  builds over the 9P bridge are slow); (b) a Windows-side clone at
  `C:\Users\nilsm\code\recall`, synced through GitHub; (c) move the whole repo to
  `/mnt/c/...` so both sides see the same files (WSL git/python get slower).
  Default plan: (b).
- Overlay testing needs League in borderless/windowed mode (exclusive fullscreen hides
  every overlay).

## Testing tips
- Champ select without queueing: create a custom game lobby (Practice Tool or a
  custom with bots) - `lol-champ-select/v1/session` is live there too.
- Live Client Data: Practice Tool exposes the full API; buying items there is the
  fastest way to exercise the poller. Capture fixtures with `--dump`.
