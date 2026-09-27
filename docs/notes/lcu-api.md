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
| Runes | `GET/POST /lol-perks/v1/pages`, `PUT/DELETE /lol-perks/v1/pages/{id}`, `PUT /lol-perks/v1/currentpage`, `GET /lol-perks/v1/inventory` | Page slots: see Limits below |

## Limits (observed on patch 16.19, 2026-09-26, in a draft champ select)
- **Rune page slots.** `GET /lol-perks/v1/inventory` returned
  `{"canAddCustomPage": false, "customPageCount": 2, "isCustomPageCreationUnlocked": true, "ownedPageCount": 2}`.
  `canAddCustomPage` is the client's own answer. A page the client ties to a Swiftplay pick
  (`quickPlayChampionIds: [498]`, `isTemporary: false`) reports `isDeletable: false` yet still takes a
  slot; the client's temporary Swiftplay page (`isTemporary: true`) does not. Counting only deletable
  pages made the importer POST into a full inventory: `HTTP 400 {"errorCode":"RPC_ERROR","message":"Max pages reached"}`.
  Champion select now updates our page for the loadout or our free page, creates a page only while a
  slot is free, and otherwise borrows our Swiftplay-tied page with its `quickPlayChampionIds` and
  `isTemporary` sent back unchanged (`core/src/runes.rs`); Swiftplay preparation rewrites that page
  for its pick in the next lobby. Personal pages are never written or deleted.
- **Item-set upload size.** A `PUT .../sets` body above about 64 KiB fails with
  `HTTP 413 {"errorCode":"BAD_REQUEST_HEADERS","message":"Content length is too large"}`. The account's
  30 sets (all ours, about 2.2 KB each) were 64,891 bytes and accepted; one more set was refused.
  `itemset::merge` keeps uploads under 60 KiB by dropping our own oldest sets, never anyone else's.

## Item set schema (`LolItemSetsItemSets` / `LolItemSetsItemSet`)
```json
{"accountId": 0, "timestamp": 0, "itemSets": [{
  "uid": "uuid", "title": "Recall Xayah", "type": "custom", "map": "any", "mode": "any",
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
- Captured from a real Practice Tool session (fixtures `champselect_practicetool_*.json`):
  hovering sets `championPickIntent`, locking sets `championId` and resets the intent to 0;
  `actions[][]` also carry `duration`; custom games have `assignedPosition: ""`, `theirTeam: []`,
  `isCustomGame: true`, `queueId`. `chatDetails.mucJwtDto.jwt` is a real token: never commit
  raw dumps, scrub them first (see the scrub step in `docs/notes/m0-log.md`).
- Still to capture: a draft game with enemy bans and lock-ins (`watch_champselect.py --dump`).
- The client also offers a WebSocket (`wss://127.0.0.1:<port>/`, subscribe
  `[5,"OnJsonApiEvent_lol-champ-select_v1_session"]`) for push updates; polling at 1 s is
  fine for M0 and avoids a dependency.
