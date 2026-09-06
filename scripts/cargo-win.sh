#!/usr/bin/env bash
# Run cargo for the overlay on the Windows toolchain.
#
# The Rust MSVC toolchain and WebView2 live on Windows, and cargo cannot build from a
# \\wsl.localhost path, so this mirrors overlay/ and data/pack/ to the Windows drive and
# runs cargo.exe there (test fixtures come along for `cargo test`).
# Usage: scripts/cargo-win.sh build --release | test | run | check
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [ -z "${RECALL_WIN_MIRROR:-}" ]; then
    WINHOME="$(cmd.exe /c 'echo %USERPROFILE%' 2>/dev/null | tr -d '\r')"
    RECALL_WIN_MIRROR="$(wslpath -u "$WINHOME")/code/recall-win"
fi
mkdir -p "$RECALL_WIN_MIRROR/overlay" "$RECALL_WIN_MIRROR/data/pack" "$RECALL_WIN_MIRROR/m0/tests/fixtures"
rsync -a --delete --exclude target --exclude gen "$REPO/overlay/" "$RECALL_WIN_MIRROR/overlay/"
rsync -a --delete "$REPO/data/pack/" "$RECALL_WIN_MIRROR/data/pack/"
rsync -a --delete "$REPO/m0/tests/fixtures/" --exclude captured "$RECALL_WIN_MIRROR/m0/tests/fixtures/"
cd "$RECALL_WIN_MIRROR/overlay/src-tauri"
echo "[cargo-win] $(wslpath -w "$PWD")  cargo $*" >&2
if [ -n "${RECALL_NICE:-}" ]; then
    # Gentle mode for when the machine is in use: cargo and its rustc children at BelowNormal priority.
    ARGS=""; for a in "$@"; do ARGS="$ARGS'$a',"; done; ARGS="${ARGS%,}"
    exec powershell.exe -NoProfile -NonInteractive -Command "\$p = Start-Process -FilePath 'cargo.exe' -ArgumentList @($ARGS) -NoNewWindow -PassThru; \$p.PriorityClass = 'BelowNormal'; \$p.WaitForExit(); exit \$p.ExitCode"
fi
exec cargo.exe "$@"
