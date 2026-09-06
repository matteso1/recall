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
