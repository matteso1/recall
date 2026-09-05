"""Live Client Data API (https://127.0.0.1:2999/liveclientdata) - only up while a game runs.

Everything here is information the player can already see by pressing Tab:
own gold/items/level/abilities, everyone's visible items, scores, game time.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Optional

from transport import ConnectionFailed, Transport, make_transport

DEFAULT_PORT = 2999


class LiveClient:
    def __init__(self, host: str = "127.0.0.1", port: int = DEFAULT_PORT,
                 transport: Optional[Transport] = None) -> None:
        self.base = f"https://{host}:{port}/liveclientdata"
        self.transport = transport or make_transport()

    def get(self, path: str, timeout: float = 4.0):
        return self.transport.request("GET", f"{self.base}/{path.lstrip('/')}", timeout=timeout)

    def all_game_data(self) -> Optional[dict]:
        """Full game state, or None when no game is running (loading screen included)."""
        try:
            r = self.get("allgamedata")
        except ConnectionFailed:
            return None
        if not r.ok:
            return None
        data = r.json()
        return data if isinstance(data, dict) and "activePlayer" in data else None


# ------------------------------------------------------------------ snapshots


@dataclass(frozen=True)
class Item:
    slot: int
    id: int
    name: str
    count: int = 1


@dataclass
class PlayerState:
    name: str
    champion: str
    team: str
    position: str
    level: int
    items: tuple[Item, ...]
    kills: int = 0
    deaths: int = 0
    assists: int = 0
    cs: int = 0
    is_me: bool = False
    gold: Optional[float] = None
    abilities: dict = field(default_factory=dict)  # {"Q": 1, "W": 0, "E": 2, "R": 0}

    @property
    def kda(self) -> str:
        return f"{self.kills}/{self.deaths}/{self.assists}"

    def item_names(self) -> list[str]:
        return [f"{i.name}x{i.count}" if i.count > 1 else i.name for i in self.items]


@dataclass
class Snapshot:
    game_time: float
    mode: str
    me: Optional[PlayerState]
    allies: list[PlayerState]
    enemies: list[PlayerState]

    def everyone(self) -> list[PlayerState]:
        return ([self.me] if self.me else []) + self.allies + self.enemies


def fmt_time(seconds: float) -> str:
    s = int(seconds)
    return f"{s // 60:02d}:{s % 60:02d}"


def _items(raw: list) -> tuple[Item, ...]:
    return tuple(
        sorted(
            (Item(int(i.get("slot", 0)), int(i.get("itemID", 0)), i.get("displayName", "?"), int(i.get("count", 1)))
             for i in raw or []),
            key=lambda i: i.slot,
        )
    )


def summarize(data: dict) -> Snapshot:
    active = data.get("activePlayer") or {}
    my_id = active.get("riotId") or active.get("summonerName") or ""
    game = data.get("gameData") or {}

    me: Optional[PlayerState] = None
    allies: list[PlayerState] = []
    enemies: list[PlayerState] = []
    players: list[PlayerState] = []
    for p in data.get("allPlayers") or []:
        scores = p.get("scores") or {}
        pid = p.get("riotId") or p.get("summonerName") or ""
        ps = PlayerState(
            name=pid,
            champion=p.get("championName", "?"),
            team=p.get("team", "?"),
            position=p.get("position") or "",
            level=int(p.get("level", 0)),
            items=_items(p.get("items")),
            kills=int(scores.get("kills", 0)),
            deaths=int(scores.get("deaths", 0)),
            assists=int(scores.get("assists", 0)),
            cs=int(scores.get("creepScore", 0)),
            is_me=(pid == my_id and bool(my_id)),
        )
        if ps.is_me:
            ps.gold = float(active.get("currentGold", 0.0))
            ps.abilities = {
                k: int((v or {}).get("abilityLevel", 0))
                for k, v in (active.get("abilities") or {}).items()
                if k in ("Q", "W", "E", "R")
            }
            me = ps
        players.append(ps)

    my_team = me.team if me else None
    for ps in players:
        if ps.is_me:
            continue
        (allies if my_team and ps.team == my_team else enemies).append(ps)

    return Snapshot(
        game_time=float(game.get("gameTime", 0.0)),
        mode=game.get("gameMode", "?"),
        me=me,
        allies=allies,
        enemies=enemies,
    )


def _item_bag(ps: PlayerState) -> dict[tuple[int, str], int]:
    bag: dict[tuple[int, str], int] = {}
    for i in ps.items:
        bag[(i.id, i.name)] = bag.get((i.id, i.name), 0) + i.count
    return bag


def _item_changes(before: Optional[PlayerState], after: PlayerState) -> list[str]:
    old, new = (_item_bag(before) if before else {}), _item_bag(after)
    lines = []
    for key in sorted(set(old) | set(new), key=lambda k: k[1]):
        d = new.get(key, 0) - old.get(key, 0)
        if d > 0:
            lines.append(f"+ {key[1]}" + (f" x{d}" if d > 1 else ""))
        elif d < 0:
            lines.append(f"- {key[1]}" + (f" x{-d}" if d < -1 else ""))
    return lines


def diff(prev: Optional[Snapshot], cur: Snapshot, include_allies: bool = False) -> list[str]:
    """Human-readable changes between two snapshots (first call describes the game)."""
    out: list[str] = []
    if prev is None:
        who = f"{cur.me.champion} ({cur.me.position or 'no lane'})" if cur.me else "spectating?"
        out.append(f"game detected: {cur.mode} at {fmt_time(cur.game_time)}, you are {who}")
        out.append("allies : " + ", ".join(p.champion for p in cur.allies))
        out.append("enemies: " + ", ".join(p.champion for p in cur.enemies))
        if cur.me:
            out.append(f"you: lvl {cur.me.level}, {cur.me.gold:.0f}g, items: {', '.join(cur.me.item_names()) or 'none'}")
        return out

    if cur.me:
        pm = prev.me
        if pm and cur.me.level != pm.level:
            ab = " ".join(f"{k}{v}" for k, v in cur.me.abilities.items())
            out.append(f"LEVEL {pm.level} -> {cur.me.level}   abilities now {ab}")
        elif pm and cur.me.abilities != pm.abilities:
            changed = [k for k in cur.me.abilities if cur.me.abilities.get(k) != pm.abilities.get(k)]
            out.append(f"skilled {'/'.join(changed)} -> " + " ".join(f"{k}{v}" for k, v in cur.me.abilities.items()))
        for line in _item_changes(pm, cur.me):
            out.append(f"you {line}")
        if pm and (cur.me.kills, cur.me.deaths, cur.me.assists) != (pm.kills, pm.deaths, pm.assists):
            out.append(f"you KDA {cur.me.kda}")

    def by_name(players: list[PlayerState]) -> dict[str, PlayerState]:
        return {p.name or p.champion: p for p in players}

    groups = [("enemy", prev.enemies, cur.enemies)]
    if include_allies:
        groups.append(("ally", prev.allies, cur.allies))
    for label, before, after in groups:
        b = by_name(before)
        for key, ps in by_name(after).items():
            for line in _item_changes(b.get(key), ps):
                out.append(f"{label} {ps.champion} {line}")
    return out


def heartbeat(snap: Snapshot) -> str:
    if not snap.me:
        return f"[{fmt_time(snap.game_time)}] no active player"
    m = snap.me
    ab = " ".join(f"{k}{v}" for k, v in m.abilities.items())
    return (f"[{fmt_time(snap.game_time)}] {m.champion} lvl {m.level} | {m.gold:.0f}g | {m.kda} | "
            f"{ab} | {', '.join(m.item_names()) or 'no items'}")
