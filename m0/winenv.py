"""Where are we running, and where is League?

WSL / Windows detection, League install discovery, and lockfile parsing.
Stdlib only so it runs under WSL Python or a bare Windows Python alike.
"""
from __future__ import annotations

import functools
import json
import os
import platform
import re
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Optional


class ClientNotRunning(Exception):
    """The League client is not running (no lockfile / no LeagueClientUx process)."""


# --------------------------------------------------------------------------- OS


def is_windows() -> bool:
    return platform.system() == "Windows"


@functools.lru_cache(maxsize=None)
def is_wsl() -> bool:
    if is_windows():
        return False
    try:
        return "microsoft" in Path("/proc/version").read_text().lower()
    except OSError:
        return False


@functools.lru_cache(maxsize=None)
def wsl_networking_mode() -> Optional[str]:
    """'nat' | 'mirrored' | None (not WSL).

    In NAT mode, 127.0.0.1 inside WSL is the Linux VM, not Windows, so the
    League client's local APIs are unreachable directly (see transport.py).
    """
    if not is_wsl():
        return None
    exe = shutil.which("wslinfo")
    if exe:
        try:
            out = subprocess.run(
                [exe, "--networking-mode"], capture_output=True, text=True, timeout=5
            ).stdout.strip().lower()
            if out:
                return out
        except (OSError, subprocess.SubprocessError):
            pass
    return "nat"  # wslinfo predates mirrored mode; without it, NAT is the only option


def windows_to_local_path(win_path: str) -> Path:
    """'C:/Riot Games/League of Legends/' -> a Path this Python can open."""
    p = win_path.replace("\\", "/")
    if is_windows():
        return Path(p)
    m = re.match(r"^([A-Za-z]):/(.*)$", p)
    if m:
        return Path(f"/mnt/{m.group(1).lower()}/{m.group(2)}")
    return Path(p)


def program_data_dir() -> Path:
    if is_windows():
        return Path(os.environ.get("ProgramData", r"C:\ProgramData"))
    return Path("/mnt/c/ProgramData")


# ---------------------------------------------------------------- League install


def parse_riot_client_installs(text: str) -> list[str]:
    """Windows paths of League installs listed in RiotClientInstalls.json."""
    try:
        data = json.loads(text)
    except ValueError:
        return []
    assoc = data.get("associated_client") or {}
    return [k for k in assoc if "league of legends" in k.lower()]


def find_league_dir() -> Optional[Path]:
    env = os.environ.get("FEATHERSTORM_LEAGUE_DIR")
    if env:
        return Path(env)
    installs = program_data_dir() / "Riot Games" / "RiotClientInstalls.json"
    if installs.is_file():
        for win_path in parse_riot_client_installs(installs.read_text(encoding="utf-8")):
            d = windows_to_local_path(win_path)
            if d.is_dir():
                return d
    fallback = windows_to_local_path("C:/Riot Games/League of Legends/")
    return fallback if fallback.is_dir() else None


# --------------------------------------------------------------------- lockfile


@dataclass(frozen=True)
class Lockfile:
    process: str
    pid: int
    port: int
    password: str
    protocol: str

    @property
    def auth(self) -> tuple[str, str]:
        return ("riot", self.password)

    def masked(self) -> str:
        return f"{self.process}:{self.pid}:{self.port}:{self.password[:2]}***:{self.protocol}"


def parse_lockfile(text: str) -> Lockfile:
    """Lockfile format: LeagueClient:<pid>:<port>:<password>:https"""
    parts = text.strip().split(":")
    if len(parts) != 5:
        raise ValueError(f"unexpected lockfile format: {text!r}")
    name, pid, port, password, proto = parts
    return Lockfile(name, int(pid), int(port), password, proto)


def lockfile_path(league_dir: Optional[Path] = None) -> Path:
    env = os.environ.get("FEATHERSTORM_LOCKFILE")
    if env:
        return Path(env)
    league_dir = league_dir or find_league_dir()
    if league_dir is None:
        raise ClientNotRunning(
            "could not locate the League install (set FEATHERSTORM_LEAGUE_DIR or FEATHERSTORM_LOCKFILE)"
        )
    return league_dir / "lockfile"


def read_lockfile(league_dir: Optional[Path] = None) -> Lockfile:
    path = lockfile_path(league_dir)
    if not path.is_file():
        raise ClientNotRunning(f"no lockfile at {path} - is the League client running?")
    return parse_lockfile(path.read_text(encoding="utf-8"))


# ------------------------------------------------- fallback: process command line

_PORT_RE = re.compile(r"--app-port=(\d+)")
_TOKEN_RE = re.compile(r"--remoting-auth-token=([^\s\"']+)")


def parse_ux_commandline(cmdline: str) -> Optional[tuple[int, str]]:
    port, token = _PORT_RE.search(cmdline or ""), _TOKEN_RE.search(cmdline or "")
    if port and token:
        return int(port.group(1)), token.group(1)
    return None


def lockfile_from_process(timeout: float = 20) -> Lockfile:
    """Ask Windows for LeagueClientUx.exe's command line (works from WSL via interop)."""
    ps = shutil.which("powershell.exe") or shutil.which("powershell") or shutil.which("pwsh")
    if not ps:
        raise ClientNotRunning("powershell not available for process lookup")
    script = (
        "Get-CimInstance Win32_Process -Filter \"Name='LeagueClientUx.exe'\" "
        "| Select-Object -ExpandProperty CommandLine"
    )
    out = subprocess.run(
        [ps, "-NoProfile", "-NonInteractive", "-Command", script],
        capture_output=True, text=True, timeout=timeout,
    ).stdout
    parsed = parse_ux_commandline(out)
    if not parsed:
        raise ClientNotRunning("LeagueClientUx.exe is not running")
    port, token = parsed
    return Lockfile("LeagueClientUx", 0, port, token, "https")


def describe_environment() -> dict:
    return {
        "python": platform.python_version(),
        "os": platform.system(),
        "wsl": is_wsl(),
        "wsl_networking_mode": wsl_networking_mode(),
        "league_dir": str(find_league_dir()),
    }
