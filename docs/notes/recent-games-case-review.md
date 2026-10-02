# Recent-game case review — 2026-10-02

This follows the [repeated-advice review](repeated-advice-review.md). The latest saved
recording still starts October 1 at 10:18 PM Pacific; there are no games recorded after
the October 2 installation. Seven recent Xayah recordings are normal Draft Pick. The
intervening Bel'Veth recording is KIWI on map 12, outside the supported build planner.

Times below are game-clock observations. Scores are the last observed scoreboard, not
verified end-of-game results. Purchases are first observed inventory completions. The
recorder stores incremental kill events; the review joins events across the whole file.
It does not infer camera position, attack targets, damage taken or a counterfactual win.

## What happened in the games

### October 1, 10:18 PM — Xayah, Jinx/Nautilus lane

- Last observation 17:41, Xayah 0/3/3, level 8, 50 reported CS.
- Deaths: Nautilus at 2:42; Nautilus with Jinx at 14:05; Jinx with Nautilus at 16:32.
- Yun Tal first appears at 15:01 and upgraded Greaves at 16:35. The next target then
  becomes Infinity Edge. Recall was progressing through the opening purchases.
- This match did not reach the third/fourth-item choices. Repeated early core here is
  not enough evidence that another build would have been better. The app cannot claim
  that the late items on its displayed path would have solved the lane.

### October 1, 9:45 PM — Xayah, Rammus/Cassiopeia/Trundle

- Xayah was 5/1/6 at 15:16; the last observation is 5/7/10 at 27:08. The next six deaths
  involve Cassiopeia, Trundle or Rammus as killer. Enemy Jinx finishes the recording 2/11/15.
- At 21:36, Infinity Edge completes. Recall immediately selects Navori. Rammus already
  has 180 visible bonus armor from items; at 25:02 he has 265, and Navori remains first.
- The replay explicitly explains the LDR alternative with Rammus's armor. At 21:36,
  Navori scores 3.00, while LDR scores 2.40: 1.50 from path order plus the maximum 1.00
  situational contribution, less 0.098 for its higher cost. The armor signal is present
  but cannot beat the order preference at this step through that term alone.
- By the final snapshot, Navori components occupy the bag and add investment credit.
  Later switching is more costly; the useful comparison was before those purchases.
- At 12:54, Executioner's was offered for Trundle. A Pickaxe appears at 12:56, causing
  the planner to mark Executioner's declined for the rest of the game. The log proves
  a purchase, not that the player deliberately rejected a two-second recommendation.

### September 30, 10:20 PM — Xayah, 33/9/9, Miss Fortune/Swain lane

- Yun Tal 11:30; IE 19:20; Navori 21:58; LDR 25:20; Bloodthirster 30:10; GA 35:08.
- The LDR recommendation names Illaoi's 83 visible item armor at 21:58. This is an
  example of the engine providing a relevant contextual reason while following its core.
- The recorded panel stops recommending purchases at 30:10 after five legendaries and
  quest-slot boots. Doran's still fills the normal sixth slot then. After Doran's is
  removed at 35:06, Recall still offers nothing despite a genuinely free slot and 5,241g.
- This is the confirmed capacity defect fixed on October 2. The corrected replay blocks
  the future purchase until a slot is free, then offers GA. No automatic sale is assumed.
- Executioner's was also suppressed after a Pickaxe purchase two seconds after the
  anti-heal recommendation, at 15:12/15:14.

### September 30, 10:00 PM — Xayah, Ashe/Yuumi lane

- Last observation 14:00, Xayah 0/0/1. Yun Tal appears at 12:16 and Greaves at 13:54.
- IE becomes the next target. As in the newest short game, later build adaptation was
  never exercised by purchases. The similar first items alone do not establish a bug.

### September 29, 7:58 PM — Xayah, enemy Ahri 26/0/5

- Last observation 24:51, Xayah 3/11/3. Ahri kills Xayah six times: 14:05, 14:55, 19:32,
  20:50, 22:11 and 23:01. Ahri is already 13/0 at 15:01 and 18/0 at 20:11.
- Recall continues toward Greaves, then IE. The replay recognizes Ahri: at 15:01 its
  alternative is Mercury's Treads for Ahri's magic damage. The recorded main target is
  still Berserker's Greaves. The corpus boots model uses champion/role and enemy magic
  count, not which of those enemies is now far ahead.
- At 19:33 the alternative explanation names Ahri 18/0 and Bloodthirster's shield;
  the main target remains IE. Mercurial is in the option pool but is not in the purchase
  score trace at that moment. With only one completed core item, unstarted off-path
  finished items generally fail the automatic eligibility gate. Later Pickaxe ownership
  makes Mercurial eligible through recipe credit, but its score is still far below IE.
- At 18:39 the automatic detour is instead Executioner's for Aatrox. Cheap completed
  detours can win the affordability bonus while expensive protective alternatives are
  gated or constrained. The policies therefore respond unevenly to different needs.

The older September 29 Kog'Maw/Morgana game also ends with one legendary and IE progress
(2/8/2 at 18:44). The 1:25 recording does not provide meaningful mid-game evidence.

## Why this happened

1. **The learned objective is purchase prediction.** The shipped table conditions mostly
   on champion, role and owned legendaries, with three coarse composition flags. Its
   Xayah table has 497 games plus role-level backoff. Live equipment and kill-feed needs
   are hand-weighted additions; the model does not learn a fight-specific item value.
   Saving these recordings does not retrain the embedded model in the running app.
2. **The September 27 rewrite deliberately constrained adaptation.** Commit `4892ede`
   introduced the bounded model nudge and disabled automatic kill-feed answer promotions
   for covered standard-mode champions. That corrected overaggressive early defensive
   recommendations, but a population-wide observation about purchases after deaths does
   not establish that a particular fed threat should never justify an exception.
3. **Several rules reinforce the original order.** The model's live nudge is capped;
   next-target need is capped below the first/second path-order gap; finished off-path
   answers have a core-count gate; and new components increase investment credit. A small
   early preference can consequently remain the recommendation across very different games.
4. **The evaluation contract was too narrow for the product goal.** Legality, repeated-state
   stability and matching common purchases are useful checks. They do not establish good
   situational judgment. The all-magic regression explicitly checks that Mercurial's model
   probability rises; it does not require a changed recommendation. That test can pass
   while the complete displayed build stays the same. Strong opponent-equipment coverage
   is also scarce in the large Kaggle validation corpus.
5. **The hidden-boots fix was incomplete.** The earlier change restored ownership through
   virtual slot 9 while retaining the old six-commitment limit. Ownership and available
   inventory capacity needed separate contracts. The October 2 fix adds that missing case.

## Evidence boundaries and next work

All 957 observations with a recorded next target agree with the archived replay's target;
eight observations have no recorded panel yet. This supports the target timelines above.
Future full paths differ in 445 observations even after removing display tags: the replay
uses the available aggregate cache and starts without the original pregame planner memory.
The recorded panel is authoritative for what was emitted. Replay scores are a reconstruction,
not a preserved production score trace. Saving aggregate provenance and planner preferences
at session start would make this investigation more reproducible.

The next engine experiment should measure response to controlled armor, fed magic damage,
and repeat kill-feed evidence before the player invests in another item. It needs to protect
core progression without making the common order effectively mandatory. Compare the whole
planner on fixed full-team validation cases, keep eligibility and candidate coverage visible,
and evaluate temporal stability as well as purchase agreement. Automatic purchase-based
detour suppression should also be distinguished from an explicit player skip.

No scoring, installed app, or model-data changes were made in this follow-up review.
The confirmed slot/recap/explanation fixes from the previous review remain installed.
