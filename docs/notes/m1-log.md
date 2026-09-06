# M1 log

## 2026-09-05
- 13:xx Rust workspace written: `core` (brain) + `src-tauri` (shell) + `ui/`. Core compiled first try
  in WSL, 16 tests green.
- VS Build Tools 2022 (C++ workload) installed after three UAC attempts (the prompt times out in 120 s).
- 14:19 First Windows debug build of the whole Tauri app: clean, 41 s, zero errors. 22 MB debug exe.
- 14:23 `--probe` headless mode added (no window; for checks while the screen is busy). Output on this
  machine with the client closed:
  ```
  ddragon 16.17.1 (868 items, 173 champions, 62 runes)
  client: no lockfile (client not running)
  engine vs Tristana/Soraka/Malphite/Ornn/Thresh:
    path: ER, Greaves, IE, Mortal (Soraka), Navori, GA
    next: Essence Reaver -> buy Sheen
    why: "Mortal Reminder over LDR: Soraka heals", "Malphite and Ornn both build armor: armor pen earlier"
    matchup: vs Tristana line
  ```
- 14:27 Release build (opt-level s, LTO, strip) at BelowNormal priority with -j 6 while the user was
  gaming: 3 m 47 s, 9.6 MB `recall.exe`. Headless probe passes on the release exe as well.
- Pending: on-screen test with the League client open (user was gaming), Practice Tool dogfood, draft game.
- 17:53 First on-screen run (release exe): launched, connected to the client (`connected to client on
  port 51493 as matteso#NA1`), panel rendered as a collapsed 380x64 strip at the top-middle of the
  screen instead of expanded bottom-right. Cause: `%LOCALAPPDATA%\Recall\settings.json` held
  `{"x":3803,"y":322,"collapsed":true}` from an earlier run, and the saved position is honoured
  as-is. Fix next: validate saved position against the monitor, do not start collapsed unless the
  user collapsed it this session (or drop persisting `collapsed`), delete the stale settings.json.
  Monitor: 5120x2160 physical at 150% (Windows reports 3413x1440 logical to non-DPI-aware code).
  Window exstyle 0x8040118 (TOPMOST + NOACTIVATE present).
- `scripts/overlay-run.sh` used to hang its caller: the detached exe inherited the stdout pipe.
  Fixed with full stdio redirection + nohup.
- `scripts/overlay-visual-test.sh` never completed because of that hang; re-test after the fix.
- Session moved to the terminal TUI at the user's request (handoff prompt given).
- 18:05 Startup fix. `%LOCALAPPDATA%\Recall` turned out to be empty already (settings.json, log
  and the Data Dragon cache all gone), so there was nothing stale left to delete; the fix makes the
  stale case impossible anyway. New `core::placement` (pure geometry, 9 tests): the saved position is
  used only when the panel's header is entirely on some monitor, otherwise the bottom-right default,
  which now also stays above the taskbar (the taskbar is a topmost window too and covered the bottom
  54 px of the old default on the desktop). `collapsed` is no longer persisted: the panel always
  starts expanded. `Settings` is position-only; unknown keys in an old file are ignored.
  The main.rs `Moved` handler still saves drags, but a programmatic `set_position` at startup did not
  raise `Moved` (no settings.json after the first run), so the placement is saved explicitly.
- 18:13 Release rebuild 1 m 45 s. Probe passes (client `None`, matteso#NA1, Data Dragon 16.17.1 re-cached).
  On-screen: window at (3884,1692) 570x450 physical = exactly the computed default, expanded, header +
  `idle` pill + "Ready. Start a game." + "matteso#NA1 · patch 16.17.1". Verified with the new
  `scripts/win-rect.sh` (GetWindowRect in physical pixels) rather than eyeballing a 5120x2160 PNG;
  `scripts/win-screenshot.sh out.png x,y,w,h` now crops. The user dragged the panel to (4448,149)
  during the test and settings.json followed (`{"x":4448,"y":149}`), so drag persistence works.
- `scripts/capture.sh start|stop|status`: both M0 watchers detached with `--dump` into
  `m0/tests/fixtures/captured/`, tracked by pidfile (a `pgrep -f` pattern matched the very shell that
  launched it, the gotcha from the notes). Running since 18:13 so the Practice Tool / draft payloads
  are captured whenever the user plays.
- 18:21 Rebuilt (1 m 23 s; the running exe has to be stopped first or cargo cannot replace it) and
  verified both placement paths on the real machine with `win-rect.sh`: restart with the user's saved
  (4448,149) -> kept (`[saved position]`); planted `{"x":9000,"y":9000,"collapsed":true}` -> panel at
  (3884,1620) = default above the 72 px taskbar, expanded (`[default placement], saved was Some((9000,
  9000))`); restored the user's position afterwards. Log now prints the monitor and work area
  (`5120x2160 ... work area 5120x2088`). Step 1 + 2 of the handoff done; the overlay is running at the
  user's chosen spot with the client open, capture.sh is recording. Next: Practice Tool dogfood (needs
  the user at the keyboard), then a draft game for enemy-driven swaps.
- 18:26-18:33 Practice Tool dogfood (user at the keyboard, Xayah). Panel fine in game (borderless,
  `WindowMode=2`). All three imports landed: rune page "Recall Xayah" became the current page
  (Slightly Magical Footwear showed up in the live data at 12:00, so it was active in game), the item set
  has the Start / 1..6 / situational / Vision blocks, spells were set. Live data followed the shop:
  Sheen -> Caulfield's -> Cloak -> Essence Reaver, and the skill points were Q1 W1 E3 at level 5, i.e.
  the shown order. 30 raw payloads captured by capture.sh.
- Findings: (1) the overlay log recorded nothing between "connected" and the end of the game: the
  poller logged no phases, the imports no results. Added info lines for gameflow changes, champ-select
  and live plan changes (path/next/why, only when they change), level-ups and import results.
  (2) The user had Flash + Barrier at lock-in; the "Spells" button changed it to the pack's Flash + Heal,
  which read as wrong to them (and the button name read as abilities). Pack default is now Flash +
  Barrier (Draven override dropped as redundant, Ashe -> Cleanse stays), button renamed "Summoner spells".
  (3) The pack's rune page differed from the current op.gg aggregate page in three slots (Alacrity vs
  Bloodline, Cut Down vs Coup de Grace, flat Health vs Health Scaling). No M1 rule adapts runes, so the
  pack now carries the aggregate page; Cut Down vs 2+ tanks is a candidate rule for later.
  (4) With 2 rune pages owned and both in use (op.gg + ours), the import replaces only its own page; a
  user with two foreign pages gets "all 2 rune pages are in use; delete one in the client and retry".
- 18:45 User feedback on the above: hand-tuning pack defaults is the wrong fix; runes and spells should come
  from the aggregate for every champion and be put in the client automatically (design doc 6.1 said so all
  along; the Xayah pack was the M1 shortcut). Probed sources from WSL: lolalytics `ax.lolalytics.com/mega`
  404, u.gg `stats2.u.gg` behind Cloudflare ("Just a moment..."), op.gg `lol-api-champion.op.gg/api/<region>/
  champions/ranked/<key>/<position>?tier=` answers in 0.3 s with no headers needed: summoner_spells,
  runes (top pages with perk ids), core_items, boots, starter_items, last_items, skills, skill_masteries,
  summary.positions (role rates), counters; `meta.version` is the patch. Xayah ADC: Flash + Barrier 84%,
  exactly the "OP.GG adc Xayah" page in the client, Doran's Bow start (item 1086, new this season, the hand
  pack still said Doran's Blade), core Yun Tal > Navori > IE (the pack's ER line is second at 25%).
  Bad champion 404, bad position 422, off-role (Xayah support) returns a 1-game sample.
- 19:30 `core::aggregate` (decode, position choice with a 200-game floor and fallback to the main position,
  6 h disk cache with stale fallback, positions index for champions without an assigned position) + engine
  rewire: aggregate = base (start, core, boots, skill order, runes, spells) for any champion, pack = late
  slots + matchups + rules; no-pack champions fill from popular finished items (no components, no alternative
  first items, one armor-pen item). `Inputs` gains champion / pack Option / aggregate. Item set title per
  champion, "Other popular items" block, situational blocks only with a pack. Real response saved as
  `m0/tests/fixtures/opgg_xayah_adc.json`; item fixture regenerated with 21 more items. 33 core tests.
- Shell: aggregate fetched per (champion, position) in champ select and in game (retry 30 s), auto-import of
  runes + spells at pick/hover (once per champion), item set at lock and on path change; `do_import_*` shared
  with the buttons; settings `auto_runes|auto_spells|auto_itemset|region|tier`; Flash stays on the key it is
  on (`runes::order_spells`). First Windows build failed on a mutex guard held across an await (the rule
  from CLAUDE.md), second passed (1 m 33 s). Probe on the release exe: aggregate fetched from inside the exe
  (87k games, patch 16.17), engine path Yun Tal > Greaves > Navori > Mortal (Soraka) > IE > GA with the why
  lines, spells Flash + Barrier, runes "Lethal Tempo / Inspiration", skills max E > W > Q.
- Next: a real champ select with the new build. Expect in the log: `aggregate: champion 498 as ADC ...`,
  `auto-import runes: Rune page 'Recall Xayah ADC' set`, `auto-import spells: Flash + Barrier selected`,
  `auto-import item set: ...` at lock. Also try any non-Xayah champion.
- 19:25 First real draft game on the aggregate build (Xayah vs Yone, Vi, Katarina, Vayne, Lux). Live path
  Yun Tal > Greaves > Navori > IE > LDR > Maw (AP comp) with the why line; NEXT followed the shop. User asked
  why the shop showed Greaves as "item unavailable" and then "randomly" got boots: the rune page (op.gg's,
  and the old pack's too) has Magical Footwear, which locks boots until the free Slightly Magical Footwear
  arrives (12:00, 45 s earlier per takedown; here about 8:00). The panel had said NEXT: Greaves during the
  lock. Rule 10 added: boots slot tagged `free @12`, NEXT skips locked boots, first why line explains,
  the footwear counts as the Boots component (Greaves shows 800 remaining). Item-set block titles fall back
  to the short name when a tag would push them past 30 chars. 34 core tests. Deploy after the game.
- The log of that game shows the whole chain firing in a *one-second* ChampSelect phase (ReadyCheck ->
  ChampSelect 02:17:02, -> InProgress 02:17:03): a Quickplay-style queue where the champion is chosen in the
  lobby. `aggregate: champion 498 as ADC (op.gg emerald+ global, 87k games, patch 16.17)`, then
  `auto-import runes: Rune page 'Recall Xayah ADC' set`, `auto-import spells: Flash + Barrier selected`,
  `auto-import item set: Item set 'Recall Xayah' is in the client`, all inside that second. Enemies
  were empty at that point (`vs []`), so the pushed item set had GA where the live plan later said Maw (AP
  comp). Known gap for Quickplay: enemy comps are only known in game, and the shop reads item sets at
  game start. Candidate fix: read the gameflow session's team data at GameStart and push once more.
- 19:40 Build review of that game (Swiftplay, 15:58, 4/1/2, path followed exactly: Yun Tal 5:41, Greaves via
  the footwear 7:12, Navori 10:09, IE 14:43; over before slots 5-6). Findings, from the op.gg numbers and
  the traits: (1) the most-picked core line Yun Tal > Navori > IE wins 57.2% (1542 games) while Yun Tal >
  IE > Navori wins 60.1% (639 games, 14% pick): `choose_core` now prefers a line that is popular enough
  (>=10%, >=500 games) and clearly better (>=2 points), with both numbers in a why line; boots, spells and
  runes stay most-picked (their differences are within noise or below the bars: Flash + Exhaust 53.4% at
  7%, Gluttonous Greaves +1.2 points). (2) "Maw over GA: mostly magic damage" hinged on Vi being tagged
  `tank`, which left the AD count at 2 vs 3 AP; Vi is a diver (AD, jungle) and is now `assassin`, so vs
  Vi + Katarina the engine says "defensive item earlier" instead, and the damage split is 3 v 3 (GA stays).
  (3) Champions without a pack now get a matchup line from op.gg's counters (Xayah's worst at emerald+:
  Ashe 39%, Jhin 46%, Twitch 49%, MF 49%). 36 core tests.
- 19:45 Panel redesign (user: "looks vibecoded; should look like part of League"). League's Hextech
  language: navy ground, 1 px gold gradient frame, cream/gold/dim text, teal only for actionable state,
  Cinzel display face (bundled OFL, Beaufort stand-in) + Segoe UI body, Data Dragon icons in gold frames
  for items (NEXT 44 px, path 30 px, components 20 px), enemy champions, keystone/secondary tree, spells and
  starters; owned = dimmed + teal tick, next = lit gold frame, rule tags under the slot (`FREE @12`,
  `SORAKA`). New `--demo <phase>` mode + `scripts/overlay-demo-shots.sh` render staged states for design
  checks without a game. First pass looked right except a wrapping chip and a crowded header; fixed.

## Engine v2 follow-up — 2026-09-05

The entries above describe the earlier prototype, not the current planner. The
user confirmed a general all-champion/role tool and direct, hand-holding purchase
recommendations; learning comes from repetition, with deeper explanation optional.

- Replaced the WR threshold and ordered slot swaps with aggregate-backed,
  compositional candidate scoring and exact inventory-aware shop quotes.
- Removed preferred pack builds from planner defaults; added real mage, tank,
  fighter, support, and jungle aggregates alongside Xayah regression coverage.
- Hardened source/session freshness, asynchronous refresh/import handling, actual
  rune/spell constraints, and nonstandard champion/mode boundaries.
- Added compact action-first controls, bounded local recaps, offline replay,
  cross-role scenarios, browser checks, and headless runtime tests.
- Kept old deployments untouched; created a separate Windows candidate. No live
  account writes or game interventions were part of this verification.

Exact checks, measurements, and unverified client behavior are recorded in
[engine-v2.md](engine-v2.md). The earlier >40% baseline-deviation target is retired:
deviation is measured descriptively, not treated as evidence of better decisions.

## 2026-09-06 (handoff: the Irelia Jungle blank panel)
- The first real Swiftplay game on the engine v2 build assigned Irelia Jungle. op.gg has no Irelia
  Jungle sample; `aggregate::load` refused any other role, the panel said "waiting for build data"
  and the player quit at 1:22. Reproduced offline from the recorded capture (18 snapshots) and from
  a sanitized fixture (`m0/tests/fixtures/swiftplay_irelia_jungle_0120.json`, identifiers replaced).
- Fix: labelled same-champion fallback (most-played role, other roles if that fetch fails), assigned
  role kept for spells/starters/legality, Swiftplay shop rules (level 3, 1400 g, no Doran's,
  Guardian's sold) as an explicit mode, honest loading/failed/unsupported states, fallback labels on
  the panel, in Swiftplay preparation and in the item set. Research and verification in
  `docs/notes/swiftplay.md`. New canonical build/launch: `scripts/overlay-build.sh` and
  `scripts/overlay-run.sh` share one executable path (`target\swiftplay\release`).
- Verified offline only: 233 core, 35 runtime, 20 browser, 34 Python tests; replay of the capture 18/18
  legal; the built exe's headless probe for Irelia as Jungle returns the labelled Top build with
  Flash + Smite. No game was played and nothing on the account was touched.

## 2026-09-06 (first full Swiftplay game on the fallback build: Xayah ADC, 20:36, 6/5/7)
- Pre-queue preparation was right every time the player changed choices (Xayah ADC, Soraka
  Jungle with Smite via the labelled fallback, Soraka Mid, Shen in three roles, Morgana Support)
  and the in-game plan used exact ADC data: Yun Tal, Greaves, Navori, Mortal Reminder all bought
  as recommended. The decision journal holds 32 entries for the match.
- Defects found in the log and fixed in the decision layer (`overlay/core/tests/swiftplay_game_regressions.rs`):
  a Quicksilver Sash offered twice as "magic protection" because any affordable finished item got
  the finish-now bonus (now only planned items or real detours with a situational score of at least
  1.5, and cleanse items never count as magic defense); Stormrazor displacing Infinity Edge through
  the shared B. F. Sword and Cloak (the player bought it and IE was never completed; the target now
  stays IE with an honest 178 g saving gap since the six slots were full); a Randuin's Omen with a
  0.3% pick rate shown for six seconds (late items need a 2% pick rate); "anti-heal for Naafiri"
  where Yuumi was the healer (the reason names the strongest healing source); a Health Potion at the
  level-one instant of a Swiftplay start (the opening branch is classic-only).
- Header chip: WebView2 stacked the state dot above the label; the dot is now a pseudo-element of
  one inline chip and the pre-queue state reads "lobby".
- Follow-up: the stacked chip was not a WebView2 quirk. The chip carried the phase name as a class and
  `swiftplay` is also the body section's column-layout class, so in the lobby the chip became a column.
  Phase classes are now `phase-*`, the dot is gone, and the tag is an outlined uppercase label coloured by
  phase (gold in the lobby and champ select, teal in game, amber without a client). A browser test checks
  the tag's box against the header in every phase; demo and live screenshots confirmed the fix.

## 2026-09-06 (renamed to Recall)
- Everything but the WSL checkout path says Recall. `core/brand.rs` owns the account-facing name and
  still recognises `Featherstorm ...` rune pages and item sets as ours. Verified live: the first start of
  `recall.exe` adopted `%LOCALAPPDATA%\Featherstorm` as `Recall`, and the two prepared Swiftplay pages were
  renamed in place (same page ids) to "Recall Xayah ADC" and "Recall Morgana Support".
- Repo renamed to `matteso1/recall` (private). MIT license, player-facing README, macOS support is issue #1.
- Commit history on both branches was rewritten to remove AI trailers and session links.

## 2026-09-06 (second Swiftplay game: Wukong Jungle, 15:40, surrendered)
- 75 recorded states, all legal on replay. Runes, Smite and the jungle data were right from the first
  poll. Three defects, each now a regression test (`overlay/core/tests/swiftplay_jungle_regressions.rs`):
  the level-three start pointed at a Trinity Force component instead of the jungle companion, because
  the previous fix had made the whole opening branch classic-only (Swiftplay now keeps only the role
  mechanics as starters and opens at level 3); the panel alternated between Black Cleaver and an
  affordable Executioner's Calling five times in 2.5 minutes as gold crossed 450 (a detour is now offered
  once, and buying anything else instead declines it for the game: `PlannerPreferences.offered_detour`,
  `declined_detours`, carried by the shell and by replay); the "(situational)" tag on Guardian Angel
  flickered around its 0.2 threshold (named needs keep 0.2, the generic label needs 0.75).
- A recorded Irelia state exposed a related bug: with one companion owned, the opening branch pointed
  at a second, blocked one. One companion now satisfies the group.
