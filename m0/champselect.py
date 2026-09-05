"""Champ select session -> compact state, plus diffs between polls ("print as champs lock in").

Field names follow the LCU /lol-champ-select/v1/session payload. Until a real
session has been captured with `watch_champselect.py --dump`, the fixture in
tests/fixtures is hand-written from the documented shape.
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Optional

from ddragon import ChampionIndex

POSITIONS = {"top": "top", "jungle": "jungle", "middle": "mid", "bottom": "bot", "utility": "support"}


def pos_label(position: str) -> str:
    return POSITIONS.get(position, position)


@dataclass(frozen=True)
class Cell:
    cell_id: int
    ally: bool
    champion_id: int      # locked (or, for allies mid-turn, currently selected) champion; 0 if none
    intent_id: int        # declared pick intent (hover) - allies only
    locked: bool
    position: str         # top | jungle | middle | bottom | utility | '' (enemies, blind pick)
    is_me: bool

    @property
    def shown_champion(self) -> int:
        return self.champion_id or self.intent_id


@dataclass
class State:
    phase: str            # timer phase: PLANNING | BAN_PICK | FINALIZATION | GAME_STARTING
    my_cell: int
    cells: dict[int, Cell]
    ally_bans: tuple[int, ...]
    enemy_bans: tuple[int, ...]

    @property
    def me(self) -> Optional[Cell]:
        return self.cells.get(self.my_cell)

    def allies(self) -> list[Cell]:
        return sorted((c for c in self.cells.values() if c.ally), key=lambda c: c.cell_id)

    def enemies(self) -> list[Cell]:
        return sorted((c for c in self.cells.values() if not c.ally), key=lambda c: c.cell_id)

    def locked(self, ally: bool) -> list[int]:
        return [c.champion_id for c in (self.allies() if ally else self.enemies()) if c.locked and c.champion_id]

    @property
    def all_locked(self) -> bool:
        cells = list(self.cells.values())
        return bool(cells) and all(c.locked for c in cells)


def extract(session: dict) -> State:
    my_cell = int(session.get("localPlayerCellId", -1))
    completed: set[int] = set()
    for phase in session.get("actions") or []:
        for a in phase or []:
            if a.get("type") == "pick" and a.get("completed"):
                completed.add(int(a.get("actorCellId", -1)))

    cells: dict[int, Cell] = {}
    for ally, key in ((True, "myTeam"), (False, "theirTeam")):
        for p in session.get(key) or []:
            cid = int(p.get("cellId", -1))
            champ = int(p.get("championId") or 0)
            intent = int(p.get("championPickIntent") or 0)
            # Enemy champion ids only become visible once locked; ally ids can show mid-turn.
            locked = cid in completed or (not ally and champ > 0)
            cells[cid] = Cell(cid, ally, champ, intent, locked,
                              (p.get("assignedPosition") or "").lower(), cid == my_cell)

    bans = session.get("bans") or {}

    def ids(v) -> tuple[int, ...]:
        return tuple(int(b) for b in (v or []) if b)

    return State(
        phase=str((session.get("timer") or {}).get("phase") or "?"),
        my_cell=my_cell,
        cells=cells,
        ally_bans=ids(bans.get("myTeamBans")),
        enemy_bans=ids(bans.get("theirTeamBans")),
    )


def label(cell: Cell) -> str:
    side = "ally" if cell.ally else "enemy"
    s = f"{side} {pos_label(cell.position)}" if cell.position else f"{side} cell {cell.cell_id}"
    return s + (" (you)" if cell.is_me else "")


def _new_bans(old: tuple[int, ...], new: tuple[int, ...]) -> list[int]:
    return list(new[len(old):]) if new[:len(old)] == old else [b for b in new if b not in old]


def summary(state: State, champs: Optional[ChampionIndex] = None) -> list[str]:
    def nm(cid: int) -> str:
        return champs.name(cid) if champs else str(cid)

    def team(cells: list[Cell]) -> str:
        parts = []
        for c in cells:
            s = nm(c.shown_champion) if c.shown_champion else "?"
            if c.position:
                s += f" ({pos_label(c.position)})"
            if c.is_me:
                s += " *you*"
            parts.append(s)
        return ", ".join(parts)

    return [f"ALLY : {team(state.allies())}", f"ENEMY: {team(state.enemies())}"]


def diff(prev: Optional[State], cur: State, champs: Optional[ChampionIndex] = None) -> list[str]:
    """Human-readable changes since the previous poll; the first call describes the lobby."""
    def nm(cid: int) -> str:
        return champs.name(cid) if champs else str(cid)

    out: list[str] = []
    if prev is None:
        me = cur.me
        where = f"cell {cur.my_cell}" + (f", {pos_label(me.position)}" if me and me.position else "")
        out.append(f"champ select started (phase {cur.phase}); you are {where}")
        for c in cur.allies() + cur.enemies():
            if c.locked and c.champion_id:
                out.append(f"[LOCK]  {label(c)}: {nm(c.champion_id)}")
            elif c.shown_champion:
                out.append(f"[HOVER] {label(c)}: {nm(c.shown_champion)}")
        for b in cur.ally_bans:
            out.append(f"[BAN]   ally banned {nm(b)}")
        for b in cur.enemy_bans:
            out.append(f"[BAN]   enemy banned {nm(b)}")
        if cur.all_locked:
            out.extend(summary(cur, champs))
        return out

    if cur.phase != prev.phase:
        out.append(f"[PHASE] {prev.phase} -> {cur.phase}")
    for side, old, new in (("ally", prev.ally_bans, cur.ally_bans), ("enemy", prev.enemy_bans, cur.enemy_bans)):
        for b in _new_bans(old, new):
            out.append(f"[BAN]   {side} banned {nm(b)}")
    for cid, c in sorted(cur.cells.items()):
        p = prev.cells.get(cid)
        if c.locked and c.champion_id and not (p and p.locked):
            out.append(f"[LOCK]  {label(c)}: {nm(c.champion_id)}")
        elif not c.locked and c.shown_champion and c.shown_champion != (p.shown_champion if p else 0):
            out.append(f"[HOVER] {label(c)}: {nm(c.shown_champion)}")
    if cur.all_locked and not prev.all_locked:
        out.extend(summary(cur, champs))
    return out
