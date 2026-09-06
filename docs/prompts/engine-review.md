# Featherstorm build engine: senior review and algorithm-design brief

You are reviewing the "brain" of Featherstorm, a League of Legends build overlay, as a senior
engineer with a strong algorithms / applied-statistics background who also understands League at a
high level. Treat this as an experiment in algorithm design: the question is not "does it work" (it
does, in real games) but "is what happens under the hood actually clever, and what would make it
clever?". Be blunt. Prefer proposals we can validate with data we can actually get.

If you have the repository, read these first, in this order:
`docs/design.md` (the design doc; sections 5.3, 6, 8, 11 matter most), `overlay/core/src/engine.rs`
(the rules), `overlay/core/src/aggregate.rs` (the data source), `data/pack/xayah.json` and
`data/pack/champion_traits.json` (hand-curated data), `overlay/core/src/live.rs` (what the live game
gives us), `m0/tests/fixtures/opgg_xayah_adc.json` (a real aggregate response), `docs/notes/m1-log.md`
(what happened in real games). If you do not have the repository, everything you need is below.

## What the product is

One on-screen panel during a League game that says what to buy next and why, adapted to the actual
lobby: enemy composition, lane matchup, and how the game is going. Runes, summoner spells and the
ordered item set are put into the client automatically at champion select. Riot compliance is a hard
constraint (design doc section 8): only the two local Riot APIs (LCU client API, Live Client Data API),
only information the player can already see (enemy champions, their visible items, scoreboard, game
time), no memory reading, no enemy cooldowns or ult timers, and everything is phrased as a
recommendation with a reason. Single user, runs locally on the player's Windows machine, Rust core
tested in WSL, panel updates every 2 s. Success criteria from the design doc: a player should look at
one spot and know what to click; the build should differ from the op.gg default in a meaningful share of
games (target over 40%) and the difference must be explainable in one line every time.

## What exists today

**Data sources**

1. op.gg's champion API (what op.gg's own site renders), per champion and position, current patch, one
   request per champion pick, cached 6 h. Fields: `summoner_spells` (pairs with play/win/pick_rate),
   `runes` (full pages: 4 primary + 2 secondary perk ids + 3 stat shards, with play/win/pick_rate),
   `core_items` (3-item lines with play/win/pick_rate; e.g. Xayah ADC: Yun Tal > Navori > IE 33% pick /
   57.2% win / 1542 games; Yun Tal > IE > Navori 14% / 60.1% / 639; Essence Reaver line 18% / 52.4%),
   `boots`, `starter_items`, `last_items` (single items seen in final builds with play/win/pick_rate,
   including components), `skills` (15-level orders with play/win), `skill_masteries` (max order),
   `summary.positions` (games and role rate per position), `counters` (per enemy champion: games and
   wins). Marginals only: no per-game rows, no conditioning on the full enemy comp, no item timing.
2. Data Dragon: items (cost, recipe, tags like `ArmorPenetration`, `Armor`, `Boots`, stats), champions
   (class tags), runes. Cached per patch.
3. The live game (Live Client Data API, polled every 2 s): my gold, items, level, ability levels, KDA,
   game time; every player's champion, visible items, scores; game mode. No enemy gold, no cooldowns.
4. Champion select (LCU): my champion, assigned position, my current spells, both teams' champions as
   they lock (in draft), bans. In Swiftplay/quickplay champ select lasts one second and enemies are
   unknown until the game starts.
5. A hand-curated pack for Xayah only (`data/pack/xayah.json`): 16 one-line matchup notes, item
   alternatives by role (anti_heal: Mortal Reminder, armor_pen: LDR, cleanse: Mercurial, defensive_ad:
   GA, defensive_ap: Maw, anti_burst: Shieldbow, sustain: Bloodthirster), short names, an offline
   fallback path. Plus `champion_traits.json`: 164 champions hand-tagged with `damage` (ad/ap/mixed),
   `roles`, and booleans `healing`, `shielding`, `tank`, `lockdown_ult`, `assassin`, `burst`, `poke`.
   A real game already exposed one wrong tag (Vi as tank) that flipped a rule.

**The engine** (`engine::plan`, pure function, ~700 lines, 36 unit tests on real fixtures)

Base build = the aggregate: starters, core line, boots, skill order, rune page, spells. The core line
is the most-picked one unless another line has at least 10% pick rate, 500 games and 2 points more win
rate (thresholds are ad hoc; both numbers are shown in the why line). Path = core (3) + boots, then the
pack's non-damage slots (armor pen, defensive) to six items, else the most popular finished items with
role inferred from Data Dragon tags (never a component, never an alternative first item, at most one
armor-pen item). Every path item has a role: damage | boots | armor_pen | defensive.

Then explicit, ordered rules, each pushing one "why" line when it changes something:

1. Lane matchup from the pack: line, optional first item / start / spells (Ashe -> Cleanse).
2. Anti-heal: an enemy tagged `healing` swaps the armor-pen slot to the anti-heal item.
3. Two or more `tank` enemies: armor pen one slot earlier.
4. A `lockdown_ult` enemy: the defensive slot becomes the cleanse item (Mercurial).
5. Mostly magic damage among enemy carries (AP count > AD count; tanks and pure supports excluded):
   Maw instead of GA.
6. Two or more `poke` enemies and no assassins: Bloodthirster instead of GA.
7. Live: an enemy holding two or more items with the `Armor` tag costing 900+: armor pen earlier.
8. Live: behind (3+ deaths and at most 1 kill): Navori before IE (a cheaper spike; Xayah-specific).
9. Two `assassin` enemies, or one assassin with 3+ kills and more kills than deaths: defensive item
   moved to slot 4.
10. Magical Footwear on the rune page: boots slot tagged, NEXT skips boots until the free ones arrive,
    the footwear counts as the Boots component.
11. Core order as above (win rate vs pick rate).
12. Matchup line from counters when the pack has none (50+ games).

NEXT = first unowned path item; its components in recipe order with owned ones ticked; "buy now" = the
whole item if affordable, else the first affordable unowned component, else the first unowned one.
Skill point = most-picked 15-level order completed with the max priority, R at 6/11/16.

**What we can measure today**: our own games (raw Live Client snapshots every 2 s and champ select
sessions are captured to JSON), the aggregate marginals, and the panel's log of every path change with
its reason. We have no Riot API key, so no Match-v5 match histories or timelines yet (a personal key is
easy to get for development; a production key is a process).

## What I want from you

Write for an engineer who will implement this in Rust with tests. Organize the answer as:

1. **Critique.** What in the current design is genuinely smart, what is naive, and what is wrong.
   Include the statistics: the core-line thresholds, the AD/AP majority vote, the "behind" heuristic, the
   armor-stacking detector, the trait booleans. Point out interactions between rules (ordering effects,
   double counting, rules that can undo each other) and any place where a wrong hand-tag silently
   changes the build.

2. **Formulate the problem properly.** What are we actually optimizing when we say "best next item"?
   Propose an objective (or a small family of them) that is computable from the data above: e.g.
   expected win probability given comp and state, gold-efficient power at the next recall, time-to-spike.
   State the decision variables (which six items, in which order, which component now), the constraints
   (one boots, exclusive item groups, gold on hand, recall timing), and the information available at each
   decision time (pre-game marginals; in-game visible state). Discuss the search space honestly: with
   roughly 50 relevant legendaries per role, six slots, ordering and exclusivity constraints, is search
   even the hard part, or is the scoring function? Where do the NP-hard formulations (subset selection with
   interaction terms, ordering under a budget) actually bite at this scale, and where does a greedy or
   beam search with a good objective simply win?

3. **Proposals, ranked by expected value per unit of effort.** For each: the idea, the algorithm or
   model, the data it needs (and whether we have it), complexity and latency (the panel recomputes every
   2 s on a gaming PC while the game runs), how it stays explainable in one line, how we would validate it
   (metric, data, what "better" means), and the effort. Candidates to consider, extend or reject:
   - Replacing slot-swapping rules with a compositional scoring model over item traits (Grievous Wounds,
     armor pen, MR, anti-burst, cleanse, sustain, mobility) derived from Data Dragon tags/descriptions, so
     rules generalize to all champions and roles instead of one pack per champion.
   - Conditioning on the enemy comp with the data we have: counters are per single champion; can we
     combine them sensibly (independence assumptions, shrinkage), and what would we need from Match-v5 to
     do it properly?
   - Proper statistics for "line A beats line B": Wilson/Bayesian comparisons, shrinkage toward the
     previous patch when a new patch has few games, guarding against multiple comparisons.
   - Live state: estimating enemy gold from visible items (item costs), lane gold differential as the
     "behind/ahead" signal instead of KDA thresholds, detecting enemy armor/MR stacking from actual item
     stats rather than tags, and reacting to the enemy carry's power curve.
   - Component ordering and recall timing: which component to buy now given gold, expected income and
     the next spike, rather than recipe order.
   - Rune and spell adaptation conditional on matchup (Cut Down vs tanks, Cleanse vs lockdown), with the
     data limits stated.
   - Data quality: validating or deriving `champion_traits.json` from data instead of hand tags; tests
     that catch a wrong tag.
   - An evaluation harness: backtesting recommendations against real games (ours now, Match-v5 timelines
     later), agreement with high-win-rate builds conditional on comp, and a "golden set" of lobbies with
     reviewed expected outputs.

4. **The local-model question.** The owner is wondering whether a local AI model under the hood could
   inform gameplay. Separate the cases: (a) a small statistical/ML model (win-probability or item-value
   model trained on match data) versus (b) a local LLM (e.g. via Ollama) for reasoning or for writing the
   one-line "why" (the design doc explicitly says a model may write the sentence but never pick the
   item). For each: what it would add over the rules + aggregate, what data and compute it needs, latency
   and footprint on a gaming PC, failure modes, and whether it is worth it at this stage. Say plainly if
   it is excessive.

5. **What not to do.** The over-engineering traps for a one-person project at this stage, and the two
   or three things you would do first next week with only the data we have.

6. **Code-level notes** if you read the repository: correctness issues, edge cases (Practice Tool,
   Swiftplay's one-second champ select, ARAM, a champion the aggregate has never seen, mid-game start),
   and anything in the Rust that will bite later.

Constraints to respect in every proposal: Riot compliance as stated above; every recommendation must
carry a one-line reason a player can read mid-fight; the panel is the size of a phone screen; no
hand-tuned defaults hiding in data files (the owner has rejected that; data or rules only).
