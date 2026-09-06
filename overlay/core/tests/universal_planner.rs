//! Every role uses its own real aggregate. Xayah is not a fallback or an eligibility gate.
use featherstorm_core::{
    aggregate::{self, Position},
    ddragon::Catalog,
    engine::{self, Inputs},
    live::{Abilities, InvItem, LiveSnapshot, Me, Player},
    pack,
};
use serde_json::{json, Value};

const CASES: &[(&str, u32, Position, &str)] = &[
    (
        "Xayah",
        498,
        Position::Adc,
        include_str!("../../../m0/tests/fixtures/opgg_xayah_adc.json"),
    ),
    (
        "Ahri",
        103,
        Position::Mid,
        include_str!("../../../m0/tests/fixtures/opgg_ahri_mid.json"),
    ),
    (
        "Ornn",
        516,
        Position::Top,
        include_str!("../../../m0/tests/fixtures/opgg_ornn_top.json"),
    ),
    (
        "Darius",
        122,
        Position::Top,
        include_str!("../../../m0/tests/fixtures/opgg_darius_top.json"),
    ),
    (
        "Lulu",
        117,
        Position::Support,
        include_str!("../../../m0/tests/fixtures/opgg_lulu_support.json"),
    ),
    (
        "Lee Sin",
        64,
        Position::Jungle,
        include_str!("../../../m0/tests/fixtures/opgg_leesin_jungle.json"),
    ),
    (
        "Aphelios",
        523,
        Position::Adc,
        include_str!("../../../m0/tests/fixtures/opgg_aphelios_adc.json"),
    ),
    (
        "Udyr",
        77,
        Position::Jungle,
        include_str!("../../../m0/tests/fixtures/opgg_udyr_jungle.json"),
    ),
];

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

#[test]
fn real_champions_and_roles_receive_their_own_build_and_loadout() {
    let (cat, traits) = (catalog(), pack::load_traits().unwrap());
    for &(name, key, role, raw) in CASES {
        let a = aggregate::decode(
            &serde_json::from_str::<Value>(raw).unwrap(),
            key,
            role,
            "global",
            "emerald_plus",
        )
        .unwrap();
        let p = engine::plan(&Inputs {
            champion: name,
            pack: None,
            aggregate: Some(&a),
            traits: &traits,
            catalog: &cat,
            enemies: &[],
            live: None,
        });
        assert!(p.path.len() >= 4, "{name}: {:?}", p.note);
        assert_eq!(p.position.as_deref(), Some(role.label()), "{name}");
        assert_eq!(
            p.spell_ids, a.spells.ids,
            "loadouts must not inherit Xayah's Barrier: {name}"
        );
        assert_eq!(
            p.runes.as_ref().map(|r| &r.perks),
            a.runes.as_ref().map(|r| &r.perks),
            "{name}"
        );
        assert!(
            a.core
                .ids
                .iter()
                .filter(|id| cat.item(**id).is_some_and(|i| i.is_finished(&cat)))
                .all(|id| p.path.iter().any(|i| i.id == *id)),
            "{name}: {:?}",
            p.path
        );
        assert!(
            p.next.is_some() && p.learning.is_some(),
            "shared recommendation + reason missing for {name}"
        );
        if role == Position::Jungle {
            assert!(p.spell_ids.contains(&11));
        }
        if name != "Xayah" {
            assert!(
                !p.path.iter().any(|i| i.id == 3032),
                "Yun Tal leaked into {name}'s plan"
            );
        }
    }
}

#[test]
fn a_champions_aggregate_cannot_be_relabelled_as_another_champion() {
    let (cat, traits) = (catalog(), pack::load_traits().unwrap());
    let a = aggregate::decode(
        &serde_json::from_str::<Value>(CASES[0].3).unwrap(),
        498,
        Position::Adc,
        "global",
        "emerald_plus",
    )
    .unwrap();
    let p = engine::plan(&Inputs {
        champion: "Ahri",
        pack: None,
        aggregate: Some(&a),
        traits: &traits,
        catalog: &cat,
        enemies: &[],
        live: None,
    });
    assert!(p.path.is_empty() && p.next.is_none());
    assert!(p.note.is_some());
}

#[test]
fn nonstandard_skill_champions_never_get_an_invented_level_six_ultimate() {
    let (cat, traits) = (catalog(), pack::load_traits().unwrap());
    for &(name, key, role, raw) in CASES.iter().filter(|c| c.0 == "Aphelios" || c.0 == "Udyr") {
        let a = aggregate::decode(
            &serde_json::from_str::<Value>(raw).unwrap(),
            key,
            role,
            "global",
            "emerald_plus",
        )
        .unwrap();
        let snap = LiveSnapshot {
            mode: "CLASSIC".into(),
            game_time: 500.0,
            me: Some(Me {
                player: Player {
                    champion: name.into(),
                    level: 6,
                    position: role.slug().into(),
                    ..Default::default()
                },
                abilities: Abilities {
                    q: 2,
                    w: 2,
                    e: 1,
                    r: 0,
                },
                ..Default::default()
            }),
            ..Default::default()
        };
        let p = engine::plan(&Inputs {
            champion: name,
            pack: None,
            aggregate: Some(&a),
            traits: &traits,
            catalog: &cat,
            enemies: &[],
            live: Some(&snap),
        });
        assert_ne!(
            p.skill.next,
            Some('R'),
            "{name} does not use the standard R-at-6 rule"
        );
        assert!(
            p.next.is_some(),
            "nonstandard skills must not disable ordinary item recommendations"
        );
    }
}

#[test]
fn a_tanks_detour_component_does_not_count_as_a_completed_core() {
    let (cat, traits) = (catalog(), pack::load_traits().unwrap());
    let a = aggregate::decode(
        &serde_json::from_str::<Value>(CASES[2].3).unwrap(),
        516,
        Position::Top,
        "global",
        "emerald_plus",
    )
    .unwrap();
    let snap = LiveSnapshot {
        mode: "CLASSIC".into(),
        game_time: 600.0,
        me: Some(Me {
            player: Player {
                champion: "Ornn".into(),
                level: 10,
                position: "TOP".into(),
                items: vec![InvItem {
                    id: 3076,
                    count: 1,
                    slot: 0,
                    ..Default::default()
                }],
                ..Default::default()
            },
            ..Default::default()
        }),
        ..Default::default()
    };
    let p = engine::plan(&Inputs {
        champion: "Ornn",
        pack: None,
        aggregate: Some(&a),
        traits: &traits,
        catalog: &cat,
        enemies: &[],
        live: Some(&snap),
    });
    let first_pending = p
        .path
        .iter()
        .find(|i| !i.owned && i.role != "boots")
        .unwrap();
    assert_eq!(
        first_pending.id, 3068,
        "Bramble Vest is a component detour, not a finished Sunfire replacement"
    );
}

#[test]
fn a_full_inventory_still_allows_the_free_completed_support_quest_choice() {
    let (cat, traits) = (catalog(), pack::load_traits().unwrap());
    let a = aggregate::decode(
        &serde_json::from_str::<Value>(CASES[4].3).unwrap(),
        117,
        Position::Support,
        "global",
        "emerald_plus",
    )
    .unwrap();
    let items = [3867, 3504, 3158, 6617, 2065, 3107]
        .into_iter()
        .enumerate()
        .map(|(slot, id)| InvItem {
            id,
            count: 1,
            slot: slot as u32,
            ..Default::default()
        })
        .collect();
    let snap = LiveSnapshot {
        mode: "CLASSIC".into(),
        game_time: 1200.0,
        me: Some(Me {
            player: Player {
                champion: "Lulu".into(),
                level: 12,
                position: "UTILITY".into(),
                items,
                ..Default::default()
            },
            gold: 0.0,
            ..Default::default()
        }),
        ..Default::default()
    };
    let p = engine::plan(&Inputs {
        champion: "Lulu",
        pack: None,
        aggregate: Some(&a),
        traits: &traits,
        catalog: &cat,
        enemies: &[],
        live: Some(&snap),
    });
    let next = p
        .next
        .expect("a consuming upgrade is not a seventh-slot purchase");
    assert_eq!(next.buy_now.unwrap().id, 3870);
    assert!(next.buy_now_affordable);
    assert_eq!(next.remaining_cost, 0);
    assert_eq!(p.path.len(), 6);
    assert!(
        p.path.iter().all(|i| i.owned),
        "show all actual commitments until the player upgrades"
    );
}

#[test]
fn support_opening_includes_the_catalog_quest_then_the_aggregate_consumables() {
    let (cat, traits) = (catalog(), pack::load_traits().unwrap());
    let a = aggregate::decode(
        &serde_json::from_str::<Value>(CASES[4].3).unwrap(),
        117,
        Position::Support,
        "global",
        "emerald_plus",
    )
    .unwrap();
    assert_eq!(
        a.starters.ids,
        vec![2003, 2003],
        "source omits the support quest, not an edited fixture"
    );
    let mut snap = LiveSnapshot {
        mode: "CLASSIC".into(),
        game_time: 30.0,
        me: Some(Me {
            player: Player {
                champion: "Lulu".into(),
                level: 1,
                position: "UTILITY".into(),
                ..Default::default()
            },
            gold: 500.0,
            ..Default::default()
        }),
        ..Default::default()
    };
    let make = |live: &LiveSnapshot| {
        engine::plan(&Inputs {
            champion: "Lulu",
            pack: None,
            aggregate: Some(&a),
            traits: &traits,
            catalog: &cat,
            enemies: &[],
            live: Some(live),
        })
    };
    let p = make(&snap);
    assert_eq!(
        p.next.unwrap().buy_now.unwrap().id,
        3865,
        "the unique base support quest is a role mechanic"
    );
    snap.me.as_mut().unwrap().player.items = vec![InvItem {
        id: 3865,
        count: 1,
        slot: 0,
        ..Default::default()
    }];
    snap.me.as_mut().unwrap().gold = 100.0;
    let p = make(&snap);
    assert_eq!(
        p.next.unwrap().buy_now.unwrap().id,
        2003,
        "Atlas is not a manual alternative to the two potions"
    );
}

#[test]
fn a_low_cost_finished_mage_item_is_an_immutable_owned_slot() {
    let (cat, traits) = (catalog(), pack::load_traits().unwrap());
    let a = aggregate::decode(
        &serde_json::from_str::<Value>(CASES[1].3).unwrap(),
        103,
        Position::Mid,
        "global",
        "emerald_plus",
    )
    .unwrap();
    let items = [3041, 3118, 3020, 4645, 3157, 3089]
        .into_iter()
        .enumerate()
        .map(|(slot, id)| InvItem {
            id,
            count: 1,
            slot: slot as u32,
            ..Default::default()
        })
        .collect();
    let snap = LiveSnapshot {
        mode: "CLASSIC".into(),
        game_time: 1800.0,
        me: Some(Me {
            player: Player {
                champion: "Ahri".into(),
                level: 16,
                position: "MIDDLE".into(),
                items,
                ..Default::default()
            },
            gold: 4000.0,
            ..Default::default()
        }),
        ..Default::default()
    };
    let p = engine::plan(&Inputs {
        champion: "Ahri",
        pack: None,
        aggregate: Some(&a),
        traits: &traits,
        catalog: &cat,
        enemies: &[],
        live: Some(&snap),
    });
    assert!(p.path.iter().any(|i| i.id == 3041 && i.owned));
    assert!(
        p.next.is_none(),
        "Mejai's fills the sixth slot regardless of its low price"
    );
}
