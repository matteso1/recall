//! Behaviour contracts from the recorded Swiftplay game of 2026-09-06 (Annie Mid vs Miss Fortune,
//! Kayn, Vayne, Tristana, Seraphine). The AP build was coherent and Void Staff answered Vayne's
//! magic resist; two things were off: the 1400-gold start opened with Sorcerer's Shoes instead of
//! the Lost Chapter, and an Oblivion Orb the player kept not buying stayed the target.
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
        "../../../m0/tests/fixtures/opgg_annie_mid.json"
    ))
    .unwrap();
    aggregate::decode(&raw, 1, Position::Mid, "global", "emerald_plus").unwrap()
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
            champion: "Annie",
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

const START: &str = include_str!("../../../m0/tests/fixtures/swiftplay_annie_mid_start.json");
const AT_0955: &str = include_str!("../../../m0/tests/fixtures/swiftplay_annie_mid_0955.json");
const AT_1111: &str = include_str!("../../../m0/tests/fixtures/swiftplay_annie_mid_1111.json");
const AT_1127: &str = include_str!("../../../m0/tests/fixtures/swiftplay_annie_mid_1127.json");
const SORCERERS_SHOES: u32 = 3020;
const MALIGNANCE: u32 = 3118;
const LOST_CHAPTER: u32 = 3802;
const OBLIVION_ORB: u32 = 3916;

#[test]
fn the_swiftplay_start_opens_with_the_first_core_component_not_boots() {
    // 0:00, level 3, 1400 gold, nothing bought. The recorded panel said Sorcerer's Shoes.
    let p = plan(&snapshot(START, None), &PlannerPreferences::default());
    let next = p.next.expect("a recommendation");
    assert_eq!(next.id, MALIGNANCE, "{next:?}");
    let buy = next.buy_now.as_ref().expect("a component");
    assert_eq!(buy.id, LOST_CHAPTER, "{buy:?}");
    assert!(next.buy_now_affordable);
    assert!(
        p.path.iter().any(|i| i.id == SORCERERS_SHOES),
        "boots stay on the path"
    );
}

#[test]
fn boots_come_up_once_the_core_item_cannot_progress_right_now() {
    // 9:55 with Sorcerer's Shoes already owned nothing changes; with boots missing, Malignance
    // done and 700 gold that cannot buy Rocketbelt's next piece, boots are a fine buy.
    let mut raw: Value = serde_json::from_str(AT_0955).unwrap();
    let riot_id = raw["activePlayer"]["riotId"].clone();
    let me = raw["allPlayers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["riotId"] == riot_id)
        .unwrap();
    me["items"] = json!([{"itemID": MALIGNANCE, "count": 1, "slot": 0}]);
    raw["activePlayer"]["currentGold"] = json!(1100.0);
    let p = plan(&live::summarize(&raw), &PlannerPreferences::default());
    let next = p.next.expect("a recommendation");
    assert!(
        next.id == SORCERERS_SHOES || next.buy_now_affordable,
        "either boots now or a buyable core component: {next:?}"
    );
}

#[test]
fn a_declined_detour_is_never_the_target_again_even_when_its_need_alone_would_win() {
    // 9:55: Oblivion Orb affordable, Kayn heals, offered. The player then bought Rocketbelt
    // components (11:11) and finished the Rocketbelt (11:27). The recorded panel went back to the
    // Orb five more times; now it stays declined although its anti-heal score beats the next
    // core item's prior on its own.
    // 9:55 with the gold of the recorded offer a few seconds later.
    let first = plan(
        &snapshot(AT_0955, Some(1250.0)),
        &PlannerPreferences::default(),
    );
    assert_eq!(
        first.next.as_ref().unwrap().id,
        OBLIVION_ORB,
        "{:?}",
        first.next
    );
    let second = plan(&snapshot(AT_1111, None), &first.preferences);
    assert_eq!(second.preferences.declined_detours, vec![OBLIVION_ORB]);
    assert_ne!(second.next.as_ref().unwrap().id, OBLIVION_ORB);
    for gold in [10.0, 500.0, 1300.0] {
        let later = plan(&snapshot(AT_1127, Some(gold)), &second.preferences);
        let next = later.next.expect("a recommendation");
        assert_ne!(next.id, OBLIVION_ORB, "gold {gold}: {next:?}");
    }
    // It stays visible as a path item or option, not as the action.
    let later = plan(&snapshot(AT_1127, Some(1300.0)), &second.preferences);
    assert!(
        later
            .path
            .iter()
            .chain(later.options.iter())
            .any(|i| i.id == OBLIVION_ORB || i.id == 3165),
        "{:?}",
        later.path.iter().map(|i| i.id).collect::<Vec<_>>()
    );
}
