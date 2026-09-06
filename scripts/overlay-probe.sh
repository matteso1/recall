#!/usr/bin/env bash
# Headless pipeline check: runs the canonical recall.exe with --probe (no window) and prints
# its JSON report. The release exe has no console, so the report is read back from
# %LOCALAPPDATA%\Recall\probe.json. Extra arguments go to the probe, e.g.
#   scripts/overlay-probe.sh --champion Irelia --role jungle --swiftplay
# plans an offline request against the real cache without a client or a game.
# Usage: scripts/overlay-probe.sh [probe args...]
set -euo pipefail
WINHOME="$(cmd.exe /c 'echo %USERPROFILE%' 2>/dev/null | tr -d '\r')"
EXE="${RECALL_EXE_UNIX:-$(wslpath -u "$WINHOME")/code/recall-win/overlay/target/swiftplay/release/recall.exe}"
[ -f "$EXE" ] || { echo "not built: $EXE (run scripts/overlay-build.sh)" >&2; exit 1; }
OUT="$(timeout 90 "$EXE" --probe "$@" 2>&1 | tr -d '\r' || true)"
if [ -n "$OUT" ]; then
    printf '%s\n' "$OUT"
else
    powershell.exe -NoProfile -NonInteractive -Command "Get-Content \"\$env:LOCALAPPDATA\Recall\probe.json\"" 2>&1 | tr -d '\r'
fi
