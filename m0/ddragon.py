"""Data Dragon: patch version plus item/champion catalogs, cached under data/cache/ddragon.

Everything in the data pack refers to items and champions by *name*; this module
resolves names to the numeric ids the client and the Live Client API use.
"""
from __future__ import annotations

import json
import re
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Optional

REPO_ROOT = Path(__file__).resolve().parents[1]
CACHE_DIR = REPO_ROOT / "data" / "cache" / "ddragon"
BASE = "https://ddragon.leagueoflegends.com"
SR_MAP_ID = "11"


def _fetch(url: str, timeout: float = 20) -> bytes:
    with urllib.request.urlopen(url, timeout=timeout) as r:
        return r.read()


def latest_version(max_age_s: float = 6 * 3600, offline: bool = False) -> str:
    """Newest patch on Data Dragon; falls back to the cached answer when offline."""
    vfile = CACHE_DIR / "versions.json"
    if vfile.exists() and (offline or time.time() - vfile.stat().st_mtime < max_age_s):
        return json.loads(vfile.read_text())[0]
    try:
        data = _fetch(f"{BASE}/api/versions.json")
    except (OSError, urllib.error.URLError):
        if vfile.exists():
            return json.loads(vfile.read_text())[0]
        raise
    CACHE_DIR.mkdir(parents=True, exist_ok=True)
    vfile.write_bytes(data)
    return json.loads(data)[0]


def load(kind: str, version: Optional[str] = None, locale: str = "en_US") -> dict:
    """kind: 'item' | 'champion' | 'summoner' | 'runesReforged'."""
    version = version or latest_version()
    f = CACHE_DIR / version / f"{kind}.json"
    if not f.exists():
        f.parent.mkdir(parents=True, exist_ok=True)
        f.write_bytes(_fetch(f"{BASE}/cdn/{version}/data/{locale}/{kind}.json"))
    return json.loads(f.read_text(encoding="utf-8"))


def normalize_name(name: str) -> str:
    """'B. F. Sword' == 'B.F. Sword' == 'bf sword' -> 'bfsword'."""
    return re.sub(r"[^a-z0-9]+", "", name.lower())


class ItemIndex:
    def __init__(self, item_json: dict, version: str = "?") -> None:
        self.version = version
        self.data: dict[str, dict] = item_json["data"]
        self._by_name: dict[str, str] = {}
        for iid, it in self.data.items():
            key = normalize_name(it["name"])
            cur = self._by_name.get(key)
            if cur is None or self._rank(iid) < self._rank(cur):
                self._by_name[key] = iid

    def _rank(self, iid: str) -> tuple:
        """Among same-named entries prefer the real SR shop item (e.g. 6676 over 667666)."""
        it = self.data[iid]
        return (
            0 if it.get("maps", {}).get(SR_MAP_ID) else 1,
            0 if it.get("gold", {}).get("purchasable") else 1,
            0 if it.get("inStore", True) else 1,
            len(iid),
            int(iid) if iid.isdigit() else 0,
        )

    def id_for(self, name: str) -> Optional[str]:
        return self._by_name.get(normalize_name(name))

    def name(self, iid) -> str:
        it = self.data.get(str(iid))
        return it["name"] if it else f"item {iid}"

    def cost(self, iid) -> int:
        return int(self.data.get(str(iid), {}).get("gold", {}).get("total", 0))

    def components(self, iid) -> list[str]:
        """Direct recipe, in Data Dragon order (duplicates preserved)."""
        return [str(c) for c in self.data.get(str(iid), {}).get("from", [])]

    def leaf_components(self, iid) -> list[str]:
        """Fully expanded basic components, in recipe order."""
        out: list[str] = []
        for c in self.components(iid):
            sub = self.components(c)
            out.extend(self.leaf_components(c) if sub else [c])
        return out

    def purchasable_on_sr(self, iid) -> bool:
        it = self.data.get(str(iid), {})
        return bool(it.get("maps", {}).get(SR_MAP_ID)) and bool(it.get("gold", {}).get("purchasable"))


class ChampionIndex:
    def __init__(self, champion_json: dict, version: str = "?") -> None:
        self.version = version
        self.by_key: dict[int, dict] = {}
        self._by_name: dict[str, int] = {}
        for c in champion_json["data"].values():
            key = int(c["key"])
            self.by_key[key] = c
            self._by_name[normalize_name(c["name"])] = key
            self._by_name.setdefault(normalize_name(c["id"]), key)  # e.g. MonkeyKing -> Wukong

    def name(self, key) -> str:
        if not key:
            return "-"
        c = self.by_key.get(int(key))
        return c["name"] if c else f"champ {key}"

    def key_for(self, name: str) -> Optional[int]:
        return self._by_name.get(normalize_name(name))


def load_indexes(version: Optional[str] = None, offline: bool = False) -> tuple[ItemIndex, ChampionIndex, str]:
    version = version or latest_version(offline=offline)
    return ItemIndex(load("item", version), version), ChampionIndex(load("champion", version), version), version
