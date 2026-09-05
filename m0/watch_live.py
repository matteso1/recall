#!/usr/bin/env python3
"""Poll the Live Client Data API every ~2s: your gold/items/level/abilities and everyone's visible items.

Only prints changes (plus a periodic heartbeat). Use --dump DIR to save the raw
allgamedata JSON whenever something changed, for test fixtures.
"""
from __future__ import annotations

import argparse
import json
import time
from datetime import datetime
from pathlib import Path

from liveclient import LiveClient, diff, fmt_time, heartbeat, summarize
from transport import make_transport


def say(msg: str) -> None:
    print(f"{datetime.now():%H:%M:%S}  {msg}", flush=True)


def dump(directory: Path, data: dict) -> Path:
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / f"allgamedata_{datetime.now():%Y%m%d_%H%M%S_%f}.json"
    path.write_text(json.dumps(data, indent=1), encoding="utf-8")
    return path


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--interval", type=float, default=2.0, help="seconds between polls (default 2)")
    ap.add_argument("--heartbeat", type=float, default=10.0,
                    help="seconds between status lines when nothing changed (0 = off)")
    ap.add_argument("--allies", action="store_true", help="also report ally item changes")
    ap.add_argument("--once", action="store_true", help="print one snapshot and exit (1 if no game)")
    ap.add_argument("--dump", type=Path, help="directory to save raw allgamedata JSON on every change")
    args = ap.parse_args(argv)

    live = LiveClient(transport=make_transport())
    say(f"polling {live.base} via {live.transport.name}")
    prev = None
    last_beat = 0.0
    last_wait_msg = -1e9
    while True:
        data = live.all_game_data()
        now = time.monotonic()
        if data is None:
            if prev is not None:
                say("game over (API went away)")
                prev = None
            if now - last_wait_msg > 30:
                say("waiting for a game... (Practice Tool counts)")
                last_wait_msg = now
            if args.once:
                return 1
        else:
            snap = summarize(data)
            lines = diff(prev, snap, include_allies=args.allies)
            if lines:
                for line in lines:
                    say(f"[{fmt_time(snap.game_time)}] {line}")
                if args.dump:
                    say(f"saved {dump(args.dump, data)}")
                last_beat = now
            elif args.heartbeat and now - last_beat >= args.heartbeat:
                say(heartbeat(snap))
                last_beat = now
            prev = snap
            if args.once:
                return 0
        time.sleep(args.interval)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        pass
