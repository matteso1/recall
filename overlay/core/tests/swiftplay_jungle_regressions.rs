//! Behaviour contracts from the recorded Swiftplay game of 2026-09-06 (Wukong Jungle vs Zilean,
//! Locke, Sylas, Sivir, Mel). At the level-three start with 1400 gold the panel pointed at a
//! Trinity Force component instead of the jungle companion, and from 11:00 on it alternated
//! between Black Cleaver and an Executioner's Calling the player kept not buying.
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
        "../../../m0/tests/fixtures/opgg_wukong_jungle.json"
    ))
    .unwrap();
    aggregate::decode(&raw, 62, Position::Jungle, "global", "emerald_plus").unwrap()
}

fn snapshot(fixture: &str, gold: Option<f64>, items: Option<&[u32]>) -> LiveSnapshot {
    let mut raw: Value = serde_json::from_str(fixture).unwrap();
    if let Some(gold) = gold {
        raw["activePlayer"]["currentGold"] = json!(gold);
    }
    if let Some(ids) = items {
        let riot_id = raw["activePlayer"]["riotId"].clone();
        let me = raw["allPlayers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|p| p["riotId"] == riot_id)
            .unwrap();
        me["items"] = json!(ids
            .iter()
            .enumerate()
            .map(|(slot, id)| json!({"itemID": id, "count": 1, "slot": slot}))
            .collect::<Vec<_>>());
    }
    live::summarize(&raw)
}

fn start(gold: f64, level: u32, game_time: f64) -> LiveSnapshot {
    let mut raw: Value = serde_json::from_str(include_str!(
        "../../../m0/tests/fixtures/swiftplay_wukong_jungle_start.json"
    ))
    .unwrap();
    raw["activePlayer"]["currentGold"] = json!(gold);
    raw["activePlayer"]["level"] = json!(level);
    raw["gameData"]["gameTime"] = json!(game_time);
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
            champion: "Wukong",
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

const COMPANIONS: [u32; 3] = [1101, 1102, 1103];

#[test]
fn a_swiftplay_jungler_is_told_to_buy_the_companion_first() {
    // 0:00.7, level 3, 1400 gold, nothing bought: the recorded panel said "Trinity Force (buy
    // Hearthbound Axe)". The companion is what makes the jungle playable.
    let p = plan(&start(1400.0, 3, 0.69), &PlannerPreferences::default());
    let next = p.next.expect("a recommendation");
    assert!(COMPANIONS.contains(&next.id), "{next:?}");
    assert!(next.buy_now_affordable);
    // op.gg's starters for Wukong Jungle already name a companion, so the generic starter line
    // applies; a source without one gets the explicit companion line instead.
    assert!(
        p.why
            .iter()
            .any(|line| line.contains("companion") || line.contains("starting purchase")),
        "{:?}",
        p.why
    );
    assert!(p.start.iter().any(|s| COMPANIONS.contains(&s.id)));
    assert!(
        !p.start
            .iter()
            .any(|s| s.name.contains("Potion") || s.name.contains("Doran")),
        "{:?}",
        p.start
    );
}

#[test]
fn the_level_one_instant_before_a_swiftplay_start_still_points_at_the_companion() {
    let p = plan(&start(0.0, 1, 0.04), &PlannerPreferences::default());
    let next = p.next.expect("a recommendation");
    assert!(COMPANIONS.contains(&next.id), "{next:?}");
    assert!(!next.buy_now_affordable);
}

#[test]
fn after_the_companion_the_core_path_resumes() {
    let snap = snapshot(
        include_str!("../../../m0/tests/fixtures/swiftplay_wukong_jungle_start.json"),
        Some(950.0),
        Some(&[1101]),
    );
    let p = plan(&snap, &PlannerPreferences::default());
    let next = p.next.expect("a recommendation");
    assert_eq!(next.id, 3078, "Trinity Force next: {next:?}");
}

#[test]
fn a_detour_the_player_answered_with_another_purchase_is_not_offered_again() {
    // 11:00: Trinity Force, Steelcaps, Kindlegem and a Long Sword owned, 500 gold. Executioner's
    // Calling is affordable (Long Sword + 450) and Sylas heals, so it is offered once.
    let fixture = include_str!("../../../m0/tests/fixtures/swiftplay_wukong_jungle_1100.json");
    let owned = [1101, 3078, 3047, 3067, 1036, 3340];
    let first = plan(
        &snapshot(fixture, Some(500.0), Some(&owned)),
        &PlannerPreferences::default(),
    );
    let offered = first.next.expect("a recommendation");
    assert_eq!(offered.id, 3123, "Executioner's offered once: {offered:?}");
    assert_eq!(first.preferences.offered_detour, Some(3123));

    // The player bought a Ruby Crystal toward Black Cleaver instead. With the gold back above the
    // price the recorded panel flipped to Executioner's again; now the answer stands.
    let with_ruby = [1101, 3078, 3047, 3067, 1036, 1028];
    let second = plan(
        &snapshot(fixture, Some(500.0), Some(&with_ruby)),
        &first.preferences,
    );
    assert_eq!(second.preferences.declined_detours, vec![3123]);
    let next = second.next.expect("a recommendation");
    assert_eq!(
        next.id, 3071,
        "Black Cleaver, not the declined detour: {next:?}"
    );
    assert!(
        second
            .alternative
            .as_ref()
            .is_none_or(|alt| alt.item.id != 3123)
            || second.path.iter().all(|item| item.id != 3123),
    );

    // A second Long Sword builds into Executioner's but also into the planned Black Cleaver:
    // that is a purchase for the path, so the detour counts as answered (the recorded Zed game
    // bought a Long Sword toward Bastionbreaker and got the detour offered again).
    let with_sword = [1101, 3078, 3047, 3067, 1036, 1036];
    let third = plan(
        &snapshot(fixture, Some(200.0), Some(&with_sword)),
        &first.preferences,
    );
    assert_eq!(third.preferences.declined_detours, vec![3123]);

    // A new trinket or consumable is not an answer.
    let with_ward = [1101, 3078, 3047, 3067, 1036, 3340, 2055];
    let fourth = plan(
        &snapshot(fixture, Some(500.0), Some(&with_ward)),
        &first.preferences,
    );
    assert!(
        fourth.preferences.declined_detours.is_empty(),
        "{:?}",
        fourth.preferences
    );
}

#[test]
fn the_declined_detour_memory_is_carried_through_the_plan() {
    // The shell stores plan.preferences after every poll, so the memory must live there.
    let fixture = include_str!("../../../m0/tests/fixtures/swiftplay_wukong_jungle_1100.json");
    let p = plan(
        &snapshot(fixture, Some(500.0), None),
        &PlannerPreferences::default(),
    );
    let round_trip: PlannerPreferences =
        serde_json::from_value(serde_json::to_value(&p.preferences).unwrap()).unwrap();
    assert_eq!(round_trip, p.preferences);
    let legacy: PlannerPreferences =
        serde_json::from_value(json!({"mode": "balanced", "pinned_item": null})).unwrap();
    assert!(legacy.offered_detour.is_none() && legacy.declined_detours.is_empty());
}
