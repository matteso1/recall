#!/usr/bin/env python3
"""Featherstorm doctor: can this machine reach the League client and the in-game API?

Run it first. It reports the environment (WSL/Windows, networking mode, transport),
finds the League install and lockfile, and probes the LCU, the Live Client Data API
and Data Dragon. Exit code 0 means the LCU pipe works.
"""
from __future__ import annotations

import argparse
import platform

import ddragon
from lcu import LCU, LCUError
from liveclient import LiveClient, heartbeat, summarize
from transport import ConnectionFailed, explain_backend, make_transport
from winenv import (ClientNotRunning, find_league_dir, is_wsl, lockfile_from_process,
                    lockfile_path, read_lockfile, wsl_networking_mode)

MIRRORED_HINT = ("WSL is in NAT mode, so requests go through Windows curl.exe. For direct access, "
                 "put '[wsl2]' + 'networkingMode=mirrored' in C:\\Users\\<you>\\.wslconfig and run "
                 "'wsl --shutdown' from Windows (the doctor then reports networking=mirrored).")


def row(key: str, value) -> None:
    print(f"  {key:<12} {value}")


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--no-ddragon", action="store_true", help="skip the Data Dragon check (no network)")
    args = ap.parse_args(argv)

    ok = True
    hints: list[str] = []
    print("Featherstorm doctor")
    env = f"Python {platform.python_version()} on {platform.system()}"
    if is_wsl():
        env += f" (WSL, networking={wsl_networking_mode()})"
    row("environment", env)
    row("transport", explain_backend())
    if is_wsl() and wsl_networking_mode() != "mirrored":
        hints.append(MIRRORED_HINT)
    try:
        transport = make_transport()
    except RuntimeError as e:
        row("transport", f"UNAVAILABLE: {e}")
        print("\n  verdict      cannot reach anything on the Windows side")
        return 1

    league = find_league_dir()
    row("League dir", league or "NOT FOUND (set FEATHERSTORM_LEAGUE_DIR)")

    lockfile = None
    try:
        lockfile = read_lockfile(league)
        row("lockfile", f"{lockfile_path(league)} -> {lockfile.masked()}")
    except ClientNotRunning as e:
        row("lockfile", f"missing ({e})")
        try:
            lockfile = lockfile_from_process()
            row("process", f"LeagueClientUx.exe on port {lockfile.port} (used instead of the lockfile)")
        except Exception as e2:  # noqa: BLE001 - report anything, this is a diagnostic
            row("process", f"LeagueClientUx.exe not found ({e2})")

    if lockfile is None:
        ok = False
        row("LCU", "skipped - start the League client and re-run")
    else:
        try:
            lcu = LCU(lockfile, transport=transport)
            phase = lcu.gameflow_phase()
            me = lcu.current_summoner()
            row("LCU", f"OK  phase={phase}  summoner={me.get('gameName')}#{me.get('tagLine')} "
                       f"(summonerId {me.get('summonerId')})")
        except ConnectionFailed as e:
            ok = False
            row("LCU", f"UNREACHABLE: {e}")
            hints.append("The lockfile exists but nothing answers on its port: stale lockfile from a "
                         "crashed client, or 127.0.0.1 here is not Windows (WSL NAT without curl.exe).")
        except LCUError as e:
            ok = False
            row("LCU", f"HTTP error: {e}")
        except RuntimeError as e:
            ok = False
            row("LCU", f"transport error: {e}")

    try:
        data = LiveClient(transport=transport).all_game_data()
        if data:
            row("Live Client", "OK  " + heartbeat(summarize(data)))
        else:
            row("Live Client", "not running (normal outside a game; Practice Tool works for testing)")
    except RuntimeError as e:
        row("Live Client", f"transport error: {e}")

    if not args.no_ddragon:
        try:
            items, champs, version = ddragon.load_indexes()
            row("Data Dragon", f"{version}  ({len(items.data)} items, {len(champs.by_key)} champions, "
                               f"cached in {ddragon.CACHE_DIR})")
        except Exception as e:  # noqa: BLE001
            row("Data Dragon", f"FAILED: {e}")
            hints.append("Data Dragon needs internet once per patch; after that the cache works offline.")

    print()
    row("verdict", "LCU pipe OK" if ok else "LCU pipe NOT working")
    if hints:
        print("hints:")
        for h in hints:
            print(f"  - {h}")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
