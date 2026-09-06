//! Behaviour contracts from the recorded Swiftplay game of 2026-09-06 (Xayah ADC vs Cho'Gath,
//! Shaco, Naafiri, Ashe, Yuumi). The panel briefly pointed at a Quicksilver Sash "for magic
//! protection", at Stormrazor because it happened to be affordable with IE's components, at a
//! Randuin's Omen nobody buys on Xayah, and at a Health Potion in Swiftplay's level-one instant.
use featherstorm_core::{
    aggregate::{self, Aggregate, Position},
    ddragon::Catalog,
    engine::{self, GameMode, Inputs, PlannerPreferences},
    live::{self, InvItem, LiveSnapshot, Player},
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
        "../../../m0/tests/fixtures/opgg_xayah_adc.json"
    ))
    .unwrap();
    aggregate::decode(&raw, 498, Position::Adc, "global", "emerald_plus").unwrap()
}

fn enemy(champion: &str, position: &str, level: u32, items: &[u32]) -> Player {
    Player {
        champion: champion.into(),
        team: "CHAOS".into(),
        position: position.into(),
        level,
        items: items
            .iter()
            .enumerate()
            .map(|(slot, &id)| InvItem {
                id,
                name: String::new(),
                count: 1,
                slot: slot as u32,
            })
            .collect(),
        ..Default::default()
    }
}

/// Xayah bottom in Swiftplay with the recorded enemy team, at the given inventory and gold.
fn state(time: f64, level: u32, gold: f64, ids: &[u32]) -> LiveSnapshot {
    let mut raw: Value =
        serde_json::from_str(include_str!("../../../m0/tests/fixtures/allgamedata.json")).unwrap();
    raw["gameData"]["gameMode"] = json!("SWIFTPLAY");
    raw["gameData"]["gameTime"] = json!(time);
    raw["activePlayer"]["currentGold"] = json!(gold);
    raw["activePlayer"]["level"] = json!(level);
    raw["activePlayer"]["fullRunes"] = json!({"generalRunes": [{"id": 8008}], "statRunes": []});
    let me = raw["allPlayers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["riotId"] == "matteso#NA1")
        .unwrap();
    me["items"] = json!(ids
        .iter()
        .enumerate()
        .map(|(slot, id)| json!({"itemID": id, "count": 1, "slot": slot}))
        .collect::<Vec<_>>());
    me["level"] = json!(level);
    me["position"] = json!("BOTTOM");
    let mut snap = live::summarize(&raw);
    snap.allies.clear();
    snap.enemies = vec![
        enemy("Cho'Gath", "TOP", level, &[3084, 3047, 3075]),
        enemy("Shaco", "JUNGLE", level, &[3142, 3009]),
        enemy("Naafiri", "MIDDLE", level + 1, &[3008, 6697]),
        enemy("Ashe", "BOTTOM", level, &[3006, 6672]),
        enemy("Yuumi", "UTILITY", level, &[3870, 3047]),
    ];
    snap
}

fn plan(snapshot: &LiveSnapshot) -> engine::Plan {
    let (cat, traits, a) = (catalog(), pack::load_traits().unwrap(), aggregate());
    let enemies: Vec<String> = snapshot.enemies.iter().map(|p| p.champion.clone()).collect();
    engine::plan_in_mode(
        &Inputs {
            champion: "Xayah",
            pack: None,
            aggregate: Some(&a),
            traits: &traits,
            catalog: &cat,
            enemies: &enemies,
            live: Some(snapshot),
        },
        &PlannerPreferences::default(),
        GameMode::Swiftplay,
    )
}

#[test]
fn an_affordable_off_path_item_never_displaces_the_next_core_item() {
    // 14:48 in the recorded game: four items done, 1400 gold, IE next. The panel said Quicksilver Sash.
    let p = plan(&state(888.0, 13, 1400.0, &[3032, 3006, 6675, 3033]));
    let next = p.next.expect("a recommendation");
    assert_eq!(next.id, 3031, "Infinity Edge stays the target: {next:?}");
    let buy = next.buy_now.expect("a component to buy");
    assert_ne!(buy.id, 3140, "no Quicksilver Sash detour");
    assert!(next.buy_now_affordable, "a 600 g Cloak or a 1300 g B. F. Sword fits 1400 g");
    assert!(
        !p.why.iter().any(|line| line.contains("Quicksilver")),
        "{:?}",
        p.why
    );
}

#[test]
fn shared_components_do_not_switch_the_target_to_a_cheaper_off_path_item() {
    // 16:33: B. F. Sword and Cloak owned toward IE, 1422 gold. The panel said Stormrazor and the
    // player bought it, so IE was never completed.
    let p = plan(&state(993.0, 15, 1422.0, &[3032, 3006, 6675, 3033, 1038, 1018]));
    let next = p.next.expect("a recommendation");
    assert_eq!(next.id, 3031, "Infinity Edge keeps its components: {next:?}");
    // Six slots are full, so the Pickaxe cannot be bought loose; the finished item needs 1600
    // (Pickaxe plus the combine), 178 more than the player holds. The honest action is saving.
    let buy = next.buy_now.as_ref().expect("the finished item as a saving target");
    assert_eq!(buy.id, 3031, "{buy:?}");
    assert!(!next.buy_now_affordable);
    assert_eq!(next.save_gap, Some(178), "{next:?}");
    assert!(
        p.alternative.as_ref().is_none_or(|alt| alt.item.id != 3095) || next.id == 3031,
        "Stormrazor may be an alternative, never the action"
    );
}

#[test]
fn the_anti_heal_reason_names_the_healer_not_a_lifesteal_boot() {
    // 9:41: Mortal Reminder became the target. Naafiri's Gluttonous Greaves are minor sustain;
    // Yuumi is the healer the item is for.
    let p = plan(&state(581.0, 9, 315.0, &[3032, 3006, 6675]));
    let next = p.next.expect("a recommendation");
    assert_eq!(next.id, 3033, "Mortal Reminder against the healer: {next:?}");
    let reason = p.learning.as_ref().map(|tip| tip.reason.as_str()).unwrap_or("");
    assert!(reason.contains("Yuumi"), "{reason}");
    assert!(!reason.contains("Naafiri"), "{reason}");
}

#[test]
fn swiftplays_level_one_instant_does_not_recommend_a_potion() {
    // 0:00 in Swiftplay: the client reports level 1 for a moment before the level-three start.
    let p = plan(&state(0.04, 1, 0.0, &[]));
    let next = p.next.expect("a recommendation");
    assert_eq!(next.id, 3032, "Yun Tal is the first item, not a Health Potion: {next:?}");
    assert!(!p.why.iter().any(|line| line.contains("Potion")), "{:?}", p.why);
}

#[test]
fn items_almost_nobody_buys_on_this_champion_are_not_candidates() {
    for ids in [
        &[3032, 3006, 6675, 3033][..],
        &[3032, 3006, 6675, 3033, 3095][..],
    ] {
        let p = plan(&state(1151.0, 17, 1857.0, ids));
        for rare in [3143, 6664] {
            assert!(
                !p.path.iter().any(|item| item.id == rare)
                    && !p.options.iter().any(|item| item.id == rare)
                    && p.next.as_ref().is_none_or(|next| next.id != rare),
                "{rare} appeared with {ids:?}: {:?}",
                p.path
            );
        }
    }
}
