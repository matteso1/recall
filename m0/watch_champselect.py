#!/usr/bin/env python3
"""Print champ select as it happens: your role, hovers, bans, and each lock-in.

Polls the LCU gameflow phase; while in ChampSelect it polls the session and prints
only what changed. Use --dump DIR to save every changed raw session as JSON
(that is how we capture real fixtures for the tests).
"""
from __future__ import annotations

import argparse
import json
import time
from datetime import datetime
from pathlib import Path

import champselect
import ddragon
from lcu import LCU, LCUError
from transport import ConnectionFailed
from winenv import ClientNotRunning


def say(msg: str) -> None:
    print(f"{datetime.now():%H:%M:%S}  {msg}", flush=True)


def connect_blocking(poll: float = 5.0) -> LCU:
    warned = False
    while True:
        try:
            lcu = LCU.connect()
            lcu.gameflow_phase()
            return lcu
        except (ClientNotRunning, ConnectionFailed) as e:
            if not warned:
                say(f"waiting for the League client... ({e})")
                warned = True
            time.sleep(poll)


def load_champions(offline: bool):
    try:
        _, champs, version = ddragon.load_indexes(offline=offline)
        say(f"Data Dragon {version} loaded ({len(champs.by_key)} champions)")
        return champs
    except Exception as e:  # noqa: BLE001
        say(f"no Data Dragon data ({e}); champion ids will be shown instead of names")
        return None


def dump(directory: Path, session: dict) -> Path:
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / f"champselect_{datetime.now():%Y%m%d_%H%M%S_%f}.json"
    path.write_text(json.dumps(session, indent=1), encoding="utf-8")
    return path


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--interval", type=float, default=1.0, help="seconds between polls (default 1)")
    ap.add_argument("--once", action="store_true", help="print the current phase/state and exit")
    ap.add_argument("--dump", type=Path, help="directory to save raw session JSON on every change")
    ap.add_argument("--offline", action="store_true", help="use cached Data Dragon data only")
    args = ap.parse_args(argv)

    champs = load_champions(args.offline)
    lcu = connect_blocking()
    say(f"connected to the client on port {lcu.lockfile.port} via {lcu.transport.name}")

    prev_phase = None
    prev_state = None
    while True:
        try:
            phase = lcu.gameflow_phase()
            if phase != prev_phase:
                say(f"[GAMEFLOW] {prev_phase or '-'} -> {phase}")
                prev_phase = phase
            if phase == "ChampSelect":
                session = lcu.champ_select_session()
                if session is not None:
                    state = champselect.extract(session)
                    lines = champselect.diff(prev_state, state, champs)
                    for line in lines:
                        say(line)
                    if lines and args.dump:
                        say(f"saved {dump(args.dump, session)}")
                    prev_state = state
            elif prev_state is not None:
                say("champ select over; final teams:")
                for line in champselect.summary(prev_state, champs):
                    say(line)
                prev_state = None
            if args.once:
                if phase != "ChampSelect":
                    say("not in champ select right now")
                return 0
        except ConnectionFailed:
            say("lost the client; waiting for it to come back...")
            prev_phase = None
            lcu = connect_blocking()
        except LCUError as e:
            say(f"LCU error: {e}")
        time.sleep(args.interval)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        pass
