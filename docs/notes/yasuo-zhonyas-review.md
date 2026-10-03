# Zhonya's Hourglass on Yasuo, ahead of boots — October 2, 2026

The owner played Yasuo mid in a normal draft game (26 minutes, 3/11/2, a loss) and reported that the
panel told him to build "hourglass before boots, then it switched". This note records what the
recording showed, the causes, and what changed. The recording itself stays outside Git.

## What the panel did

- From 0:00 the path ended in Zhonya's Hourglass: Stormrazor, Berserker's Greaves, Immortal
  Shieldbow, Infinity Edge, Death's Dance, Zhonya's Hourglass.
- At 12:15, with Stormrazor finished and two deaths to Yone, Zhonya's moved to second place, ahead
  of boots: "Zhonya's: Yone (3/3) killed you; stasis stops the all-in".
- Between 15:33 and 16:47 the target went Zhonya's, Greaves, Zhonya's, Greaves, Zhonya's. It was
  Greaves whenever the gold covered them and Zhonya's otherwise, including "buy Cloth Armor" seconds
  after the player had sold one.
- The player followed it: about 2,850 gold of Zhonya's components (two Amplifying Tomes, Seeker's
  Armguard, Needlessly Large Rod) and one finished damage item at the end of the game.

No Master+ Yasuo in the corpus builds Zhonya's Hourglass (0 of 395 games).

## Causes

1. **The role backoff ignored what the champion builds.** `nextprior.rs` smoothed a champion's
   build-step table toward the role's with 100 pseudo-games. Yasuo has 10.6 weighted games at the
   fifth item, so that step was 90% the mid-lane table, which is mostly mages: Zhonya's 21%,
   Rabadon's 18%, Void Staff 17%. The chain had no filter for the champion's damage family (the
   provider pool has one), so Zhonya's passed. Lux got Guardian Angel the same way. Across the
   held-out players, 11.3% of the model's probability sat on items the champion never builds, and
   its first choice was such an item in 4.3% of decisions.
2. **The defensive exception was a promotion, not a preference.** Commit `b79aad1` (the same
   morning) re-enabled `promote_answer` for v3 after two direct deaths to a stronger enemy. It moves
   the best-scoring defensive item on the path to the front of what is left to buy, whatever the
   champion and whatever the step: ahead of boots, and ahead of Shieldbow, which is Yasuo's own
   answer to burst.
3. **The promoted item and boots alternated as the target.** The promotion outranked boots unless
   boots could be finished at once, so the target followed the purse.
4. **Sold boots counted as hidden boots.** `roleslot.rs` treats boots that vanish without a 70%
   refund as moved to the bot-lane quest slot. The mid laner sold Boots while other gold arrived
   (+381), the tracker kept them, and Berserker's Greaves were quoted 300 gold too cheap.

The existing checks could not see any of this: legality, identical-state stability and agreement
with common purchases all passed. The audit used here asks three other questions of every replayed
plan: is any unowned item one the champion's Master+ players never build, is a defensive item
placed ahead of unfinished boots, and does the target go A, B, A within five minutes.

## Changes

- **The model proposes only what the champion builds** (`nextprior.rs`). The role tables are a prior
  only among items the champion's Master+ players complete at some step. An owned set the
  champion's table lacks is smoothed through the sets one item smaller as well as the build step,
  so a skipped core item is still due (Yasuo with Stormrazor, Shieldbow and Death's Dance: Infinity
  Edge next, where the step table alone said Guardian Angel). In the step table, an item never built
  together with anything owned counts 0.3 (two families of builds on one champion). `A_NTH` 100 -> 50.
- **The first legendary matches the provider's build family** (`prior_chain`), because the runes
  and skill order come from the provider: Master+ Katarina opens Lich Bane 48% and Kraken Slayer
  40%, and the provider's page is the on-hit one. After the first item, a candidate must have been
  built with something owned or planned.
- **A repeated threat is a bounded preference inside the learned sequence** (`THREAT_BONUS`, 2 nats,
  about seven times the usual share), for one item with resistance against the enemy's damage type,
  stasis, or a spell shield against a mage. It needs one finished legendary, and it ends when such
  an item is owned. It moves an answer forward only where the champion's players plausibly build it:
  Death's Dance third on Yasuo (15% against Infinity Edge's 58%), Zhonya's third on Lux (10%) but
  not second (4% against 53%), Mercurial Scimitar fifth on Xayah and never second. The answer keeps
  its place for three minutes after the evidence lapses and while the player owns a component only
  it explains. `promote_answer` is used by the provider fallback only.
- **Boots come before any answer.** The fallback's promotion goes behind unfinished boots that are
  next, and neither it nor an anti-heal or cleanse component is offered while they are.
- **The learned sequence applies in Swiftplay too.** The legendary shop is the same, and the
  fallback there had kept promoting defense after every death (Banshee's Veil and Zhonya's second
  and third on LeBlanc on October 2, Jak'Sho and Randuin's before Guinsoo's on Kalista).
- **The boots slot is the bot lane's.** In another known role a vanish is a sale.
- Visible armor keeps its effect with the sharper champion counts (`V3_VISIBLE_NUDGE` 0.4): Lord
  Dominik's third against Rammus at 180 armor from items, as before.

Master+ players do not buy defensively after deaths (-1.1 points). The threat bonus is therefore a
policy for a player who keeps dying to one enemy, not a measured effect, and its size is a choice.
What the data does bound is where the answer may go.

## Validation

Next-legendary model, held out by player, on the half of the held-out players the constants were
not chosen on (`tools/priors/eval_next.py report`, 24,013 decisions):

| | Top-1 | Top-3 | Log loss |
|---|---:|---:|---:|
| Before | 53.4% | 79.1% | 1.705 |
| After | 55.0% | 81.5% | 1.524 |

Owned sets the champion's table lacks: top-1 21.0% -> 30.9%. Fourth items 34.8% -> 41.7%, fifth
items 16.8% -> 26.9%.

Whole planner on the held-out ranked timelines (`tools/priors/backtest.py`, validation split,
3,924 games, 88,959 frames, same prepared cases for both binaries):

| | Before | After |
|---|---:|---:|
| Target is the next completed item, at the purchase | 50.81% | 52.80% |
| First unowned legendary on the path is the next one | 59.30% | 61.76% |
| The next legendary is within three path slots | 66.53% | 71.55% |
| Planned boots are the first boots bought | 67.41% | 67.41% |
| Target changes with the same inventory, per transition | 2.31% | 2.42% |
| ... boots become or stop being the target | 0.65% | 0.39% |
| ... an anti-heal or cleanse component does | 0.61% | 0.64% |
| ... gold now finishes the new target | 1.01% | 1.32% |
| ... two finished items trade places, nothing bought | 0.03% | 0.06% |
| Identical state replanned: target or path differs | 0 | 0 |
| Invalid recommendations | 0 | 0 |

Every role improves on the first three rows. Frames offering an affordable anti-heal or cleanse
component fall from 802 to 629. The benchmark now reports the four kinds of target change
(`flips_boots`, `flips_detour`, `flips_finishable`, `flips_order`).

All 28 recorded games, replayed with both binaries (inventories are the recorded ones, bought under
the old advice):

| | Before | After |
|---|---:|---:|
| Games with an item the champion's Master+ players never build | 3 | 0 |
| Defensive item placed ahead of unfinished boots | 13 | 2 |
| Target went A, B, A within five minutes | 26 | 19 |
| Invalid recommendations | 0 | 0 |

The two remaining defensive-before-boots cases are Swiftplay replays where the recorded inventory
already held most of that item's components, so finishing it comes first.

In the Yasuo recording the path is now Stormrazor, Berserker's Greaves, Immortal Shieldbow, Death's
Dance (moved ahead of Infinity Edge: "Yone (3/3) keeps killing you; its armor cuts that damage"),
Infinity Edge, Guardian Angel.

## Limits

- The remaining target changes are mostly an anti-heal component offered while its price is in the
  purse and withdrawn when the player buys something else. It is offered once per finished item.
- The replays cannot show what the player would have bought under the new advice.
- The threat bonus and the visible-resistance step are policy constants. The corpus has no kill
  feed, so the benchmark exercises the model and the ordinary planner, not the exception.
- Swiftplay has no corpus of its own; it borrows ranked builds.
