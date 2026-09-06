# Recall learning build engine

The owner approved the senior review and the itemization-companion scope, then clarified the target audience as beginner League players. This spec turns that approval into an executable design. No additional approval is needed for ordinary implementation choices.

## Outcome

The compact panel recommends a legal purchase using actual inventory and loadout, explains the immediate reason, and offers a short reusable lesson. Players can choose balanced or survival-oriented recommendations, pin a compatible target, return to automatic planning, and review significant decisions after the game. The decision system remains local, deterministic, and auditable.

## Boundaries

- Runtime game information comes only from LCU and Live Client Data, restricted to information the player can see.
- No enemy wallets, cooldown/ultimate timers, fog information, automated gameplay, or model-generated item choices.
- No unsupported win-probability claims. Aggregate win rates are observational evidence.
- Base builds come from aggregate data; factual mechanics and explicit policy rules may be curated, but no preferred build hidden in a champion pack.
- Every champion and role is in scope. Loadouts and candidate items come from that champion and role's aggregate; shared scoring derives build archetypes from item stats/effects. Regression data spans marksmen, mages, tanks, fighters, supports, and junglers. Xayah-specific mechanics are isolated factual data, never a fallback for other champions.
- Unsupported modes do not receive misleading ranked-SR contextual claims.
- No network call or model inference in the pure planner.
- Keep the existing Tauri/Rust/plain-JavaScript stack and compact visual identity.

## Architecture

`ddragon` retains item stats/descriptions and exposes typed effects and purchase restrictions. `shop` allocates owned components against recipe trees, computes exact remaining prices, and generates legal individual purchases and small baskets. `statistics` describes uncertainty; `aggregate` supplies the popular baseline and alternative observations with versioned cache provenance. `live` retains the actual runes, spells, position, and own combat stats. `engine` generates compatible plans and scores marginal needs and completion opportunities. `coaching` selects a grounded explanation and reusable lesson from the final decision. `journal` records significant changes and player feedback locally; `replay` runs the same engine offline.

The planner preserves owned equipment and values completion using remaining cost. It compares alternative orders and situational choices rather than mutating slots in a fixed rule order. Team anti-heal coverage is evidence with limited confidence, not a guarantee. Threats depend on observed equipment and levels, not a binary KDA state. Mechanical cleanse compatibility replaces broad ultimate tags. Explanations are generated from the final selected action.

## Player flow

In champion select, load the aggregate and import permitted loadout changes with success/retry tracking and respect for manual changes. In game, show next target, exact remaining cost, affordable purchase/basket, a concise reason, and a small expandable learning area. Advanced controls reveal alternate targets and survival preference. Unknown/stale data is visible. At game end, show a bounded recap of important decisions and allow useful/not-useful feedback saved on the machine.

## Validation

Regression cases include nested Long Sword credit, repeated Daggers, full inventory upgrade consumption, unknown item prices, incompatible boots/penetration, off-plan completed items, actual versus recommended Footwear, affordable IE while 0/3, invalid skill ranks after a different opening, false QSS answers, ambiguous bot opponents, stale state, short champion select, missing aggregates, and ARAM boundaries. Golden decisions specify forbidden outputs and acceptable alternatives. Replay uses entire sessions and never claims independent samples or causal uplift from snapshots.

## Integration decisions

Work on branch `feat/learning-build-engine` in the clean current checkout. Separate agents own independent files; the main agent owns integration, scoring, coaching, controls, and documentation. Use fresh tests for behavioral fixes and an independent final review. Build a Windows artifact if the installed toolchain is available; do not interrupt a running game or replace the active executable during play.

The owner clarified during implementation that this must be a general League tool, not a Xayah-first product. Apply all purchasing, preference, learning, and runtime features across champions. Candidate pools and factual mechanics may differ; missing or low-sample data is explicit. Do not substitute a different role's spells or a Xayah item path.
