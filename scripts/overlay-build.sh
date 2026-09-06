#!/usr/bin/env bash
# The one canonical Windows build of the overlay. It always lands at
#   C:\Users\<you>\code\recall-win\overlay\target\swiftplay\release\recall.exe
# which is also what scripts/overlay-run.sh, overlay-probe.sh and overlay-demo-shots.sh use, so
# there is exactly one executable path to know. Refuses to build while that executable is
# running: cargo cannot replace a running exe, and a half-replaced binary is worse than an old one.
# Low priority (BelowNormal) and two jobs, so a game on the same machine keeps its frames.
# Usage: scripts/overlay-build.sh [jobs]          (default 2)
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
JOBS="${1:-2}"
if tasklist.exe 2>/dev/null | tr -d '\r' | grep -q "^recall.exe"; then
    echo "recall.exe is running; stop it first (scripts/overlay-stop.sh), the build cannot replace a running exe" >&2
    exit 1
fi
WINHOME="$(cmd.exe /c 'echo %USERPROFILE%' 2>/dev/null | tr -d '\r')"
TARGET_WIN="$WINHOME\\code\\recall-win\\overlay\\target\\swiftplay"
RECALL_NICE=1 scripts/cargo-win.sh build --locked --release -p recall --target-dir "$TARGET_WIN" -j "$JOBS"
EXE="$(wslpath -u "$TARGET_WIN")/release/recall.exe"
[ -f "$EXE" ] || { echo "build finished but $EXE is missing" >&2; exit 1; }
echo "built: $TARGET_WIN\\release\\recall.exe ($(stat -c %s "$EXE") bytes)"
echo "sha256: $(sha256sum "$EXE" | cut -c1-64)"
