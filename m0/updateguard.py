"""Read-only update guard: unknown client/process state must never authorize a restart."""
import json
import shutil
import subprocess

from lcu import LCU


IDLE = {"None", "Lobby", "EndOfGame"}
BUSY = {"Matchmaking", "ReadyCheck", "ChampSelect", "GameStart", "InProgress",
        "Reconnect", "WaitingForStats", "PreEndOfGame"}


def league_processes():
    """Return relevant Windows process names, or None if absence cannot be verified."""
    powershell = shutil.which("powershell.exe") or shutil.which("powershell")
    if not powershell:
        return None
    # Query names only. Process command lines can contain LCU authentication tokens.
    script = (
        "$ErrorActionPreference = 'Stop'; "
        "ConvertTo-Json -Compress -InputObject @(Get-Process | "
        "Where-Object { $_.ProcessName -in @('LeagueClient', 'LeagueClientUx', 'League of Legends') } | "
        "Select-Object -ExpandProperty ProcessName)"
    )
    try:
        result = subprocess.run([powershell, "-NoProfile", "-NonInteractive", "-Command", script],
                                capture_output=True, text=True, timeout=10)
        if result.returncode != 0:
            return None
        names = json.loads(result.stdout)
        if not isinstance(names, list) or any(not isinstance(name, str) for name in names):
            return None
        return set(names)
    except (OSError, subprocess.TimeoutExpired, ValueError):
        return None


def phase(read_phase=None, read_processes=None):
    read_phase = read_phase or (lambda: LCU.connect().gameflow_phase())
    read_processes = read_processes or league_processes
    try:
        current = read_phase()
    except Exception:
        current = None
    if isinstance(current, str) and current in BUSY:
        return current
    try:
        processes = read_processes()
    except Exception:
        processes = None
    if processes is None:
        return "Unknown"
    if "League of Legends" in processes:
        return "InProgress"
    if isinstance(current, str) and current in IDLE:
        return current
    return "Unknown" if processes else "NoClient"


if __name__ == "__main__":
    print(phase())
