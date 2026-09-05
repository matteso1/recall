# Smart Build Overlay for League of Legends — Design Doc
**Working name:** Featherstorm (placeholder)
**Author:** matteso
**Status:** Draft v0.1 — September 2026
**Platform:** Windows, League of Legends (PC)
---
## 1. The problem
Every current companion app (op.gg, Blitz, U.GG, Mobalytics, Porofessor) does the same thing: it looks up the highest-win-rate build for your champion across millions of games and imports it. That build is chosen *before the game* and never changes. It doesn't know the enemy has Soraka. It doesn't know you're into a triple-tank comp. It doesn't know you're behind. It shows the first 3 items and stops because the stats run thin after that.
The result: a Xayah player sees "Essence Reaver → Greaves → IE" every single game and has to alt-tab to Skill Capped mid-game to figure out whether to buy Mortal Reminder or Lord Dominik's. That defeats the point of an overlay.
**What players actually need at the shop is one answer:** "Buy this next. Here's why, in five words." And that answer should already account for who's on the other team.
## 2. Goals
1. **One on-screen panel** that shows the full build path (start → 6 items + boots), in order, adapted to the actual lobby. Not just three items.
2. **Enemy-comp aware.** Anti-heal if they have healers. Armor pen earlier if they have 2+ tanks. QSS if they have lockdown ults. Defensive item swaps against assassins or heavy poke.
3. **Lane-matchup aware.** The specific opponent in your lane changes first item, starting item, and sometimes summoner spells.
4. **Skill-order guidance.** Show the next ability to level based on current meta skill path, with a highlight when you level up.
5. **Import into the client** like op.gg does: runes, summoner spells, and the item set (so the ordered build also appears inside the in-game shop's "recommended" tab).
6. **Lightweight and Riot-compliant.** No Overwolf, no ads, no memory reading, under 100 MB RAM.
## 3. Non-goals (v1)
- Jungle timers, objective timers, teammate ult tracking (Blitz/Porofessor already do this and it's a different product)
- Post-game analytics / performance scores (Mobalytics territory)
- Pre-game player scouting (rank, win rate of each lobby member) — nice to have, not core
- macOS support
- Anything that reads hidden info (enemy cooldowns, fog of war, spectator data). Riot banned enemy ult alerts in overlays in 2025; we stay well clear of that line.
## 4. What existing tools do (research summary)
| Tool | Build source | Adapts to enemy comp? | Full 6-item path? | Skill order | Imports to client | Overlay | Runs on |
|---|---|---|---|---|---|---|---|
| **op.gg app** | Highest WR aggregate | No | No (3 core items) | Yes (static) | Runes, spells, item set | Yes, Electron | Standalone |
| **Blitz** | Highest WR aggregate | No | Partial | Yes (static) | Runes, spells, item set — best auto-import | Yes, feature-rich, heavy RAM/ads | Standalone (Electron) |
| **U.GG** | High-elo aggregate | No | Partial | Yes (static) | Runes, item set | Yes, minimal | Standalone |
| **Mobalytics** | Highest WR aggregate | No | Partial | Yes (static) | Runes, item set | Yes, data-heavy | Overwolf |
| **Porofessor** | Aggregate | No | No | Yes (static) | Runes | Yes, scouting-focused, dated | Overwolf |
| **Hexgate** | WR filtered by enemy comp | **Yes** — scores items vs enemy damage split, CC, healing, tankiness | Yes, slot by slot | No | Runes only | Yes, minimal (Tauri) | Standalone |
| **buildzcrank / iTero** | AI + live game state | Partially | Partial | No | Varies | Yes | Standalone |
Key takeaways:
- Only Hexgate does real adaptive itemization, and it only starts once the game loads (it uses the Live Client Data API, not champ select). It has no skill-order guidance and doesn't import item sets.
- Nobody combines: adaptive build + full ordered path + skill order + one-click import + lane matchup context in one panel.
- The "why" is missing everywhere. Skill Capped writes the reasoning but it's a website, not an overlay.
- Ads and RAM are the two most-hated things about the big apps. Both are solvable by not being an ad business and not shipping a Chromium instance.
**Our differentiator, in one line:** the adaptive brain of Hexgate, the auto-import of Blitz, the reasoning of Skill Capped, in a panel the size of op.gg's.
## 5. User experience
### 5.1 Champ select
- Overlay detects champ select via the LCU API and reads: your champion, your assigned role, all 10 champions as they lock in.
- Panel shows: recommended runes + summoners (importable with one click, auto-import optional), starting item, and a **live-updating build path** that shifts as enemies lock in.
- Small tag next to any item that changed from the default, e.g. `Mortal Reminder (Soraka)`.
- Lane matchup line: "vs Tristana — loses lvl 2 all-in, wins after 2 items. Don't fight before 3."
### 5.2 Loading screen
- Final build locked. Item set written to the client so it appears in the shop.
- One-sentence game plan for your lane + one for teamfights.
### 5.3 In game (the main panel)
Single compact panel, default bottom-right above the minimap area, draggable, hotkey to collapse. Contents:
```
┌─────────────────────────────────────┐
│ NEXT: Infinity Edge         3400g   │  ← big, one item, the answer
│  ├ B.F. Sword ✓                     │  ← components, ticks as you buy
│  ├ Pickaxe                          │
│  └ Cloak of Agility                 │
│                                     │
│ Path: ER ✓ · Greaves ✓ · IE · Navori│
│       · Mortal Rem. · GA            │
│                                     │
│ LVL UP → E  (max E > W > Q)         │  ← flashes on level-up
│                                     │
│ ⓘ Mortal Reminder over LDR: Soraka  │  ← one line of "why"
└─────────────────────────────────────┘
```
- **Next item** is computed from your current inventory + gold (from Live Client Data API), so "what do I buy right now" is always literal.
- **Path** updates during the game as conditions change (enemy tank builds armor → LDR/Mortal moves up; enemy carry fed → defensive item moves up).
- **Skill level-up** flashes the recommended ability for ~3 seconds when your level increases.
- Hovering "ⓘ" shows the full reasoning. Never more than two lines by default.
### 5.4 Settings
- Auto-import on/off (runes, spells, item set independently)
- Panel position, scale, opacity
- "Quiet mode": hide path and reasoning, show only NEXT item + level-up
- Champion pool: mark your mains so their builds are pre-cached and tuned
## 6. How the build brain works
### 6.1 Data layer
- **Base builds:** per champion + role, scraped/aggregated per patch from public stats (U.GG / Lolalytics-style aggregates via their public pages or a licensed feed) — full 6-item paths, skill orders, rune pages, starting items. Refreshed each patch.
- **Matchup builds:** per champion + role + lane opponent where sample size allows (first item, start item, summoner swap).
- **Champion trait tags:** each champion tagged with: damage type (AD/AP/mixed), healing/shielding provided, tankiness scaling, hard CC (and whether it's a lockdown ult), burst/assassin, poke/sustain, mobility. Manually maintained, small file, patched when champs change.
- **Item trait tags:** each item tagged with what it answers: anti-heal, armor pen, magic pen, MR, armor, anti-burst, anti-CC (QSS), sustain, mobility.
### 6.2 Scoring
For each item slot, start from the base build for that slot, then apply rules from the enemy comp. Rules are explicit and readable (not a black box), e.g.:
```yaml
- when: enemy.healing_sources >= 1 and not team.has_antiheal
  then: prefer Mortal Reminder over Lord Dominik's
  reason: "{healer} heals — nobody else has anti-heal"
- when: enemy.tanks >= 2
  then: move armor_pen item up one slot
  reason: "{tanks} both build armor"
- when: enemy.lockdown_ults >= 1
  then: replace last damage item with Mercurial Scimitar
  reason: "QSS cleanses {champ} ult"
- when: enemy.assassins >= 2 or (enemy.fed_carry is assassin)
  then: move defensive item (GA / Shieldbow) to slot 4
  reason: "{champ} will dive you"
```
Rules are champion-agnostic where possible so they scale across the roster. Champion-specific overrides live in a small per-champion file (e.g. Xayah: "prefer Navori over other haste items because W CD starts on cast").
### 6.3 Live adjustments
- Read Live Client Data (`https://127.0.0.1:2999/liveclientdata/allgamedata`) every ~2s: your items, gold, level, game time; enemy items (visible in-game via Tab, so it's public info); scoreboard KDA.
- Recompute "NEXT" from current inventory: which component of the target item is affordable now.
- Re-run rules when relevant facts change (enemy buys Thornmail → armor pen bumps up; you're 0/4 → Navori before IE for a cheaper spike).
### 6.4 Optional LLM layer (v2)
A small model call at champ select can turn the rule output + matchup into the one-sentence lane plan. Rules make the decisions; the model just writes the sentence. Never let it pick items.
## 7. Architecture
```
┌──────────────────────────────────────────┐
│  Overlay app (Tauri: Rust core + webview) │
│                                          │
│  ┌──────────┐  ┌──────────┐  ┌─────────┐ │
│  │ LCU      │  │ Live     │  │ Build   │ │
│  │ client   │  │ Client   │  │ engine  │ │
│  │ (champ   │  │ Data     │  │ (rules  │ │
│  │ select,  │  │ poller   │  │ + data) │ │
│  │ imports) │  │          │  │         │ │
│  └────┬─────┘  └────┬─────┘  └────┬────┘ │
│       └─────────────┴─────────────┘      │
│                     │                    │
│              ┌──────┴──────┐             │
│              │ Overlay UI  │             │
│              │ (HTML/CSS)  │             │
│              └─────────────┘             │
└──────────────────────────────────────────┘
            │ once per patch
            ▼
   Build/rune/matchup data pack (JSON, ~few MB)
```
- **Tauri** over Electron: Hexgate proved this gets an overlay under 100 MB RAM. Rust handles the LCU auth (lockfile → port + password), polling, and window management; the UI is plain web.
- **LCU API** (local, authenticated via the client's lockfile): champ select session, rune page CRUD, summoner spell selection, item sets. This is the same mechanism op.gg and Blitz use for imports.
- **Live Client Data API** (local, `127.0.0.1:2999`, Riot-provided): all in-game data we need. No memory reading, no packet sniffing.
- **Data pack**: shipped as JSON, updated each patch from a small server or GitHub release. The app works offline with the last pack.
- Overlay window: transparent, always-on-top, click-through except on the panel itself. Works in borderless/windowed; fullscreen exclusive needs a fallback (same limitation every overlay has).
## 8. Riot compliance
Riot's stated rules for LoL third-party apps (developer.riotgames.com/docs/lol) permit game overlays showing static data available before the game and aggregate stats, and prohibit apps that provide game-session information previously unknown to the player, or that dictate player decisions. Build recommendation overlays, win-rate trackers, and real-time build suggestions are explicitly allowed and are used in ranked by Blitz, Hexgate, op.gg, etc.
How we stay inside the lines:
- **Only public data.** Enemy champions, their visible items, KDA, and game time are all things the player can see by pressing Tab. We never surface cooldowns, fog-of-war positions, or spectator data.
- **Recommend, don't dictate.** Everything is phrased as "recommended next" with a reason; the player chooses. No "go here now" instructions.
- **No enemy ability/ult timers.** Banned by Riot in March 2025. Not building it.
- **No ads in the overlay.** Riot banned in-game overlay ads in mid-2025 anyway; we simply aren't an ad product.
- **Official local APIs only.** LCU + Live Client Data. No game-file modification, no injection.
- If we ever want to pull per-player match history (scouting), we need a production API key and must follow RSO/opt-in rules. Out of scope for v1.
## 9. Milestones
**M0 — Prove the pipe (1–2 weeks)**
Python script: connect to LCU, print champ select as champs lock in; connect to Live Client Data, print your items/gold every 2s. Push a hardcoded Xayah item set into the client. Confirms every integration works before writing UI.
**M1 — Xayah-only overlay (3–4 weeks)**
Tauri overlay with the panel from §5.3. Data pack for Xayah only (all matchups, full paths). Rule engine with the ~10 core comp rules. Manual rune/spell/item-set import buttons. Dogfood in real games.
**M2 — All ADCs (3–4 weeks)**
Extend data pack to every bot-lane champion. Auto-import toggle. Skill level-up highlight. Reasoning tooltips. Settings panel.
**M3 — All roles + polish (ongoing)**
Full roster. Patch-day data refresh pipeline. Live adjustments (enemy armor → pen bump, behind → cheaper spike). Optional LLM one-liner for lane plan.
**Later / maybe**
Loading-screen scouting (needs production key), TFT/Arena (no — Riot restricts Arena item WR display), macOS.
## 10. Open questions
- **Data licensing.** Scraping U.GG/op.gg at scale is against their terms. Options: license a feed, aggregate from Riot's Match-v5 API ourselves (needs production key and real infra), or start with a hand-curated pack for a small champion pool and expand. For v1 with one role, hand-curation plus Skill Capped–style reasoning is realistic.
- **Component ordering.** Which component to buy first inside an item is itself meta-dependent (e.g. B.F. Sword vs Pickaxe first). Base builds should include component order, not just finished items.
- **Fullscreen.** League in exclusive fullscreen breaks every overlay. Detect and nudge the user to borderless, like the other apps do.
- **What counts as "behind"?** Gold diff vs lane opponent at 10/15 min is the obvious signal; needs tuning so the panel doesn't flip-flop.
- **Panel real estate.** The mockup in §5.3 is ~8 lines. Needs testing at 1080p and 1440p to make sure it doesn't cover anything that matters in a fight.
## 11. Success criteria
- A new player can open the shop, look at one spot on screen, and know exactly what to click. Zero alt-tabs.
- The build shown differs from the op.gg default in a meaningful share of games (target: >40%), and the change is explainable in one line every time.
- Under 100 MB RAM, no measurable FPS impact.
- Zero features that would be at risk under Riot's current third-party policy.
