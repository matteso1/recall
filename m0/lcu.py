"""LCU (League Client Update) API: the client's local HTTPS API, authenticated from its lockfile.

This is the same mechanism op.gg / Blitz use for rune, spell and item-set imports.
Endpoint names were checked against the client's own /help listing (patch 16.17).
"""
from __future__ import annotations

from typing import Any, Optional

from transport import Response, Transport, make_transport
from winenv import ClientNotRunning, Lockfile, lockfile_from_process, read_lockfile


class LCUError(Exception):
    def __init__(self, method: str, path: str, response: Response) -> None:
        self.status = response.status
        self.body = response.text()
        super().__init__(f"{method} {path} -> HTTP {response.status}: {self.body[:300]}")


class LCU:
    def __init__(self, lockfile: Lockfile, host: str = "127.0.0.1",
                 transport: Optional[Transport] = None) -> None:
        self.lockfile = lockfile
        self.host = host
        self.base = f"{lockfile.protocol}://{host}:{lockfile.port}"
        self.transport = transport or make_transport()

    @classmethod
    def connect(cls, host: str = "127.0.0.1", transport: Optional[Transport] = None) -> "LCU":
        """Find the running client (lockfile first, process command line as fallback)."""
        try:
            lockfile = read_lockfile()
        except ClientNotRunning:
            lockfile = lockfile_from_process()  # raises ClientNotRunning if it is not running
        return cls(lockfile, host=host, transport=transport)

    # ----------------------------------------------------------------- plumbing

    def request(self, method: str, path: str, body: Any = None, timeout: float = 5.0) -> Response:
        return self.transport.request(method, self.base + path, body=body,
                                      auth=self.lockfile.auth, timeout=timeout)

    def _call(self, method: str, path: str, body: Any = None, ok404: bool = False) -> Any:
        r = self.request(method, path, body)
        if r.status == 404 and ok404:
            return None
        if not r.ok:
            raise LCUError(method, path, r)
        return r.json()

    def get(self, path: str, ok404: bool = False) -> Any:
        return self._call("GET", path, ok404=ok404)

    def put(self, path: str, body: Any) -> Any:
        return self._call("PUT", path, body)

    def post(self, path: str, body: Any = None) -> Any:
        return self._call("POST", path, body)

    def patch(self, path: str, body: Any) -> Any:
        return self._call("PATCH", path, body)

    def delete(self, path: str) -> Any:
        return self._call("DELETE", path)

    # ------------------------------------------------------------------ gameflow

    def gameflow_phase(self) -> str:
        """None | Lobby | Matchmaking | ReadyCheck | ChampSelect | GameStart | InProgress |
        WaitingForStats | PreEndOfGame | EndOfGame | Reconnect | ..."""
        return self.get("/lol-gameflow/v1/gameflow-phase")

    def gameflow_session(self) -> Optional[dict]:
        return self.get("/lol-gameflow/v1/session", ok404=True)

    # ------------------------------------------------------------------ summoner

    def current_summoner(self) -> dict:
        """gameName, tagLine, summonerId, puuid, summonerLevel, ..."""
        return self.get("/lol-summoner/v1/current-summoner")

    # --------------------------------------------------------------- champ select

    def champ_select_session(self) -> Optional[dict]:
        """The live champ select session, or None when not in champ select (HTTP 404)."""
        return self.get("/lol-champ-select/v1/session", ok404=True)

    def current_champion(self) -> int:
        r = self.request("GET", "/lol-champ-select/v1/current-champion")
        return int(r.json() or 0) if r.ok else 0

    def set_summoner_spells(self, spell1_id: int, spell2_id: int) -> Any:
        return self.patch("/lol-champ-select/v1/session/my-selection",
                          {"spell1Id": spell1_id, "spell2Id": spell2_id})

    # ----------------------------------------------------------------- item sets

    def item_sets(self, summoner_id: int) -> dict:
        """{accountId, timestamp, itemSets: [...]} - every custom set on the account."""
        return self.get(f"/lol-item-sets/v1/item-sets/{summoner_id}/sets")

    def put_item_sets(self, summoner_id: int, payload: dict) -> Any:
        """Replaces ALL item sets: always GET, modify, PUT (see itemsets.upsert)."""
        return self.put(f"/lol-item-sets/v1/item-sets/{summoner_id}/sets", payload)

    # --------------------------------------------------------------- runes (M1)

    def perk_inventory(self) -> dict:
        return self.get("/lol-perks/v1/inventory")

    def perk_pages(self) -> list:
        return self.get("/lol-perks/v1/pages")

    def create_perk_page(self, page: dict) -> dict:
        """page = {name, primaryStyleId, subStyleId, selectedPerkIds: [9 ids], current: True}"""
        return self.post("/lol-perks/v1/pages", page)

    def delete_perk_page(self, page_id: int) -> Any:
        return self.delete(f"/lol-perks/v1/pages/{page_id}")

    def set_current_perk_page(self, page_id: int) -> Any:
        return self.put("/lol-perks/v1/currentpage", page_id)
