# Live Client Data API notes

- Base: `https://127.0.0.1:2999/liveclientdata/` on the machine running the game. No auth,
  self-signed cert. Only up while a game is running; **not** during the loading screen.
  Practice Tool exposes it fully.
- `GET allgamedata` returns everything at once (`activePlayer`, `allPlayers`, `events`,
  `gameData`). Individual endpoints exist (`activeplayer`, `playerlist`,
  `playeritems?riotId=`, `eventdata`, `gamestats`) but one call every 2 s is plenty.
- `activePlayer`: `currentGold`, `level`, `abilities.{Q,W,E,R}.abilityLevel`,
  `championStats`, `fullRunes`, `riotId` (`Name#TAG`).
- `allPlayers[]`: `championName`, `team` (`ORDER`/`CHAOS`), `position`
  (`TOP|JUNGLE|MIDDLE|BOTTOM|UTILITY`, empty in non-SR modes), `level`, `items[]`
  (`itemID`, `displayName`, `count`, `slot` 0-6, `price`), `scores`, `summonerSpells`,
  `riotId`. Enemy items are the ones visible on Tab, which is exactly what we may use.
- Match the active player to `allPlayers` by `riotId`.
- `gameData`: `gameMode` (`CLASSIC`, `PRACTICETOOL`, `ARAM`...), `gameTime` seconds, `mapNumber`.
- Practice Tool reports `position: "NONE"` and `abilities.Passive: null`; `gameMode` is
  `PRACTICETOOL`. Real capture: `m0/tests/fixtures/allgamedata_practicetool.json`
  (`allgamedata.json` is a hand-written 5v5 stand-in until a real match is captured).
