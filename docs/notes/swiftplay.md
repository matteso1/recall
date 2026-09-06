# Swiftplay pre-queue loadouts

The previous runtime only watched `/lol-champ-select/v1/session`. In the observed
queue 480 lobby that endpoint returned 404, while two choices already existed in
`GET /lol-lobby/v2/lobby` at `localMember.playerSlots` (Xayah Bottom and Irelia Mid).

Read-only `/help?format=Full` confirmed that GET and PUT
`/lol-lobby/v1/lobby/members/localMember/player-slots` use a vector of
`LolLobbyQuickPlayPresetSlotDto`: a direct JSON array, not a wrapped object.
The fields are championId, skinId, positionPreference, perks (a JSON string),
spell1, and spell2. `/lol-gameflow/v1/session` also exposed `gameData.queue.id = 480`;
this allows queue identification after restart during assignment. No credentials,
party identities, or raw lobby payloads are included in fixtures or these notes.

The runtime prepares both role-specific public aggregates in one bounded background
worker and reports each choice independently. It validates runes against the patch
catalog, preserves each Flash key, and saves role-specific shop sets together without
discarding other sets. Fresh phase/selection checks precede writes and read-back
confirms status. Changed picks cancel old work; unchanged choice progress survives.
Subsequent manual loadout edits are kept. Failed preparations retry with backoff;
queued or stale observations cannot initiate imports. Real assignment is identified
from the live player, never from the first choice.

Offline regressions cover direct-array HTTP shape, independent choices, Flash keys,
unknown fields, semantic perk equality, role-distinct item-set IDs, changed selections,
manual edits, queue-phase rejection, read-back mismatch, preservation/concurrent edits
of foreign item sets, startup during unknown assignment, and browser readiness expiry.

Verified on this change: 221 core tests, 26 headless runtime tests, 19 browser tests,
and 34 existing Python tests passed. Core/runtime Clippy passed with warnings denied.
The UI pass kept both choice rows readable at 380 px and 320 px panel widths.
The Windows/MSVC release build also passed, with the executable under the mirror's
`overlay/target/swiftplay/release/featherstorm.exe`; the previous executable was not
replaced, and no running overlay was restarted.

Verification does not substitute for an observed real Swiftplay assignment on the
updated executable. The initial implementation's API inspection was read-only.
Runes/spells cannot be repaired after assignment.

## Linked rune-page synchronization fix

Live troubleshooting exposed two independent representations: the player's saved
slot perks and `/lol-perks/v1/pages`. A correct slot echo alone did not prove the
client's linked rune page had updated. Two temporary pages could have the identical
name while only one had `quickPlayChampionIds: [39]`; `current` identified the global
editor selection, not the selected Swiftplay loadout. The quick-play-selection
lookup also differed from the actual champion-linked page, so it is not used as
save confirmation.

A user-authorized, guarded PUT to the exclusively linked Irelia temporary page
renamed it to `Featherstorm Irelia Mid` (HTTP 201). Read-back confirmed the same page
ID, valid rune content and champion association, unchanged player slots, and no
changes to other pages. No page was deleted or created. This contract is now part
of the automatic importer: reuse the linked temporary/app-owned page in place,
preserve its temporary status and global selection, update all nine perks and both
styles, and confirm both the linked page and the saved slot before reporting Ready.

Preflight rejects personal/noneditable pages and ambiguous/shared associations;
missing links report an error instead of pretending an import succeeded. These
cases do not trigger custom-page creation or deletion to work around capacity.
Two roles for the same champion cannot safely be mapped to different pages through
this champion-only association, so rune imports pause for that case.

Page snapshots are retained across both API writes. Page-only manual changes are
kept across retries; harmless global editor selection changes are not treated as
rune edits. Fresh gameflow checks precede both mutation types. LCU offers no atomic
compare-and-swap, so a simultaneous edit in the final request window remains a
client API limitation. HTTP regressions cover stale page contents, missing/invalid
confirmation, idempotence, personal-page protection, manual edits before/during
synchronization, and queue starts during the last page read.

Verification for this follow-up: 225 core, 35 runtime, 19 browser and 34 Python
tests passed; core/runtime Clippy passed with warnings denied. Windows release
build passed. The updated executable was launched at the same Swiftplay path;
its SHA-256 is
`d31cc40ff4cecc5759c7b2edde6849e8d21c41733e9d8a071afe30eb1ca1d8b6`.
Fresh lobby reads confirmed Xayah's and Irelia's linked pages were valid and exactly
matched all nine saved perks/styles; spells were Flash/Barrier and Flash/Ignite
respectively. This is lobby verification, not a claim of observed in-game assignment.

## Assigned-role fallback and Swiftplay shop rules (2026-09-06)

### What failed

Queue 480 with Xayah Bottom and Irelia Mid prepared; the Irelia role was changed to Jungle before
queueing and the game assigned Irelia Jungle. The log shows a one-second champ select, then
`aggregate: champion 39: no aggregate games for champion 39 as Jungle` every 30 s while the panel said
it was waiting for Irelia's build data, until `client went away` at game time 1:22. `aggregate::load`
demanded exact-role data whenever a role was known and errored otherwise, so the planner had no
aggregate, produced an empty path and a "waiting" note, although the champion index (Irelia Top
83,637 games, Mid 53,921) and a cached Mid response were both on disk. That was a design decision,
not a slow request, and it was wrong for this product.

### Research (verified 2026-09-06, independent of the interrupted note)

- Recorded game on the patch 26.17 client (Data Dragon 16.17.1): mode `SWIFTPLAY`; level 3 and
  1400 gold at 0:01; a jungle companion (Mosstomper Seedling) and a support quest item (World Atlas)
  in inventories; no Doran's item in any of the ten inventories at 1:20; Guardian's Orb was seen in an
  enemy inventory in the previous Swiftplay game the same evening.
- Riot patch 26.1 notes (fetched): "Swiftplay and rotating game modes will not have this feature
  [Role Quests] enabled, and will retain the same jungle and support quest rules as before"; Crystalline
  Overgrowth starts at 30 s in Swiftplay. The page has no Swiftplay section about starting gold or the
  shop, so those facts rest on the recorded game and the wiki.
- League of Legends wiki, Swiftplay (community page, fetched): champions start at level 3 with 1400
  gold; Doran's Blade, Ring, Shield, Bow and Helm are disabled; Guardian's Blade, Hammer, Horn and Orb
  are enabled; jungle companions need 15 treats for the first evolution and 25 for the final one;
  support quest items start with three charges; two Priority roles; a solo player picks two roles, one
  of them Priority, and may pick the same champion twice unless it is among the most-picked champions;
  first surrender at 15:00.
- Data Dragon 16.17.1 flags Doran's items and Guardian's items alike as sold on maps 11 and 12, so
  the catalog cannot tell the two shops apart; the mode has to be an explicit input.
- Riot developer policies (fetched): third-party tools may use official static data and information
  visible to the player and may "give multiple choices to help players make good decisions"; they must
  not remove game decisions or automate play. A labelled recommendation from public aggregates through
  the LCU and Live Client APIs stays inside that.
- The reported penalty was not investigated beyond noting that leaving early is what LeaverBuster
  covers; its type and cause are unverified and cannot be diagnosed from here.

### What changed

- `aggregate::load`: exact role first. When the assigned role has zero games at this rank, the
  champion's most-played role is loaded instead, with `requested_position` set to the assignment; if
  that role cannot be fetched, the champion's other played roles are tried (an offline cache with only
  one role still serves). Nothing is relabelled and no other champion is ever used.
- `engine::plan_in_mode`: `Plan.position` is the assigned role, `Plan.source_position` the data's
  role when they differ, `Plan.source` ends with "(Top build)", and the first warning (the panel's
  note) reads "No Jungle data for Irelia at this rank; using the Top build as a starting point". Role
  rules follow the assigned role: a Jungle assignment gets Flash + Smite and the three jungle
  companions as starters (Doran's and other lane starters drop out as mutually exclusive), a Support
  assignment gets its quest item, a lane assigned from jungle-only data gets no spell pair (its saved
  spells are kept) and no companion. Lane-to-lane fallbacks reuse the lane spells.
- `engine::GameMode` and `shop::ShopContext.swiftplay`: Swiftplay removes Doran's items from
  starters, blocks Doran's purchases and allows Guardian's items; the classic shop blocks Guardian's
  items. Pre-queue Swiftplay preparation plans in Swiftplay mode; live games take the mode from the
  snapshot.
- Poller states while no purchasable path exists: "Loading X's build data…", "Build data for X is
  unavailable (…); retrying automatically", "No build data for X in any role at this rank yet", or the
  planner's own note when data exists but no legal path does.
- Swiftplay pre-queue: a fallback slot carries its note and the top line says
  "Prepared. Irelia JUNGLE: Top build (no JUNGLE data)"; a role without a data spell pair keeps the
  saved spells as `kept`, a settled state rather than an error. The existing page-safety guards are
  untouched; a missing safe linked page is still an error, not a page creation.
- Item set block titles "Start (Top build)" and "Full build: Top data, Jungle"; panel header
  "Irelia · Jungle (Top build)", a warning line above the action, Swiftplay slot "JUNGLE · Top build".
- Replay: `--aggregate-role` names the role a raw aggregate file was fetched for (cache records carry
  it in their request URL); a mismatching `--role` replays the labelled fallback instead of relabelling.

### Verified offline

- Core: 233 tests (166 lib, 16 replay, 23 planner regressions, 16 purchase context, 5 role
  fallback, 7 universal planner); Clippy with warnings denied. Headless runtime: 35 tests, Clippy
  clean. Browser: 20 tests, including the fallback labels. Python: 34.
- Replay of the recorded capture (18 snapshots, the patch 16.17.1 catalog and the app's own cached
  Mid record, `--champion Irelia --role jungle`): 18 of 18 targets legal, no violations, every plan
  labelled "Mid build" with Jungle kept; at 1:20 the recommendation is Vampiric Scepter (900 g)
  toward Blade of The Ruined King, affordable with the recorded 982 g; spells stay Flash + Smite.
- The shipped executable ran headless on this machine with no client:
  `scripts/overlay-probe.sh --champion Irelia --role jungle --swiftplay` → index Top/Mid only,
  fallback fetched the Top build (83,637 games), plan labelled "(Top build)", spells Flash + Smite,
  starters the three companions and a potion, path BotRK, Steelcaps, Hullbreaker, Wit's End, DD,
  Kraken, first target BotRK.

### Not observed

A real Swiftplay assignment on this executable, the client's behaviour when a role is changed after
preparation, the shop's rendering of the labelled blocks, and the account's penalty state. The exact
Windows artifact is `C:\Users\nilsm\code\featherstorm-win\overlay\target\swiftplay\release\featherstorm.exe`
(SHA-256 `753d61de3fb0d3efa0b64b0dcfd70a12526a4ba3b27fd1d24b8c4dd46487b734`, 10,660,352 bytes, rebuilt after the
2026-09-06 game fixes; the first game was played on `e0019d27…`), built by
`scripts/overlay-build.sh` and launched by `scripts/overlay-run.sh`, which now know only that path.
