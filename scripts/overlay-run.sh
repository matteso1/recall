#!/usr/bin/env bash
# Launch the canonical overlay build on Windows (detached), replacing a running instance so two
# overlays never fight over the client. The executable is the one scripts/overlay-build.sh makes:
#   %USERPROFILE%\code\featherstorm-win\overlay\target\swiftplay\release\featherstorm.exe
# Override with FEATHERSTORM_EXE=<windows path> only for a deliberate experiment.
# Usage: scripts/overlay-run.sh
set -euo pipefail
WINHOME="$(cmd.exe /c 'echo %USERPROFILE%' 2>/dev/null | tr -d '\r')"
EXE_WIN="${FEATHERSTORM_EXE:-$WINHOME\\code\\featherstorm-win\\overlay\\target\\swiftplay\\release\\featherstorm.exe}"
if [ ! -f "$(wslpath -u "$EXE_WIN")" ]; then
    echo "not built: $EXE_WIN (run scripts/overlay-build.sh)" >&2
    exit 1
fi
echo "launching $EXE_WIN"
if tasklist.exe 2>/dev/null | tr -d '\r' | grep -q "^featherstorm.exe"; then
    echo "stopping the running overlay first"
    taskkill.exe /IM featherstorm.exe /F >/dev/null 2>&1 || true
    sleep 1
fi
# Fully detach: a GUI exe that inherits our stdout keeps the caller's pipe open forever.
nohup cmd.exe /c start "" "$EXE_WIN" >/dev/null 2>&1 </dev/null &
sleep 2
echo "running: $(tasklist.exe 2>/dev/null | tr -d '\r' | grep -c featherstorm.exe) process(es)"
