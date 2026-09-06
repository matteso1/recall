"""HTTP transport to the two local Riot APIs (LCU and Live Client Data).

Both listen on Windows 127.0.0.1 with a self-signed certificate. Two backends:

* DirectTransport  - urllib, cert verification off. Works from Windows Python,
                     and from WSL when networkingMode=mirrored.
* WinCurlTransport - shells out to Windows' built-in curl.exe through WSL
                     interop, so the request originates on the Windows side.
                     Needed in WSL NAT mode, where 127.0.0.1 is the Linux VM.

Backend choice is automatic (see choose_backend) and can be forced with
RECALL_TRANSPORT=direct|curl.
"""
from __future__ import annotations

import base64
import json
import os
import shutil
import ssl
import subprocess
import urllib.error
import urllib.request
from dataclasses import dataclass
from typing import Any, Optional

from winenv import is_wsl, wsl_networking_mode


class ConnectionFailed(Exception):
    """Nothing is listening (client/game not running) or the host is unreachable."""


@dataclass
class Response:
    status: int
    body: bytes

    @property
    def ok(self) -> bool:
        return 200 <= self.status < 300

    def text(self) -> str:
        return self.body.decode("utf-8", errors="replace")

    def json(self) -> Any:
        if not self.body.strip():
            return None
        return json.loads(self.text())


class Transport:
    name = "base"

    def request(self, method: str, url: str, body: Any = None,
                auth: Optional[tuple[str, str]] = None, timeout: float = 5.0) -> Response:
        raise NotImplementedError


class DirectTransport(Transport):
    name = "direct"

    def __init__(self) -> None:
        self._ctx = ssl.create_default_context()
        self._ctx.check_hostname = False
        self._ctx.verify_mode = ssl.CERT_NONE

    def request(self, method, url, body=None, auth=None, timeout=5.0) -> Response:
        headers = {"Accept": "application/json"}
        data = None
        if body is not None:
            data = json.dumps(body).encode("utf-8")
            headers["Content-Type"] = "application/json"
        if auth:
            token = base64.b64encode(f"{auth[0]}:{auth[1]}".encode()).decode()
            headers["Authorization"] = f"Basic {token}"
        req = urllib.request.Request(url, data=data, method=method, headers=headers)
        try:
            with urllib.request.urlopen(req, timeout=timeout, context=self._ctx) as r:
                return Response(r.status, r.read())
        except urllib.error.HTTPError as e:
            return Response(e.code, e.read())
        except (urllib.error.URLError, OSError) as e:
            raise ConnectionFailed(f"{method} {url}: {e}") from e


# curl exit codes that mean "nothing answered", as opposed to a bad request.
_CURL_CONNECTION_CODES = {6, 7, 28, 35, 52, 55, 56}


def parse_curl_output(out: bytes) -> Response:
    """Split '<body>\\n<http_code>' produced by -w '\\n%{http_code}'."""
    body, _, code = out.rpartition(b"\n")
    try:
        status = int(code.strip() or 0)
    except ValueError:
        status = 0
    return Response(status, body)


class WinCurlTransport(Transport):
    name = "curl.exe"

    def __init__(self, exe: Optional[str] = None) -> None:
        self.exe = exe or shutil.which("curl.exe")
        if not self.exe:
            raise RuntimeError("curl.exe not found on PATH (is WSL interop enabled?)")

    def request(self, method, url, body=None, auth=None, timeout=5.0) -> Response:
        cmd = [
            self.exe, "-s", "-S", "-k",
            "--connect-timeout", "2", "-m", str(max(1, int(round(timeout)))),
            "-X", method, "-H", "Accept: application/json",
            "-o", "-", "-w", "\n%{http_code}",
        ]
        if auth:
            cmd += ["-u", f"{auth[0]}:{auth[1]}"]
        stdin = None
        if body is not None:
            cmd += ["-H", "Content-Type: application/json", "--data-binary", "@-"]
            stdin = json.dumps(body).encode("utf-8")
        cmd.append(url)
        try:
            p = subprocess.run(cmd, input=stdin, capture_output=True, timeout=timeout + 10)
        except subprocess.TimeoutExpired as e:
            raise ConnectionFailed(f"{method} {url}: curl.exe timed out") from e
        if p.returncode != 0:
            err = p.stderr.decode("utf-8", errors="replace").strip()
            if p.returncode in _CURL_CONNECTION_CODES:
                raise ConnectionFailed(f"{method} {url}: curl.exe exit {p.returncode}: {err}")
            raise RuntimeError(f"curl.exe exit {p.returncode}: {err}")
        return parse_curl_output(p.stdout)


def choose_backend(env: Optional[dict] = None) -> str:
    env = os.environ if env is None else env
    forced = env.get("RECALL_TRANSPORT")
    if forced in ("direct", "curl"):
        return forced
    if is_wsl() and wsl_networking_mode() != "mirrored":
        return "curl"
    return "direct"


def make_transport(kind: Optional[str] = None) -> Transport:
    kind = kind or choose_backend()
    return WinCurlTransport() if kind == "curl" else DirectTransport()


def explain_backend() -> str:
    kind = choose_backend()
    if kind == "curl":
        return ("curl.exe via WSL interop (WSL is in NAT mode, so 127.0.0.1 here is not "
                "Windows; requests are issued from the Windows side)")
    if is_wsl():
        return "direct (WSL mirrored networking shares 127.0.0.1 with Windows)"
    return "direct (running on Windows)"
