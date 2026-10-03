# Engine v3: one value model, learned from real matches

Written 2026-09-27 after the Sivir game (21 min, five deaths to Kalista): the panel promoted Guardian
Angel as Sivir's second item and the player's boots and Daggers left the inventory. Tonight's fixes
(resist balance, lane weighting, kill-feed blame, promotion hold, commitment) each patched the last
game. This note is the plan to stop doing that.

## Why the current engine keeps needing patches

`decision.rs` scores an item as op.gg order prior + hand-weighted need terms (anti-heal 3.4x, detour at
1.5, promotion at 1.0, hold 180 s, commitment margin 1.0, ...). The terms are in different units, so
every new situation needs a new threshold, and each threshold is tuned on one recorded game. Nothing in
it can say "Guardian Angel adds less to Sivir's fights than Navori does for the same gold", which is the
question a strong player actually answers.

## Architecture

1. **Candidate set and priors from real matches.** Riot's Match-v5 API (a free personal key) returns
   full timelines of ranked games: every purchase with its time, both compositions, gold, levels, kills
   and the result. Mining high-elo games per patch gives, for each champion and role, what strong players
   buy next in states like the current one: enemy damage mix, tanks, healers, who is fed, gold lead or
   deficit, items owned. op.gg stays the fallback for champions with too little data.
2. **One currency: fight power.** For each candidate, compute the change in the fights you will take,
   against the enemies you actually fight (lane opponent early, then the fed and dangerous ones). That's
   your damage per second against their current armor, magic resist and health, and your effective health
   against their damage mix. The inputs are item and champion stats from Data Dragon and CommunityDragon
   plus the live scoreboard. Divide by remaining gold, with owned components counted. Armor versus magic
   resist, penetration versus tanks, and damage versus defence all fall out of one formula. It replaces the
   separate rules.
3. **A planner over sequences, not single picks.** Beam search over the next two or three purchases,
   maximizing fight power over the gold the player will have, pulled toward what high-elo players buy in
   similar states. Stability comes from comparing whole sequences, so there are no per-poll thresholds.
4. **Judge it on thousands of games, not ours.** Backtest every change on held-out high-elo timelines. At
   each purchase, does the recommendation match what strong players bought in that state, and do its
   choices associate with winning (associational only, stated as such)? Also measure stability and
   legality. Our recorded games stay as tests for panel bugs, not as the measure of correctness.

## Order

1. Fight-power model and sequence planner on the current data (no key needed). Backtest on our recorded
   games plus the existing fixtures, with explicit before/after output.
2. Match-v5 ingestion with the user's personal key: per-patch, per-champion state-conditioned purchase
   statistics, cached locally.
3. Backtest harness on the mined corpus, then retire the hand-tuned need terms that the model covers.

Riot policy: this uses only the Match-v5 API and the local client APIs, the same sources public build
sites use. Recommendations stay recommendations.

## Built (2026-09-27)

Step 2 came first because a ready Master+ corpus existed (Kaggle, 107k player-games with per-minute inventories);
the Match-v5 key is no longer needed to start. What shipped, and what the research showed on the way
(`scratchpad` reports are not kept; the numbers are):

- **Backbone:** P(next legendary | champion, role, owned legendary set), smoothed through build step and role,
  times measured enemy-composition lifts (Mortal Reminder x2.1 against a healer, Lord Dominik's x1.4 against two
  tanks, Maw x3.0 for top laners against magic-heavy teams), chained greedily with a small hysteresis. Held out by
  player: next legendary top-1 50.6% / top-3 75.9%, against 47.4% / 71.2% for a static build order.
- **Fight power stays a nudge.** A stat-level damage x effective-health model ranked Guardian Angel and Shieldbow
  level with Navori for Sivir, while 0 of 67 Master+ Sivirs bought either second. The existing need terms add at
  most 0.6 nats on top of the data.
- **No defensive reaction to deaths** in 187k Master+ completions (-1.1 pp), so kill-feed promotions are off.
- **Role-slot boots** (`roleslot.rs`): the bot-lane quest hides boots from the Live Client, which is what made the
  Sivir panel show Greaves as unbought and let Guardian Angel through.

Next: a Match-v5 pilot for per-patch freshness and exact kill/damage events; per-champion lifts where data allows;
supports' quest items and Swiftplay still use op.gg.

October 2 update: the blanket disable above was too restrictive. The
[live adaptation follow-up](live-adaptation-fix.md) permits a bounded defensive exception after
repeated direct deaths to a stronger enemy, lets visible resistance use the full penetration
need score, and limits purchase-inferred detour suppression to the current build stage.
The original aggregate purchase result does not establish that every individual defensive
exception is wrong. These are explicit planner policies, not newly trained win predictions.

Later on October 2: that exception, as a promotion straight after the first item, put Zhonya's Hourglass on
Yasuo ahead of boots. It now acts inside the learned sequence, and the sequence itself no longer proposes
items the champion does not build. See the [Yasuo game review](yasuo-zhonyas-review.md).
