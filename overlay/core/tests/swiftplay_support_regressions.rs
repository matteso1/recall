//! Behaviour contracts from the recorded Swiftplay game of 2026-09-06 (Morgana Support vs Darius,
//! Lee Sin, Annie, Ziggs, Sona). The support quest, Zhonya's and the anti-heal calls were right;
//! near the end the target traded places between Morellonomicon and Rylai's as gold moved.
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
        "../../../m0/tests/fixtures/opgg_morgana_support.json"
    ))
    .unwrap();
    aggregate::decode(&raw, 25, Position::Support, "global", "emerald_plus").unwrap()
}

fn snapshot(fixture: &str, gold: Option<f64>) -> LiveSnapshot {
    let mut raw: Value = serde_json::from_str(fixture).unwrap();
    if let Some(gold) = gold {
        raw["activePlayer"]["currentGold"] = json!(gold);
    }
    live::summarize(&raw)
}

fn plan(snapshot: &LiveSnapshot, preferences: &PlannerPreferences) -> engine::Plan {
    let (cat, traits, a) = (catalog(), pack::load_traits().unwrap(), aggregate());
    let enemies: Vec<String> = snapshot
        .enemies
        .iter()
        .map(|p| p.champion.clone())
        .collect();
    engine::plan_with_preferences(
        &Inputs {
            champion: "Morgana",
            pack: None,
            aggregate: Some(&a),
            traits: &traits,
            catalog: &cat,
            enemies: &enemies,
            live: Some(snapshot),
        },
        preferences,
    )
}

const MORELLO: u32 = 3165;
const RYLAIS: u32 = 3116;
const AT_1514: &str =
    include_str!("../../../m0/tests/fixtures/swiftplay_morgana_support_1514.json");
const AT_1526: &str =
    include_str!("../../../m0/tests/fixtures/swiftplay_morgana_support_1526.json");
const AT_1604: &str =
    include_str!("../../../m0/tests/fixtures/swiftplay_morgana_support_1604.json");

#[test]
fn a_finishable_target_is_kept_while_it_stays_affordable() {
    // 15:14 with the gold of a few seconds later (1500), Oblivion Orb and Blasting Wand owned:
    // Morellonomicon (1200 left) can be finished, Rylai's (1750 left) cannot.
    let first = plan(
        &snapshot(AT_1514, Some(1500.0)),
        &PlannerPreferences::default(),
    );
    let target = first.next.as_ref().expect("a recommendation");
    assert_eq!(target.id, MORELLO, "{target:?}");
    assert!(target.buy_now_affordable);
    assert_eq!(first.preferences.last_target, Some(MORELLO));

    // 15:26, 1752 gold: Rylai's is affordable too. Without memory the recorded panel switched
    // to it; with the previous target still finishable it stays on Morellonomicon.
    let without_memory = plan(&snapshot(AT_1526, None), &PlannerPreferences::default());
    assert_eq!(without_memory.next.as_ref().unwrap().id, RYLAIS);
    let second = plan(&snapshot(AT_1526, None), &first.preferences);
    let kept = second.next.as_ref().expect("a recommendation");
    assert_eq!(kept.id, MORELLO, "{kept:?}");
    assert!(kept.buy_now_affordable);
}

#[test]
fn the_kept_target_gives_way_once_it_is_bought_or_no_longer_affordable() {
    let first = plan(
        &snapshot(AT_1514, Some(1500.0)),
        &PlannerPreferences::default(),
    );
    // 16:04: the player bought Rylai's instead and holds 470 gold. Morellonomicon is no longer
    // affordable, so ordinary scoring decides again; it is still the next item on the path.
    let after = plan(&snapshot(AT_1604, None), &first.preferences);
    let next = after.next.as_ref().expect("a recommendation");
    assert_eq!(next.id, MORELLO, "{next:?}");
    assert!(!next.buy_now_affordable);

    // Owning the kept target ends the memory of it as a target.
    let mut raw: Value = serde_json::from_str(AT_1604).unwrap();
    let riot_id = raw["activePlayer"]["riotId"].clone();
    let me = raw["allPlayers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["riotId"] == riot_id)
        .unwrap();
    me["items"] = json!([3871, 3158, 6653, 3157, 3116, 3165]
        .iter()
        .enumerate()
        .map(|(slot, id)| json!({"itemID": id, "count": 1, "slot": slot}))
        .collect::<Vec<_>>());
    raw["activePlayer"]["currentGold"] = json!(3000.0);
    let done = plan(&live::summarize(&raw), &after.preferences);
    assert_ne!(done.preferences.last_target, Some(MORELLO));
    assert!(done.next.as_ref().is_none_or(|n| n.id != MORELLO));
}

#[test]
fn a_pin_overrides_the_kept_target() {
    let first = plan(
        &snapshot(AT_1514, Some(1500.0)),
        &PlannerPreferences::default(),
    );
    let pinned = PlannerPreferences {
        pinned_item: Some(RYLAIS),
        ..first.preferences.clone()
    };
    let p = plan(&snapshot(AT_1526, None), &pinned);
    assert_eq!(p.next.as_ref().unwrap().id, RYLAIS);
}
