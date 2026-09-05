#!/usr/bin/env bash
# Headless pipeline check: runs featherstorm.exe --probe (no window) and prints its JSON report.
# Usage: scripts/overlay-probe.sh [debug|release]
set -euo pipefail
PROFILE="${1:-debug}"
WINHOME="$(cmd.exe /c 'echo %USERPROFILE%' 2>/dev/null | tr -d '\r')"
EXE="$(wslpath -u "$WINHOME")/code/featherstorm-win/overlay/target/$PROFILE/featherstorm.exe"
[ -f "$EXE" ] || { echo "not built: $EXE" >&2; exit 1; }
timeout 90 "$EXE" --probe 2>&1 | tr -d '\r'
