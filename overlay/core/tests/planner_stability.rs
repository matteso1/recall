#![cfg(feature = "evaluation")]
//! Small conditional distributions expose feedback between planning and presentation.
use recall_core::{
    aggregate::{self, Position},
    ddragon::Catalog,
    engine::{self, Inputs},
    live::{InvItem, LiveSnapshot, Me, Player},
    nextprior, pack,
};
use serde_json::json;

#[test]
fn repeating_an_observation_keeps_the_path_after_defensive_reordering() {
    // Two near-tied second items lead to different later purchases. Defensive
    // ordering may move one on screen; that must not replace the chain's memory.
    nextprior::load_for_evaluation(
        &json!({
            "champions": {"117|Support": {"games": 1000,
                "nth": {
                    "1": {"3504": 1000},
                    "2": {"3222": 550, "3109": 450},
                    "3": {"3109": 1000},
                    "4": {"6617": 1000},
                    "5": {"3157": 1000}
                },
                "sets": {"3109,3504": {"3107": 1000}}
            }},
            "roles": {}
        })
        .to_string(),
    )
    .unwrap();
    let cat = Catalog::from_json(
        "16.19.1",
        &serde_json::from_str(include_str!("../../../m0/tests/fixtures/item_subset.json")).unwrap(),
        &serde_json::from_str(include_str!(
            "../../../m0/tests/fixtures/champion_subset.json"
        ))
        .unwrap(),
        &json!([]),
    );
    let aggregate = aggregate::decode(
        &serde_json::from_str(include_str!(
            "../../../m0/tests/fixtures/opgg_lulu_support.json"
        ))
        .unwrap(),
        117,
        Position::Support,
        "global",
        "emerald_plus",
    )
    .unwrap();
    let traits = pack::load_traits().unwrap();
    let snap = LiveSnapshot {
        game_time: 360.0,
        mode: "CLASSIC".into(),
        me: Some(Me {
            gold: 517.0,
            player: Player {
                champion: "Lulu".into(),
                position: "UTILITY".into(),
                level: 4,
                items: vec![InvItem {
                    id: 1052,
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
    let enemies = ["Yasuo", "Wukong", "Taliyah", "Ezreal", "Karma"].map(str::to_string);
    let input = Inputs {
        champion: "Lulu",
        pack: None,
        aggregate: Some(&aggregate),
        traits: &traits,
        catalog: &cat,
        enemies: &enemies,
        live: Some(&snap),
    };
    let first = engine::plan(&input);
    let repeat = engine::plan_with_preferences(&input, &first.preferences);
    assert_eq!(first.preferences.last_path, repeat.preferences.last_path);
    assert_eq!(first.next.map(|n| n.id), repeat.next.map(|n| n.id));
}
