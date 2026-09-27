//! From the draft game of 2026-09-26 (Malphite Top vs Teemo, Wukong, Yone, Sivir, Braum). op.gg's
//! most-played Malphite line is all armor (Sunfire Aegis, Thornmail, Frozen Heart, Plated
//! Steelcaps), and the panel followed it the whole game: "Sunfire: armor for Yone's damage
//! profile", Steelcaps "the common boot upgrade", then Thornmail at 22:54 with 259 armor and 50
//! magic resist while Teemo (Liandry's Torment) laned against him. Magic resist first appeared as
//! the fifth item. The states below are rebuilt by hand from that game's scoreboard and the
//! client's measured stats; no recorded payload is included.
use recall_core::{
    aggregate::{self, Aggregate, Position},
    ddragon::Catalog,
    engine::{self, Inputs, PlannerPreferences},
    live::{self, LiveSnapshot},
    pack,
};
use serde_json::{json, Value};

fn catalog() -> Catalog {
    Catalog::from_json(
        "16.17.1",
        &serde_json::from_str(include_str!("../../../m0/tests/fixtures/item_subset.json")).unwrap(),
        &serde_json::from_str(include_str!(
            "../../../m0/tests/fixtures/champion_subset.json"
        ))
        .unwrap(),
        &json!([]),
    )
}

fn aggregate() -> Aggregate {
    let raw = serde_json::from_str(include_str!(
        "../../../m0/tests/fixtures/opgg_malphite_top.json"
    ))
    .unwrap();
    aggregate::decode(&raw, 54, Position::Top, "global", "emerald_plus").unwrap()
}

const SUNFIRE: u32 = 3068;
const THORNMAIL: u32 = 3075;
const FROZEN_HEART: u32 = 3110;
const STEELCAPS: u32 = 3047;
const MERCURYS: u32 = 3111;
const BRAMBLE_VEST: u32 = 3076;
const RUBY_CRYSTAL: u32 = 1028;
const DORANS_RING: u32 = 1056;
const LONG_SWORD: u32 = 1036;
const WARD: u32 = 3340;
const POSITIONS: [&str; 5] = ["TOP", "JUNGLE", "MIDDLE", "BOTTOM", "UTILITY"];
const TONIGHT: [&str; 5] = ["Teemo", "Wukong", "Yone", "Sivir", "Braum"];
/// A control lineup that deals only physical damage.
const PHYSICAL: [&str; 5] = ["Darius", "Lee Sin", "Zed", "Ashe", "Pyke"];

/// One live state. `stats` are the client's measured (armor, magic resist, max health); enemies
/// are (level, scoreboard items) in `POSITIONS` order.
fn snapshot(
    seconds: f64,
    level: u32,
    gold: f64,
    mine: &[u32],
    stats: (f64, f64, f64),
    names: [&str; 5],
    enemies: [(u32, &[u32]); 5],
) -> LiveSnapshot {
    let items = |ids: &[u32]| -> Vec<Value> {
        ids.iter()
            .enumerate()
            .map(|(slot, id)| {
                let slot = if *id == WARD { 6 } else { slot };
                json!({"itemID": id, "count": 1, "slot": slot})
            })
            .collect()
    };
    let mut players = vec![json!({
        "riotId": "Player#TEST", "championName": "Malphite", "team": "ORDER",
        "position": "TOP", "level": level, "items": items(mine), "scores": {}
    })];
    for (i, (level, owned)) in enemies.iter().enumerate() {
        players.push(json!({
            "riotId": format!("Enemy{i}#TEST"), "championName": names[i], "team": "CHAOS",
            "position": POSITIONS[i], "level": level, "items": items(owned), "scores": {}
        }));
    }
    let (armor, magic_resist, health) = stats;
    live::summarize(&json!({
        "activePlayer": {
            "riotId": "Player#TEST", "level": level, "currentGold": gold,
            "championStats": {"armor": armor, "magicResist": magic_resist,
                               "maxHealth": health, "currentHealth": health}
        },
        "allPlayers": players,
        "gameData": {"gameTime": seconds, "gameMode": "CLASSIC"}
    }))
}

fn plan(names: [&str; 5], live: Option<&LiveSnapshot>) -> engine::Plan {
    let (cat, traits, a) = (catalog(), pack::load_traits().unwrap(), aggregate());
    let enemies: Vec<String> = names.iter().map(|s| s.to_string()).collect();
    engine::plan_with_preferences(
        &Inputs {
            champion: "Malphite",
            pack: None,
            aggregate: Some(&a),
            traits: &traits,
            catalog: &cat,
            enemies: &enemies,
            live,
        },
        &PlannerPreferences::default(),
    )
}

fn ids(p: &engine::Plan) -> Vec<u32> {
    p.path.iter().map(|i| i.id).collect()
}

fn magic_resist(id: u32) -> bool {
    catalog()
        .item(id)
        .is_some_and(|i| i.effects.magic_resist.is_some_and(|v| v > 0.0))
}

/// Position of the first unowned magic-resist item on the path, boots excluded.
fn first_mr(p: &engine::Plan) -> Option<usize> {
    p.path
        .iter()
        .position(|i| !i.owned && i.role != "boots" && magic_resist(i.id))
}

fn position(p: &engine::Plan, id: u32) -> Option<usize> {
    p.path.iter().position(|i| i.id == id)
}

fn boots(p: &engine::Plan) -> u32 {
    p.path
        .iter()
        .find(|i| i.role == "boots")
        .expect("boots on the path")
        .id
}

fn reason(p: &engine::Plan) -> String {
    p.learning
        .as_ref()
        .map(|l| l.reason.clone())
        .unwrap_or_default()
}

/// 8:00 in lane: Doran's Ring and a Ruby Crystal toward Sunfire.
fn at_0800(names: [&str; 5]) -> LiveSnapshot {
    let physical = names == PHYSICAL;
    let lane: &[u32] = if physical {
        &[1055, LONG_SWORD, WARD]
    } else {
        &[DORANS_RING, 1052, WARD]
    };
    snapshot(
        8.0 * 60.0,
        7,
        350.0,
        &[DORANS_RING, RUBY_CRYSTAL, WARD],
        (85.0, 38.0, 1450.0),
        names,
        [
            (8, lane),
            (6, &[LONG_SWORD, WARD]),
            (7, &[1086, LONG_SWORD, WARD]),
            (7, &[1086, LONG_SWORD, WARD]),
            (6, &[3876, WARD]),
        ],
    )
}

/// 15:30, just after Plated Steelcaps: Sunfire Aegis and Steelcaps owned, 300 gold. Stats follow
/// Malphite's growth at level 11 with those items.
fn at_1530() -> LiveSnapshot {
    snapshot(
        15.5 * 60.0,
        11,
        300.0,
        &[DORANS_RING, SUNFIRE, STEELCAPS, WARD],
        (200.0, 55.0, 2100.0),
        TONIGHT,
        [
            (12, &[6653, DORANS_RING, WARD]),
            (10, &[3078, WARD]),
            (11, &[1086, 3153, WARD]),
            (10, &[1086, 3508, WARD]),
            (9, &[3876, 1031, WARD]),
        ],
    )
}

/// 22:54 as measured: Doran's Ring, Sunfire Aegis, Plated Steelcaps, Bramble Vest, Ruby Crystal;
/// 430 gold; 259 armor, 50 magic resist, 2614 health. Enemy items from the scoreboard.
fn at_2254() -> LiveSnapshot {
    snapshot(
        22.9 * 60.0,
        13,
        430.0,
        &[
            DORANS_RING,
            SUNFIRE,
            STEELCAPS,
            BRAMBLE_VEST,
            RUBY_CRYSTAL,
            WARD,
        ],
        (259.0, 50.0, 2614.0),
        TONIGHT,
        [
            (14, &[6653, 3087, WARD]),
            (12, &[3078, STEELCAPS, WARD]),
            (13, &[1086, 3153, 3172, 6673, 1037, WARD]),
            (11, &[1086, 3508, 6675, 3123, WARD]),
            (11, &[3876, 1031, STEELCAPS, 3190, 1006, 3067, WARD]),
        ],
    )
}

#[test]
fn before_the_game_magic_resist_follows_the_first_item_instead_of_two_more_armor_items() {
    // Draft hides enemy positions, so no lane opponent is known yet: Teemo, Yone (half magic)
    // and Braum against Wukong, Yone and Sivir is an even split. op.gg's first item stays first.
    let p = plan(TONIGHT, None);
    let path = ids(&p);
    assert_eq!(path.first(), Some(&SUNFIRE), "{path:?}");
    let mr = first_mr(&p).expect("a magic-resist item on the path");
    assert!(
        mr < position(&p, THORNMAIL).unwrap_or(usize::MAX),
        "{path:?}"
    );
    assert!(
        mr < position(&p, FROZEN_HEART).unwrap_or(usize::MAX),
        "{path:?}"
    );
}

#[test]
fn in_lane_against_teemo_the_boots_are_mercurys_treads() {
    let p = plan(TONIGHT, Some(&at_0800(TONIGHT)));
    assert_eq!(boots(&p), MERCURYS, "{:?}", ids(&p));
    let mr = first_mr(&p).expect("a magic-resist item on the path");
    assert!(
        mr < position(&p, THORNMAIL).unwrap_or(usize::MAX),
        "{:?}",
        ids(&p)
    );
}

#[test]
fn against_an_all_physical_lineup_the_armor_line_and_steelcaps_stay() {
    let p = plan(PHYSICAL, Some(&at_0800(PHYSICAL)));
    let path = ids(&p);
    assert_eq!(boots(&p), STEELCAPS, "{path:?}");
    assert_eq!(path.first(), Some(&SUNFIRE), "{path:?}");
    assert!(
        position(&p, THORNMAIL).unwrap_or(usize::MAX) < first_mr(&p).unwrap_or(usize::MAX),
        "{path:?}"
    );
}

#[test]
fn after_sunfire_and_steelcaps_the_next_defensive_item_is_magic_resist() {
    // Recorded: "Thornmail: reduces Yone's healing only when they attack you".
    let p = plan(TONIGHT, Some(&at_1530()));
    let next = p.next.as_ref().expect("a next item").id;
    assert!(magic_resist(next), "next {next}: {}", reason(&p));
    assert_ne!(next, THORNMAIL);
    assert!(reason(&p).contains("Teemo"), "{}", reason(&p));
}

#[test]
fn with_259_armor_and_50_magic_resist_the_panel_buys_magic_resist_and_says_why() {
    // Recorded: "Thornmail: armor for Yone's damage profile". Kaenic Rookern adds about 48%
    // effective health for its remaining 2500 gold against this team, finishing Thornmail about
    // 13% for 1250, so magic resist comes first even with Thornmail's components paid.
    let p = plan(TONIGHT, Some(&at_2254()));
    let next = p.next.as_ref().expect("a next item").id;
    assert!(magic_resist(next), "next {next}: {}", reason(&p));
    assert!(reason(&p).contains("259 armor vs 50 MR"), "{}", reason(&p));
    // Thornmail stays planned (its Bramble Vest is owned), and no third pure-armor item comes
    // before magic resist.
    let path = ids(&p);
    assert!(path.contains(&THORNMAIL), "{path:?}");
    assert!(
        first_mr(&p).unwrap_or(usize::MAX) < position(&p, FROZEN_HEART).unwrap_or(usize::MAX),
        "{path:?}"
    );
}
