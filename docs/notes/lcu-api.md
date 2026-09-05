# LCU API notes (checked against the client's /help on patch 16.17, 2026-09-05)

## Connecting
- Lockfile `C:\Riot Games\League of Legends\lockfile`: `LeagueClient:<pid>:<port>:<password>:https`.
  Removed when the client exits; may go stale after a crash (then the port refuses).
- Auth: HTTP Basic, user `riot`, password from the lockfile. Self-signed cert; verification
  is disabled for local calls.
- Fallback discovery: `LeagueClientUx.exe` command line has `--app-port=` and
  `--remoting-auth-token=` (PowerShell `Get-CimInstance Win32_Process`).
- `GET /help?format=Full` lists every function/type the running client exposes
  (3 MB JSON; `types`, `functions`, `events` are arrays).

## Endpoints used
| Purpose | Method + path | Notes |
|---|---|---|
| Phase | `GET /lol-gameflow/v1/gameflow-phase` | `"None" "Lobby" "Matchmaking" "ReadyCheck" "ChampSelect" "GameStart" "InProgress" "WaitingForStats" "PreEndOfGame" "EndOfGame" "Reconnect" ...` |
| Me | `GET /lol-summoner/v1/current-summoner` | `gameName`, `tagLine`, `summonerId`, `puuid`, `summonerLevel` |
| Champ select | `GET /lol-champ-select/v1/session` | 404 `{"errorCode":"RPC_ERROR","httpStatus":404,"message":"No active delegate"}` outside champ select |
| My champion | `GET /lol-champ-select/v1/current-champion` | int, 0 before lock |
| Spells | `PATCH /lol-champ-select/v1/session/my-selection` | `{"spell1Id","spell2Id"}` |
| Item sets | `GET/PUT /lol-item-sets/v1/item-sets/{summonerId}/sets` | PUT **replaces all** sets: GET, modify, PUT. `POST .../sets` adds one set |
| Runes | `GET/POST /lol-perks/v1/pages`, `DELETE /lol-perks/v1/pages/{id}`, `PUT /lol-perks/v1/currentpage`, `GET /lol-perks/v1/inventory` | M1 |

## Item set schema (`LolItemSetsItemSets` / `LolItemSetsItemSet`)
```json
{"accountId": 0, "timestamp": 0, "itemSets": [{
  "uid": "uuid", "title": "Featherstorm Xayah", "type": "custom", "map": "any", "mode": "any",
  "sortrank": 0, "startedFrom": "blank",
  "associatedChampions": [498], "associatedMaps": [], "preferredItemSlots": [],
  "blocks": [{"type": "Starting Items", "hideIfSummonerSpell": "", "showIfSummonerSpell": "",
              "items": [{"id": "1055", "count": 1}]}]
}]}
```
Item ids are strings. `associatedMaps: []` means every map (op.gg does this); SR is 11.
The op.gg app writes one set titled `OP.GG <Champion>` with ~12 blocks; block titles up to
~45 characters render fine in the shop.

## Champ select session (shape used by `m0/champselect.py`)
- `localPlayerCellId`, `myTeam[]` / `theirTeam[]` with `cellId`, `championId`,
  `championPickIntent`, `assignedPosition` (`top|jungle|middle|bottom|utility`, empty for
  enemies), `spell1Id`, `spell2Id`.
- `actions[][]` with `actorCellId`, `championId`, `type` (`ban|pick|ten_bans_reveal`),
  `completed`, `isAllyAction`, `isInProgress`. A completed `pick` = locked in.
- `bans.myTeamBans[]`, `bans.theirTeamBans[]`; `timer.phase` (`PLANNING|BAN_PICK|FINALIZATION|GAME_STARTING`).
- Not yet captured from a real session: run `python3 m0/watch_champselect.py --dump m0/tests/fixtures/captured`
  during the next game and replace the hand-written fixture.
- The client also offers a WebSocket (`wss://127.0.0.1:<port>/`, subscribe
  `[5,"OnJsonApiEvent_lol-champ-select_v1_session"]`) for push updates; polling at 1 s is
  fine for M0 and avoids a dependency.
