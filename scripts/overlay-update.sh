#!/usr/bin/env bash
# One command to put the current code into the overlay you play with. It waits while League is in
# queue, champion select or a game (the running overlay is never pulled out from under you), then
# stops the overlay, builds the canonical exe (overlay-build.sh), runs the headless probe on it and
# relaunches it (overlay-run.sh). The panel is gone for the minute or two of the build. If the build
# fails, the previous exe is relaunched unchanged.
# Usage: scripts/overlay-update.sh [jobs]      (default 6: nothing is being played meanwhile)
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
JOBS="${1:-6}"
phase() {
    python3 m0/updateguard.py 2>/dev/null || echo "Unknown"
}
wait_idle() {
    local p
    while :; do
        p="$(phase)"
        case "$p" in
            None|Lobby|EndOfGame|NoClient) break ;;
            *)
                echo "$(date +%H:%M:%S) League phase is $p; waiting before $1"
                sleep 20
                ;;
        esac
    done
    echo "$(date +%H:%M:%S) League is in $p; $1"
}
wait_idle updating
scripts/overlay-stop.sh || true
sleep 1
if ! scripts/overlay-build.sh "$JOBS"; then
    echo "build failed; relaunching the previous overlay" >&2
    wait_idle relaunching
    scripts/overlay-run.sh
    exit 1
fi
# The probe loads Data Dragon and plans offline without a window; its report lists the catalog.
if ! scripts/overlay-probe.sh | grep -q '"items"'; then
    echo "warning: the headless probe did not load the item catalog; see scripts/overlay-probe.sh" >&2
fi
wait_idle relaunching
scripts/overlay-run.sh
