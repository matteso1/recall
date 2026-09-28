#!/usr/bin/env python3
"""Resume a bounded, patch-filtered pilot of paired Riot Match-v5 responses.

Raw files contain player identifiers and must stay outside Git. Credentials come
from RIOT_API_KEY or --key-file, never a command-line key or logged response body.
"""
import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import re
import sys
import tempfile
import time
from urllib.error import HTTPError, URLError
from urllib.parse import parse_qs, urlsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener


REGIONS = ("americas", "europe", "asia", "sea")
PLATFORMS = {
    "na1": "americas", "br1": "americas", "la1": "americas", "la2": "americas",
    "euw1": "europe", "eun1": "europe", "tr1": "europe", "ru": "europe", "me1": "europe",
    "kr": "asia", "jp1": "asia", "oc1": "sea", "sg2": "sea", "tw2": "sea", "vn2": "sea",
}
MATCH_ID = re.compile(r"[A-Z][A-Z0-9]{1,7}_[0-9]{1,20}\Z")
PATCH = re.compile(r"[0-9]{1,3}\.[0-9]{1,3}\Z")


class ApiError(RuntimeError):
    def __init__(self, message, status=None):
        super().__init__(message)
        self.status = status


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        # The credential is only for the explicitly selected Riot API host.
        return None


class RiotClient:
    def __init__(self, key, region, *, opener=None, clock=time.monotonic, sleep=time.sleep):
        if region not in REGIONS and region not in PLATFORMS:
            raise ValueError("Unsupported Riot routing region")
        if not key or "\n" in key or "\r" in key:
            raise ValueError("A nonempty single-line Riot API key is required")
        self._key = key
        self._platform = region in PLATFORMS
        self._host = f"https://{region}.api.riotgames.com"
        self._open = opener or build_opener(NoRedirect()).open
        self._clock, self._sleep = clock, sleep
        self._next_request = 0.0

    def get(self, path):
        parts = urlsplit(path)
        query = parse_qs(parts.query, keep_blank_values=True)
        if self._platform:
            allowed = re.fullmatch(r"/lol/league/v4/(?:master|grandmaster|challenger)leagues/by-queue/RANKED_SOLO_5x5", parts.path)
            allowed = allowed and not query
        elif re.fullmatch(r"/lol/match/v5/matches/by-puuid/[A-Za-z0-9_-]{1,256}/ids", parts.path):
            allowed = set(query) <= {"queue", "count", "startTime", "start"} and all(
                len(values) == 1 and values[0].isdigit() for values in query.values())
        else:
            allowed = re.fullmatch(r"/lol/match/v5/matches/[A-Z][A-Z0-9]{1,7}_[0-9]{1,20}(?:/timeline)?", parts.path)
            allowed = allowed and not query
        if not allowed or parts.netloc or parts.scheme or parts.fragment:
            raise ValueError("Unsupported Riot API request path")
        for attempt in range(5):
            delay = self._next_request - self._clock()
            if delay > 0:
                self._sleep(delay)
            # 1.25 seconds gives headroom under 100 requests / two minutes.
            self._next_request = self._clock() + 1.25
            request = Request(self._host + path, headers={
                "X-Riot-Token": self._key, "Accept": "application/json",
                "User-Agent": "Recall-data-pilot/1",
            })
            try:
                with self._open(request, timeout=30) as response:
                    try:
                        return json.load(response)
                    except (ValueError, UnicodeError):
                        raise ApiError("Riot returned malformed JSON") from None
            except HTTPError as error:
                status = error.code
                retry_after = error.headers.get("Retry-After") if error.headers else None
                error.close()
                if status in (401, 403):
                    raise ApiError("Riot authentication failed; check or renew the local key", status) from None
                if status not in (429, 500, 502, 503, 504) or attempt == 4:
                    raise ApiError(f"Riot request failed with HTTP {status}; cached progress is preserved", status) from None
                try:
                    wait = float(retry_after)
                    if not math.isfinite(wait) or wait < 0:
                        raise ValueError
                except (TypeError, ValueError):
                    # A rate-limited response without a usable header gets a full window.
                    wait = 120.0 if status == 429 else 2.0 ** attempt
                self._next_request = max(self._next_request, self._clock() + wait)
            except (URLError, TimeoutError, OSError):
                if attempt == 4:
                    raise ApiError("Riot network request failed; cached progress is preserved") from None
                self._next_request = max(self._next_request, self._clock() + 2.0 ** attempt)
        raise ApiError("Riot retry budget exhausted")


def validate_match(match, expected_id):
    if not isinstance(match, dict) or match.get("metadata", {}).get("matchId") != expected_id:
        raise ValueError("Match response has a mismatched match ID")
    players = match.get("info", {}).get("participants", [])
    if len(players) != 10 or {p.get("participantId") for p in players} != set(range(1, 11)):
        raise ValueError("Match must contain ten distinct participant slots")
    identities = [p.get("puuid") for p in players]
    if any(not isinstance(p, str) or not p for p in identities) or len(set(identities)) != 10:
        raise ValueError("Match is missing stable participant identities")
    metadata = match["metadata"].get("participants", [])
    if len(metadata) != 10 or set(metadata) != set(identities):
        raise ValueError("Match participant identities disagree with metadata")
    if Counter(p.get("teamId") for p in players) != {100: 5, 200: 5}:
        raise ValueError("Match must contain five participants per team")
    return {p["participantId"]: p["puuid"] for p in players}


def validate_pair(match, timeline, expected_id):
    identities = validate_match(match, expected_id)
    if not isinstance(timeline, dict) or timeline.get("metadata", {}).get("matchId") != expected_id:
        raise ValueError("Timeline response has a mismatched match ID")
    metadata = timeline["metadata"].get("participants", [])
    if len(metadata) != 10 or set(metadata) != set(identities.values()):
        raise ValueError("Timeline participant identities disagree with match metadata")
    info = timeline.get("info", {})
    participants = info.get("participants", [])
    if len(participants) != 10 or {p.get("participantId"): p.get("puuid") for p in participants} != identities:
        raise ValueError("Timeline participant mapping disagrees with the match")
    interval = info.get("frameInterval")
    if not isinstance(interval, int) or interval <= 0 or interval > 60000:
        raise ValueError("Timeline must provide at least minute-resolution frames")
    frames = info.get("frames", [])
    if len(frames) < 2:
        raise ValueError("Timeline has too few frames")
    if frames[0].get("timestamp") != 0:
        raise ValueError("Timeline is missing its initial frame")
    previous = -1
    events = Counter()
    for frame in frames:
        timestamp = frame.get("timestamp")
        if not isinstance(timestamp, int) or timestamp <= previous:
            raise ValueError("Timeline frames must be strictly chronological")
        if previous >= 0 and timestamp - previous > interval + 1000:
            raise ValueError("Timeline has a gap between frames")
        previous = timestamp
        players = frame.get("participantFrames", {})
        if set(players) != {str(i) for i in identities}:
            raise ValueError("Timeline frame does not contain all ten participants")
        for slot, player in players.items():
            if player.get("participantId") != int(slot):
                raise ValueError("Timeline frame participant slot disagrees with its key")
            for field in ("currentGold", "totalGold", "level", "xp", "minionsKilled", "jungleMinionsKilled"):
                if not isinstance(player.get(field), (int, float)) or not math.isfinite(player[field]):
                    raise ValueError(f"Timeline frame is missing numeric {field}")
        for event in frame.get("events", []):
            if not isinstance(event.get("type"), str) or not isinstance(event.get("timestamp"), int):
                raise ValueError("Timeline contains a malformed event")
            events[event["type"]] += 1
    if not any(e.get("type") == "GAME_END" for e in frames[-1].get("events", [])):
        raise ValueError("Timeline is truncated: final frame has no GAME_END event")
    return {"participants": 10, "frames": len(frames), "last_timestamp": previous,
            "event_counts": dict(sorted(events.items()))}


def write_text(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, prefix=".partial-", delete=False) as target:
            temporary = Path(target.name)
            target.write(value)
            target.flush()
            os.fsync(target.fileno())
        temporary.replace(path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def write_json(path, value):
    write_text(path, json.dumps(value, separators=(",", ":"), allow_nan=False) + "\n")


def private_output(path):
    resolved = path.expanduser().resolve()
    if any((parent / ".git").exists() for parent in (resolved, *resolved.parents)):
        raise ValueError("Raw player data must be saved outside Git checkouts")
    return resolved


def load_key(path):
    key = os.environ.get("RIOT_API_KEY", "").strip()
    if not key and path.is_file():
        key = path.read_text().strip()
    if not key:
        raise ValueError("Riot API key missing: save it in the local --key-file or RIOT_API_KEY")
    return key


def read_cached(path):
    try:
        return json.loads(path.read_text())
    except (ValueError, UnicodeError):
        raise ValueError("Cached response is malformed; inspect it before resuming") from None


def collect_pair(client, root, match_id, patches):
    if not MATCH_ID.fullmatch(match_id):
        raise ValueError("Invalid match ID in seed file")
    folder = root / match_id
    match_path, timeline_path = folder / "match.json", folder / "timeline.json"
    endpoint = f"/lol/match/v5/matches/{match_id}"
    match = read_cached(match_path) if match_path.exists() else client.get(endpoint)
    validate_match(match, match_id)
    if not match_path.exists():
        write_json(match_path, match)
    patch = ".".join(str(match["info"].get("gameVersion", "")).split(".")[:2])
    if match["info"].get("queueId") != 420 or patch not in patches:
        return {"status": "excluded", "patch": patch}
    cached = timeline_path.exists()
    timeline = read_cached(timeline_path) if cached else client.get(endpoint + "/timeline")
    summary = validate_pair(match, timeline, match_id)
    if not cached:
        write_json(timeline_path, timeline)
    manifest = {
        "schema": 1, "source": "Riot Match-v5", "patch": patch, "queue": 420,
        "validated_at": datetime.now(timezone.utc).isoformat(), **summary,
        "sha256": {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                   for p in (match_path, timeline_path)},
    }
    write_json(folder / "complete.json", manifest)
    return {"status": "cached" if cached else "complete", **summary}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--match-ids", type=Path, required=True, help="Private seed file, one match ID per line")
    parser.add_argument("--region", choices=REGIONS, required=True)
    parser.add_argument("--patches", nargs="+", required=True, help="Internal gameVersion patches, e.g. 16.19")
    parser.add_argument("--limit", type=int, default=100, help="Maximum seed matches examined this invocation")
    parser.add_argument("--output", type=Path, default=Path.home() / "data/recall/riot")
    parser.add_argument("--key-file", type=Path, default=Path.home() / ".config/recall/riot-api-key")
    args = parser.parse_args()
    if args.limit <= 0 or any(not PATCH.fullmatch(p) for p in args.patches):
        parser.error("Use a positive limit and patches in major.minor format")
    client = RiotClient(load_key(args.key_file), args.region)
    seeds = list(dict.fromkeys(line.strip() for line in args.match_ids.read_text().splitlines()
                              if line.strip() and not line.lstrip().startswith("#")))
    if any(not MATCH_ID.fullmatch(seed) for seed in seeds):
        parser.error("Seed file contains an invalid match ID")
    root = private_output(args.output) / args.region
    counts = Counter()
    for index, match_id in enumerate(seeds[:args.limit], 1):
        try:
            result = collect_pair(client, root, match_id, set(args.patches))
        except ApiError as error:
            if error.status != 404:
                raise
            result = {"status": "unavailable"}
        counts[result["status"]] += 1
        print(f"Examined {index}/{min(len(seeds), args.limit)}: {dict(counts)}", flush=True)
    print("Raw responses and validation manifests saved outside the model pack.")


if __name__ == "__main__":
    try:
        main()
    except (ApiError, ValueError, OSError) as error:
        # Only locally constructed validation/status errors, never HTTP response bodies.
        print(f"Collection stopped: {error}", file=sys.stderr)
        raise SystemExit(1) from None
