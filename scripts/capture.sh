#!/usr/bin/env bash
# Record real payloads for fixtures while dogfooding: runs the M0 champ-select and live-game watchers
# detached, dumping every changed raw JSON into m0/tests/fixtures/captured/ (gitignored: scrub into
# named fixtures afterwards). Tracks them by pidfile, not by pattern (a pattern would match the shell
# that runs this). Usage: scripts/capture.sh [start|stop|status]
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
DIR="m0/tests/fixtures/captured"
mkdir -p "$DIR"
WATCHERS="champselect live"

alive() { # alive NAME -> 0 if the pidfile names a running watcher of that kind
    local pid
    pid="$(cat "$DIR/watch_$1.pid" 2>/dev/null || true)"
    [ -n "$pid" ] && [ -r "/proc/$pid/cmdline" ] && tr '\0' ' ' <"/proc/$pid/cmdline" | grep -q "watch_$1.py"
}
launch() { # launch NAME: the bash wrapper records its pid, then becomes the watcher
    setsid nohup bash -c "echo \$\$ >'$DIR/watch_$1.pid'; exec python3 m0/watch_$1.py --dump '$DIR'" \
        >>"$DIR/watch_$1.log" 2>&1 </dev/null &
}
case "${1:-start}" in
    start)
        for w in $WATCHERS; do
            if alive "$w"; then echo "watch_$w already running (pid $(cat "$DIR/watch_$w.pid"))"; else launch "$w"; fi
        done
        sleep 1
        echo "capturing into $DIR (logs: $DIR/watch_*.log)"
        ;;
    stop)
        for w in $WATCHERS; do
            if alive "$w"; then kill "$(cat "$DIR/watch_$w.pid")" && echo "stopped watch_$w"; fi
            rm -f "$DIR/watch_$w.pid"
        done
        ;;
    status)
        for w in $WATCHERS; do
            if alive "$w"; then echo "watch_$w running (pid $(cat "$DIR/watch_$w.pid"))"; else echo "watch_$w not running"; fi
        done
        echo "--- captured files: $(find "$DIR" -name '*.json' | wc -l)"
        for f in "$DIR"/watch_*.log; do [ -f "$f" ] && { echo "--- $f"; tail -3 "$f"; }; done
        ;;
    *) echo "usage: scripts/capture.sh [start|stop|status]" >&2; exit 2 ;;
esac
