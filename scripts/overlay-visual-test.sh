#!/usr/bin/env bash
# On-screen test: opens the League client if needed, launches the overlay, screenshots the
# "waiting" and "connected" states into .screens/, tails the log. ONLY run when the user is not
# gaming - it puts windows on their screen. Usage: scripts/overlay-visual-test.sh [debug|release]
set -euo pipefail
PROFILE="${1:-release}"
cd "$(dirname "${BASH_SOURCE[0]}")/.."
if ! tasklist.exe 2>/dev/null | tr -d '\r' | grep -q LeagueClientUx.exe; then
    echo "starting the League client..."
    (cmd.exe /c start "" "C:\Riot Games\Riot Client\RiotClientServices.exe" --launch-product=league_of_legends --launch-patchline=live >/dev/null 2>&1 &)
fi
scripts/overlay-run.sh "$PROFILE"
sleep 6
scripts/win-screenshot.sh .screens/overlay-1-start.png
for _ in $(seq 1 18); do
    tasklist.exe 2>/dev/null | tr -d '\r' | grep -q LeagueClientUx.exe && break
    sleep 5
done
sleep 25
scripts/win-screenshot.sh .screens/overlay-2-connected.png
echo "=== log ==="
scripts/overlay-log.sh 25
echo "overlay running: $(tasklist.exe 2>/dev/null | tr -d '\r' | grep -c recall.exe)"
