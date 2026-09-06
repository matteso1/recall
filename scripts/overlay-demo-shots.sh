#!/usr/bin/env bash
# Design check without a game: run the release exe in --demo mode for each phase, crop a screenshot of
# just the panel into .screens/demo-<phase>.png, then kill it. Stops any running overlay first and does
# NOT restart it (run scripts/overlay-run.sh afterwards). Puts windows on screen: not while gaming.
# Usage: scripts/overlay-demo-shots.sh [phase ...]   (default: champselect ingame)
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
PHASES=("$@"); [ ${#PHASES[@]} -gt 0 ] || PHASES=(champselect ingame)
WINHOME="$(cmd.exe /c 'echo %USERPROFILE%' 2>/dev/null | tr -d '\r')"
EXE_WIN="${FEATHERSTORM_EXE:-$WINHOME\\code\\featherstorm-win\\overlay\\target\\swiftplay\\release\\featherstorm.exe}"
[ -f "$(wslpath -u "$EXE_WIN")" ] || { echo "not built: $EXE_WIN" >&2; exit 1; }
mkdir -p .screens
for phase in "${PHASES[@]}"; do
    taskkill.exe /IM featherstorm.exe /F >/dev/null 2>&1 || true
    sleep 1
    nohup cmd.exe /c start "" "$EXE_WIN" --demo "$phase" >/dev/null 2>&1 </dev/null &
    sleep "${DEMO_WAIT:-7}"
    RECT="$(scripts/win-rect.sh featherstorm | grep "'Featherstorm'" | head -1)"
    echo "$phase: $RECT"
    if [[ "$RECT" =~ at\ \((-?[0-9]+),(-?[0-9]+)\)\ size\ ([0-9]+)x([0-9]+) ]]; then
        X=$((BASH_REMATCH[1] - 12)); Y=$((BASH_REMATCH[2] - 12)); W=$((BASH_REMATCH[3] + 24)); H=$((BASH_REMATCH[4] + 24))
        scripts/win-screenshot.sh ".screens/demo-$phase.png" "$X,$Y,$W,$H" | tail -1
    else
        echo "no window rect for $phase" >&2
    fi
    taskkill.exe /IM featherstorm.exe /F >/dev/null 2>&1 || true
done
