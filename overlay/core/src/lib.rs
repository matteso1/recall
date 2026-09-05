//! Featherstorm core: everything that is not the window.
//!
//! * `lcu`         - League client local API (lockfile auth): gameflow, champ select, imports
//! * `live`        - Live Client Data API (in-game): gold, items, levels, abilities
//! * `champselect` - champ select session -> lobby (who is on each side)
//! * `ddragon`     - Data Dragon catalog: items, champions, runes, by id and by name
//! * `pack`        - the hand-curated data pack (per-champion build, champion traits)
//! * `engine`      - the rules: enemy comp + live state -> ordered path, NEXT item, skill point
//! * `itemset`     - plan -> LCU item set (shows up in the in-game shop)
//! * `runes`       - pack rune page -> LCU perk page
//! * `state`       - what the panel renders
pub mod champselect;
pub mod ddragon;
pub mod engine;
pub mod itemset;
pub mod lcu;
pub mod live;
pub mod pack;
pub mod runes;
pub mod state;
