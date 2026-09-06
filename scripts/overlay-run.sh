#!/usr/bin/env bash
# Launch the built overlay on Windows (detached). Usage: scripts/overlay-run.sh [debug|release]
set -euo pipefail
PROFILE="${1:-release}"
WINHOME="$(cmd.exe /c 'echo %USERPROFILE%' 2>/dev/null | tr -d '\r')"
EXE_WIN="$WINHOME\\code\\featherstorm-win\\overlay\\target\\$PROFILE\\featherstorm.exe"
if [ ! -f "$(wslpath -u "$EXE_WIN")" ]; then
    echo "not built: $EXE_WIN (run scripts/cargo-win.sh build --release)" >&2
    exit 1
fi
taskkill.exe /IM featherstorm.exe /F >/dev/null 2>&1 || true
# Fully detach: a GUI exe that inherits our stdout keeps the caller's pipe open forever.
nohup cmd.exe /c start "" "$EXE_WIN" >/dev/null 2>&1 </dev/null &
sleep 2
echo "running: $(tasklist.exe 2>/dev/null | tr -d '\r' | grep -c featherstorm.exe) process(es)"
