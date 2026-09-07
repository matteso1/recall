#!/usr/bin/env bash
# Launch the canonical overlay build on Windows (detached), replacing a running instance so two
# overlays never fight over the client. The executable is the one scripts/overlay-build.sh makes:
#   %USERPROFILE%\code\recall-win\overlay\target\swiftplay\release\recall.exe
# Override with RECALL_EXE=<windows path> only for a deliberate experiment.
# Usage: scripts/overlay-run.sh
set -euo pipefail
WINHOME="$(cmd.exe /c 'echo %USERPROFILE%' 2>/dev/null | tr -d '\r')"
EXE_WIN="${RECALL_EXE:-$WINHOME\\code\\recall-win\\overlay\\target\\swiftplay\\release\\recall.exe}"
if [ ! -f "$(wslpath -u "$EXE_WIN")" ]; then
    echo "not built: $EXE_WIN (run scripts/overlay-build.sh)" >&2
    exit 1
fi
echo "launching $EXE_WIN"
if tasklist.exe 2>/dev/null | tr -d '\r' | grep -qE "^(recall|featherstorm).exe"; then
    echo "stopping the running overlay first"
    taskkill.exe /IM recall.exe /F >/dev/null 2>&1 || true
    taskkill.exe /IM featherstorm.exe /F >/dev/null 2>&1 || true
    sleep 1
fi
# Fully detach: a GUI exe that inherits our stdout keeps the caller's pipe open forever.
# With auto-start installed the overlay hides itself while no client is running, like at logon.
ARGS=""
[ -f "$(wslpath -u "$WINHOME")/AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Startup/Recall.lnk" ] && ARGS="--autostart"
nohup cmd.exe /c start "" "$EXE_WIN" $ARGS >/dev/null 2>&1 </dev/null &
sleep 2
echo "running: $(tasklist.exe 2>/dev/null | tr -d '\r' | grep -c recall.exe) process(es)"
