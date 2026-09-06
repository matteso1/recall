# Recall — design

Working name: Recall. Owner: matteso. Platform: Windows / League of Legends.
Revision: action-first, universal build engine, September 2026.

## 1. The problem

A beginner reaches the shop and does not know what to buy. A popular build is a
useful starting point, but it does not account for this inventory, affordable
upgrades, the visible enemy items, or an explicit preference for more protection.

Recall supplies one best-supported next purchase and a short reason.
Learning happens through repeated, concrete decisions. It is not a quiz,
self-reflection exercise, or a replacement for practicing mechanics.

## 2. Goals

1. One compact panel: a purchasable component or completed item, exact remaining
   price, one short reason, and the next legal ability point when supported.
2. A coherent path of up to six inventory items **including boots**, with starters
   shown separately. Respect purchases and never imply automatic sales.
3. Any champion and role use their own aggregate loadout and shared planning
   algorithms. No Xayah-only decision rules or hidden preferred build files.
4. Adapt when visible evidence and purchase timing justify it, not to meet a
   quota of differences from the popular build.
5. Import runes, spells, and an ordered item set during allowed client phases.
6. Keep explanations optional, state honest, local computations inexpensive,
   and gameplay decisions under the player's control.

## 3. Non-goals for this implementation

No hidden-state inference, enemy cooldown/ult tracking, positioning/combat orders,
automated purchases or movement, rank scouting, performance grades, or LLM.
No promise to solve League's complete strategic decision space from this API.
ARAM/Arena need mode-specific data and rules and are paused, not approximated.

## 4. Product positioning

The differentiator is the combination of a concrete purchase, correct inventory
accounting, a short evidence-grounded explanation, and unobtrusive controls.
Earlier competitor comparisons were exploratory research, not verified current
feature claims. Product scope should be driven by player needs and tests, not
claims that another app cannot adapt builds.

## 5. User experience

### 5.1 Champion select

Read the identified champion, assigned role, current spells, and visible rosters.
Prepare the champion/role's aggregate loadout. Auto-import switches are independent;
buttons retry manually. Keep Flash on its existing key and respect observed manual
spell edits. Reuse an editable Recall rune page without deleting it first.

A matchup note is either curated factual context or an explicitly labeled
aggregate matchup record. Counters are whole-game outcomes, not lane win rates or
conditional item-value evidence. An ambiguous lane opponent is not guessed from a
contradictory trait tag.

### 5.2 Loading and short champion selects

Draft shows the last supported loadout. Swiftplay (verified queue 480) instead prepares
both champion/role choices in the lobby, before queueing. Read `localMember.playerSlots`
and update the local player-slots array, preserving champion, role, skin, unknown fields,
and each choice's Flash key. Runes stay attached to each choice; role-specific item-set
titles/UIDs prevent collisions when the same champion occupies both choices.

Show one readiness row per choice. Successful reads back from the client, not merely
completed requests, establish saved status. Unknown/malformed choices fail closed;
manual edits are kept, including when the other choice changes. Stale observations
expire readiness after six seconds. Imports pause outside Lobby and uncertain queue
identity cannot trigger ordinary draft imports. Fresh reads reject detected concurrent
edits; LCU supplies no compare-and-swap primitive for the final HTTP race.

The assigned live champion/role is authoritative. A prequeue role is only a fallback
when that champion has one unambiguous chosen role. Enemies become known in game;
prequeue preparation does not invent a matchup. Launching after assignment cannot
repair missed runes/spells, and the shop may cache its initial item set.

### 5.3 In game

The collapsed information hierarchy is:

1. **Recommended shop buy**: component/item name and its actual current price; or
   **Save for** with the gold still needed.
2. One reason, derived from the action actually selected.
3. Small planned-item strip and the next supported skill point.
4. **Why & options**, closed by default.

Expanded details show the reusable principle, tradeoff, closest alternative,
components, source uncertainty, and optional **More protection**, target pin, and
**Auto** controls. No prompt forces the beginner to reason out the answer first.

Do not say “recall now”: these inputs cannot establish a safe recall. Gold available
does not mean the player is at a shop. “Recommended shop buy” is deliberate.

No fresh player identity/advancing live data for six seconds means purchase and
skill advice pause, including when the UI event stream itself stalls. Unknown
prices never render as zero-gold buys. Explicit read-only demos are labeled separately.

### 5.4 After the game and settings

Show up to eight recent decision records with optional Useful / Not useful feedback.
Observed purchases are not treated as mistakes, recommendation compliance, or proof
of performance. The local journal is bounded and stores no player identifiers.

Panel position and independent auto-import switches are in local settings; region
and tier are explicit source parameters. Balanced/protection and target pins are
match-scoped. A full settings dashboard, adjustable opacity, and champion prefetch
are not implemented.

## 6. The build engine

### 6.1 Evidence and data boundaries

- op.gg champion/role aggregates provide popularity, games/wins, rune pages,
  spells, core lines, starters, boots, skills, late items, and single-opponent
  counters. They contain no per-match rows, timing, or full-comp conditioning.
- Data Dragon provides current-patch names, numeric stats, prices, recipes, tags,
  restrictions, and narrowly parsed effects. Unknown description mechanics stay
  unknown. Published item groups are used where membership is available; tested,
  explicit family rules cover known gaps, not arbitrary preferred builds.
- Champion traits are weak, manually reviewed priors, not an authoritative damage
  simulator. Typed, verified cleanse interactions replace the broad
  “lockdown ult implies QSS” rule. Schema checks catch invalid data; reviewed
  scenarios catch known semantic mistakes, not every incorrect tag automatically.
- Xayah's pack retains factual matchup notes and labels only. Legacy preferred
  item/rune/spell fields do not drive the planner. Without compatible aggregate
  data, pause; never borrow a different champion or role.

Aggregate requests are bounded, validated before cache promotion, and cached six
hours. Keep provenance and patch snapshots; never add overlapping refreshes or
tiers to manufacture sample size. Stale fallback is labeled.

### 6.2 Objective and candidate comparison

This is an explicit **purchase-utility heuristic**, not expected win probability:

```text
score = popular-order prior + completion / existing investment
        + useful situational coverage - delay / opportunity cost
```

Legality comes before score: correct map/store/champion/loadout, item-family
exclusions, actual inventory, recipe consumption, boots, and slot capacity.
The same constraints apply to a player-selected target. Free quest upgrades are
real purchases; transformed owned items are not new targets.

Candidates come from that champion/role's observed core lines, finished late items,
boots, and relevant recipe components. Preserve completed/quest inventory, filter
incompatible candidates, and make a bounded deterministic comparison. Early
components do not count as completed alternate first items. Full builds do not
start a seventh item; legal quest transformations still work with six occupied slots.

Scores combine observed resistance needs, visible healing items plus weak healing
priors, equipment/level-weighted physical and magical pressure, and dive/poke
context. Existing coverage reduces duplicated investment. Ally anti-heal is
uncertain coverage, never proof every target is covered. Offensive candidates must
fit the champion's aggregate-derived itemization archetype.

Weights live as documented Rust policy constants. Completion can beat a modest
situational preference. No binary AD/AP majority vote, “two tank tags means move a
slot,” or KDA-based Navori rule remains. Final-decision explanations cannot describe
an earlier rule that another rule later undid.

The observed most-picked core remains the baseline. Wilson intervals and a
Newcombe difference interval describe uncertainty; neither identifies causation.
The Xayah fixture's alternate-minus-popular interval includes zero. No automatic
winner search across many lines, undocumented previous-patch shrinkage, or fitted
model is claimed.

### 6.3 Purchase planning and live input

Every two seconds, summarize own gold, inventory, abilities, actual rune/spell
loadout, and visible players/items/levels. Own equipped value versus a plausible
lane opponent can describe **visible equipment difference**, never hidden wallets,
earned gold, or a causal “behind” state.

Recursively allocate owned recipe items exactly once, including repeated parts and
verified transformations. Quote the remaining price, simulate component purchases
and freed slots, and prefer feasible completed upgrades/useful stat gain with
deterministic ties. Return one primary purchase, a small legal basket, or the
cheapest feasible saving step. Do not infer passive value that was not parsed.

This is a greedy bounded shop optimizer. The hard problem is scoring long-term
power, not enumerating all possible six-item permutations. Exact combat simulation,
time-to-recall prediction, and learned comp-conditioned item values need data not
available here.

### 6.4 Local models

No model runtime is shipped. Current templates are faster, deterministic, and
faithful to decision evidence. A future statistical model would need timestamped
match decisions, legal candidate actions, calibration, patch/time-separated
evaluation, and safeguards against selection/survivorship bias. An LLM may
eventually phrase a verified reason, never select the item or invent gameplay facts.

## 7. Architecture and storage

```text
LCU observations ───────┐
Live observations ─────┼─> session/freshness guards ─> pure Rust planner ─> compact UI
validated aggregates ──┤                                │
patch item catalog ────┘                                └─> bounded local journal
```

LCU/live loops remain independent of remote fetching. Aggregate results carry an
exact champion/role/source generation; stale tasks cannot replace a new request.
Imports are validated again before writes and acknowledgments. Local preferences
recompute immediately without a remote request or a gameplay action.

The journal keeps at most 20 sessions, 80 significant decisions and 160 observed
purchases per session, with a two-megabyte serialized bound. Mid-game inventory is
a starting observation, not a list of purchases. Writes are asynchronous/coalesced;
corrupt prior files are preserved and errors surfaced. Abrupt process termination
can still lose an unflushed tail.

Tauri uses the Windows system webview. Borderless/windowed use is the supported
deployment target. Memory/FPS targets require measurement on the actual gaming
machine; architecture choice alone does not prove them.

## 8. Riot compliance boundary

Only LCU and Live Client Data supply game-session observations. Public catalogs
and aggregates supply general knowledge. Do not read memory, inject code, modify
game files, infer unseen positions, obtain enemy wallets/cooldowns, or automate
gameplay. A clear recommendation with a reason remains optional; short wording is
not itself an exemption from policy.

Riot prohibits previously unknown session information, unfair advantages, and
products that remove player decisions. Registration/review requirements still
apply to products using undocumented APIs. Recall is an independent local
prototype, **not certified or endorsed**. Recheck current policy and obtain the
necessary review before distributing or expanding the scope.
See [Riot's policies](https://developer.riotgames.com/docs/lol/).

Match-v5 training data is future work and would require appropriate API access and
data handling. It does not authorize expanding live inputs beyond this boundary.

## 9. Implementation stages

- Historical M0 proved LCU/live connectivity and item-set import using Xayah.
- The initial overlay moved base loadouts to aggregates across the roster.
- This revision adds shared adaptive scoring, shop correctness, source/session
  guards, action-first UI, player controls, local recaps, and offline evaluation.
- Follow-up work should come from reviewed real-game failures and data coverage.
  Mode-specific support, nonstandard ability leveling, and trained models are
  separate validated extensions, not promises bundled into this revision.

## 10. Remaining limitations

The op.gg endpoint is unofficial/unlicensed; caching does not settle licensing.
Data Dragon description parsing and fallback exclusivity families are incomplete
and patch-sensitive. Static champion traits cannot estimate actual damage dealt.

The shared skill helper is disabled for Aphelios/Udyr and transformation/early-ult
cases Jayce/Elise/Nidalee/Karma until rank mechanics are modeled. Item advice still
works. Special item mechanics beyond the represented catalog must fail
conservatively; cross-champion fixtures do not prove exhaustive roster coverage.

Replay on historical snapshots cannot identify the outcome of a different build.
Recorded equipment prices omit consumables spent, sales, upgrades, unspent gold,
and timing. User feedback is subjective evidence, not a win label. Fullscreen
exclusive behavior, live import races, and Windows resource impact still need
controlled in-client checks.

## 11. Success criteria and evaluation

- At the shop, a beginner can identify one legal, affordable purchase—or the exact
  amount still needed—without reading an essay.
- Prices match independent fixture arithmetic; no invalid component purchases,
  seventh-item suggestions, stale-state actions, or wrong-champion defaults.
- Meaningful adaptations have a short, traceable reason and do not arise merely
  from noisy gold ticks or a rule-order accident.
- Measure baseline deviation descriptively, **not a >40% target to optimize**.
  Improvement means better reviewed decisions, fewer wrong recommendations, and
  stronger legal/data coverage.
- Keep planner latency comfortably inside the two-second observation interval;
  measure actual Windows RAM/FPS separately.
- Offline gates: scenario tests, real-aggregate role fixtures, captured-state
  legality replay, headless runtime transitions, browser interaction tests,
  independent code review, and a Windows build.
- Policy compliance is an ongoing review requirement, not a zero-risk claim.
