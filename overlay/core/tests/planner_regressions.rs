//! Behavioral contracts from real shopping failures, not snapshots of rule ordering.
use featherstorm_core::{
    aggregate::{self, Aggregate, Position},
    ddragon::Catalog,
    engine::{self, Inputs, Plan},
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
        "../../../m0/tests/fixtures/opgg_xayah_adc.json"
    ))
    .unwrap();
    aggregate::decode(&raw, 498, Position::Adc, "global", "emerald_plus").unwrap()
}

fn live(ids: &[u32], gold: f64) -> LiveSnapshot {
    let mut raw: Value =
        serde_json::from_str(include_str!("../../../m0/tests/fixtures/allgamedata.json")).unwrap();
    raw["gameData"]["gameMode"] = json!("CLASSIC");
    raw["gameData"]["gameTime"] = json!(900.0);
    raw["activePlayer"]["currentGold"] = json!(gold);
    raw["activePlayer"]["level"] = json!(10);
    raw["activePlayer"]["fullRunes"] = json!({"generalRunes": [{"id":8008}], "statRunes": []});
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
    me["level"] = json!(10);
    me["position"] = json!("BOTTOM");
    me["scores"]["kills"] = json!(0);
    me["scores"]["deaths"] = json!(3);
    let mut snap = live::summarize(&raw);
    snap.enemies.clear();
    snap.allies.clear();
    snap
}

fn planned(live: Option<&LiveSnapshot>, enemies: &[&str], with_aggregate: bool) -> Plan {
    let (cat, pack, traits, agg) = (
        catalog(),
        pack::load_xayah().unwrap(),
        pack::load_traits().unwrap(),
        aggregate(),
    );
    let enemies: Vec<String> = enemies.iter().map(|n| n.to_string()).collect();
    engine::plan(&Inputs {
        champion: "Xayah",
        pack: Some(&pack),
        aggregate: with_aggregate.then_some(&agg),
        traits: &traits,
        catalog: &cat,
        enemies: &enemies,
        live,
    })
}

#[test]
fn finish_affordable_ie_instead_of_starting_navori_when_zero_three() {
    let snap = live(&[3032, 3006, 1038, 1037, 1018], 725.0);
    let p = planned(Some(&snap), &[], true);
    let next = p.next.expect("a purchase");
    assert_eq!(
        next.id, 3031,
        "existing components and affordable completion beat the KDA shortcut"
    );
    assert_eq!(next.remaining_cost, 725);
    assert_eq!(next.buy_now.unwrap().id, 3031);
    assert!(next.buy_now_affordable);
}

#[test]
fn a_component_is_investment_not_a_completed_alternative_first_item() {
    let snap = live(&[1038, 3006], 0.0);
    let p = planned(Some(&snap), &[], true);
    assert_eq!(
        p.next.unwrap().id,
        3032,
        "B.F. Sword builds toward Yun Tal; it is not a completed alternate core"
    );
}

#[test]
fn the_opening_respects_a_different_starter_and_potion_multiplicity() {
    let mut snap = live(&[1055, 2003], 400.0);
    snap.game_time = 65.0;
    snap.me.as_mut().unwrap().player.level = 1;
    let p = planned(Some(&snap), &[], true);
    assert_ne!(
        p.next.as_ref().map(|n| n.id),
        Some(1086),
        "do not buy a second starter because the player chose Blade"
    );
    assert_eq!(
        p.start.iter().filter(|i| i.id == 2003 && i.owned).count(),
        1,
        "one owned potion cannot tick both entries"
    );
}

#[test]
fn an_early_component_purchase_does_not_restart_the_starting_kit() {
    let mut snap = live(&[1036], 450.0);
    snap.game_time = 60.0;
    snap.me.as_mut().unwrap().player.level = 1;
    let p = planned(Some(&snap), &[], true);
    assert!(
        !p.start
            .iter()
            .any(|i| Some(i.id) == p.next.as_ref().map(|n| n.id)),
        "follow the invested build instead of adding a new starting kit"
    );
}

#[test]
fn an_automatic_blocked_target_never_beats_a_legal_purchase() {
    let (cat, traits, agg) = (catalog(), pack::load_traits().unwrap(), aggregate());
    let mut snap = live(&[3032, 3006, 6675, 3031, 1029, 1055], 800.0);
    snap.enemies.push(live::Player {
        champion: "Malzahar".into(),
        level: 10,
        ..Default::default()
    });
    let preferences = engine::PlannerPreferences {
        mode: engine::BuildPreference::Survival,
        ..Default::default()
    };
    let p = engine::plan_with_preferences(
        &Inputs {
            champion: "Xayah",
            pack: None,
            aggregate: Some(&agg),
            traits: &traits,
            catalog: &cat,
            enemies: &["Malzahar".into()],
            live: Some(&snap),
        },
        &preferences,
    );
    let next = p.next.expect("legal upgrade exists");
    assert!(
        next.blocked.is_none(),
        "an unavailable QSS branch must not beat a legal inventory combine: {next:?}"
    );
    assert!(next.buy_now_affordable);
}

#[test]
fn never_replace_owned_armor_pen_with_a_conflicting_purchase() {
    let snap = live(&[3032, 3006, 6675, 3031, 3036], 3000.0);
    let p = planned(Some(&snap), &["Soraka"], true);
    assert!(
        p.path.iter().any(|i| i.id == 3036 && i.owned),
        "purchased LDR is a commitment, not an unfilled slot"
    );
    assert!(
        !p.path.iter().any(|i| i.id == 3033 && !i.owned),
        "Mortal Reminder cannot coexist with LDR"
    );
}

#[test]
fn off_plan_upgraded_boots_satisfy_the_boots_slot() {
    let snap = live(&[3032, 3047], 1100.0);
    let p = planned(Some(&snap), &[], true);
    assert!(p.path.iter().any(|i| i.id == 3047 && i.owned));
    assert!(!p.path.iter().any(|i| i.role == "boots" && !i.owned));
    assert_ne!(p.next.and_then(|n| n.buy_now).map(|b| b.id), Some(1001));
}

#[test]
fn actual_runes_not_the_recommended_page_control_footwear() {
    let snap = live(&[3032], 900.0);
    let p = planned(Some(&snap), &[], true);
    assert!(
        !p.why
            .iter()
            .any(|s| s.contains("locked") || s.contains("free boots")),
        "this player does not have Magical Footwear"
    );
    assert!(!p.path.iter().any(|i| i.tag.as_deref() == Some("free @12")));
}

#[test]
fn qss_is_not_an_answer_to_mordekaiser_zed_or_fizz_ults() {
    for champion in ["Mordekaiser", "Zed", "Fizz"] {
        let p = planned(None, &[champion], true);
        assert!(
            !p.why
                .iter()
                .any(|s| s.contains("cleans") && s.contains(champion)),
            "unsafe cleanse claim against {champion}: {:?}",
            p.why
        );
        assert!(
            !p.path.iter().any(|i| [3139, 3140].contains(&i.id)),
            "no QSS item solely due to {champion}"
        );
    }
}

#[test]
fn a_different_opening_never_produces_an_illegal_skill_rank() {
    let mut snap = live(&[], 0.0);
    let me = snap.me.as_mut().unwrap();
    me.player.level = 2;
    me.abilities = live::Abilities {
        q: 0,
        w: 0,
        e: 1,
        r: 0,
    };
    let p = planned(Some(&snap), &[], true);
    assert!(p.skill.point_available);
    assert!(
        matches!(p.skill.next, Some('Q' | 'W')),
        "E rank 2 is not legal at level 2: {:?}",
        p.skill
    );
}

#[test]
fn no_aggregate_does_not_resurrect_a_hidden_preferred_build() {
    let p = planned(None, &[], false);
    assert!(
        p.path.is_empty(),
        "no evidence should be shown as missing data, not a hand-picked ER build"
    );
    assert!(p.next.is_none());
    assert!(p.note.is_some());
}

#[test]
fn aram_does_not_reuse_ranked_summoners_rift_recommendations() {
    let mut snap = live(&[3032], 3000.0);
    snap.mode = "ARAM".to_string();
    let p = planned(Some(&snap), &["Soraka"], true);
    assert!(
        p.next.is_none(),
        "ranked SR catalog and aggregates are not an ARAM recommendation"
    );
    assert!(p.note.as_deref().is_some_and(|s| s.contains("ARAM")));
}

#[test]
fn ambiguous_bottom_roles_do_not_invent_a_lane_opponent() {
    let p = planned(None, &["Ashe", "Tristana"], true);
    assert!(
        p.matchup_champion.is_none(),
        "two possible bot carries: wait for the actual position"
    );
}

#[test]
fn a_known_support_position_cannot_be_overruled_by_a_carry_trait() {
    let mut snap = live(&[3032], 1000.0);
    snap.enemies.push(live::Player {
        champion: "Ashe".into(),
        position: "UTILITY".into(),
        level: 8,
        ..Default::default()
    });
    let p = planned(Some(&snap), &["Ashe"], true);
    assert!(
        p.matchup_champion.is_none(),
        "actual support Ashe is not an inferred ADC opponent"
    );
    assert!(!p
        .context
        .iter()
        .any(|s| s.starts_with("Visible equipment vs Ashe")));
}

#[test]
fn counters_are_game_outcomes_not_lane_win_statistics() {
    let (cat, traits, agg) = (catalog(), pack::load_traits().unwrap(), aggregate());
    let p = engine::plan(&Inputs {
        champion: "Xayah",
        pack: None,
        aggregate: Some(&agg),
        traits: &traits,
        catalog: &cat,
        enemies: &["Tristana".into()],
        live: None,
    });
    assert!(p.matchup.as_deref().is_some_and(|s| s.contains("games")));
    assert!(!p
        .matchup
        .as_deref()
        .is_some_and(|s| s.contains("wins") && s.contains("lanes")));
}

#[test]
fn a_compatible_player_pin_wins_and_has_a_direct_reason() {
    let (cat, traits, agg) = (catalog(), pack::load_traits().unwrap(), aggregate());
    let snap = live(&[3032, 3006], 1000.0);
    let pref = engine::PlannerPreferences {
        pinned_item: Some(3031),
        ..Default::default()
    };
    let p = engine::plan_with_preferences(
        &Inputs {
            champion: "Xayah",
            pack: None,
            aggregate: Some(&agg),
            traits: &traits,
            catalog: &cat,
            enemies: &[],
            live: Some(&snap),
        },
        &pref,
    );
    assert_eq!(p.next.as_ref().map(|n| n.id), Some(3031));
    assert!(p.why.first().is_some_and(|s| s.contains("pinned")));
    assert!(p
        .learning
        .as_ref()
        .is_some_and(|tip| !tip.lesson.is_empty()));
}

#[test]
fn an_incompatible_pin_is_rejected_without_a_sell_recommendation() {
    let (cat, traits, agg) = (catalog(), pack::load_traits().unwrap(), aggregate());
    let snap = live(&[3032, 3006, 3036], 3000.0);
    let pref = engine::PlannerPreferences {
        pinned_item: Some(3033),
        ..Default::default()
    };
    let p = engine::plan_with_preferences(
        &Inputs {
            champion: "Xayah",
            pack: None,
            aggregate: Some(&agg),
            traits: &traits,
            catalog: &cat,
            enemies: &[],
            live: Some(&snap),
        },
        &pref,
    );
    assert_ne!(p.next.as_ref().map(|n| n.id), Some(3033));
    assert!(p.preferences.pinned_item.is_none());
    assert!(p
        .warnings
        .iter()
        .any(|s| s.contains("pinned") || s.contains("Pinned")));
    assert!(p.path.iter().any(|i| i.id == 3036 && i.owned));
}

#[test]
fn recommendations_remain_single_action_and_have_an_explanation() {
    let snap = live(&[3032, 3006], 1000.0);
    let p = planned(Some(&snap), &[], true);
    assert!(p.next.is_some());
    assert!(p
        .why
        .first()
        .is_some_and(|s| !s.is_empty() && s.chars().count() <= 140));
    assert_eq!(p.learning.as_ref().map(|tip| &tip.reason), p.why.first());
}

#[test]
fn a_legacy_tank_tag_does_not_move_items_without_observed_armor() {
    let (cat, mut traits, agg) = (catalog(), pack::load_traits().unwrap(), aggregate());
    let enemies = vec!["Malphite".into(), "Ornn".into()];
    let before = engine::plan(&Inputs {
        champion: "Xayah",
        pack: None,
        aggregate: Some(&agg),
        traits: &traits,
        catalog: &cat,
        enemies: &enemies,
        live: None,
    });
    traits.champions.get_mut("Malphite").unwrap().tank = false;
    traits.champions.get_mut("Ornn").unwrap().tank = false;
    let after = engine::plan(&Inputs {
        champion: "Xayah",
        pack: None,
        aggregate: Some(&agg),
        traits: &traits,
        catalog: &cat,
        enemies: &enemies,
        live: None,
    });
    assert_eq!(before.path, after.path);
    assert_eq!(before.next, after.next);
}

#[test]
fn kda_alone_is_not_a_price_or_power_signal() {
    let mut snap = live(&[3032, 3006, 1038, 1037, 1018], 725.0);
    let before = planned(Some(&snap), &[], true);
    let me = snap.me.as_mut().unwrap();
    me.player.kills = 12;
    me.player.deaths = 0;
    me.player.assists = 15;
    let after = planned(Some(&snap), &[], true);
    assert_eq!(before.next, after.next);
    assert_eq!(before.score_trace, after.score_trace);
}

#[test]
fn actual_armor_amount_can_bring_penetration_forward() {
    let cat = catalog();
    let mut snap = live(&[3032, 3006, 6675], 1500.0);
    let enemy_items = ["Thornmail", "Frozen Heart", "Randuin's Omen"]
        .iter()
        .enumerate()
        .map(|(slot, name)| live::InvItem {
            id: cat.item_id(name).unwrap(),
            name: (*name).into(),
            count: 1,
            slot: slot as u32,
        })
        .collect();
    snap.enemies.push(live::Player {
        champion: "Malphite".into(),
        level: 11,
        position: "TOP".into(),
        items: enemy_items,
        ..Default::default()
    });
    let p = planned(Some(&snap), &["Malphite"], true);
    let next = p.next.as_ref().unwrap();
    assert!(
        cat.item(next.id)
            .unwrap()
            .effects
            .percent_armor_pen
            .is_some(),
        "{:?}",
        p.score_trace
    );
    assert!(p.why[0].contains("armor") && p.why[0].contains("visible"));
}

#[test]
fn verified_suppression_can_get_a_small_detour_then_resume_the_core() {
    let mut snap = live(&[3032, 3006], 1300.0);
    snap.enemies.push(live::Player {
        champion: "Malzahar".into(),
        level: 10,
        position: "MIDDLE".into(),
        ..Default::default()
    });
    let p = planned(Some(&snap), &["Malzahar"], true);
    assert_eq!(
        p.next.as_ref().map(|n| n.id),
        Some(3140),
        "{:?}",
        p.score_trace
    );
    assert!(p.why[0].contains("suppression"));
    snap.me.as_mut().unwrap().player.items.push(live::InvItem {
        id: 3140,
        count: 1,
        slot: 2,
        ..Default::default()
    });
    snap.me.as_mut().unwrap().gold = 0.0;
    let after = planned(Some(&snap), &["Malzahar"], true);
    assert_ne!(after.next.as_ref().map(|n| n.id), Some(3140));
    assert_ne!(
        after.next.as_ref().map(|n| n.id),
        Some(3139),
        "do not rush the full cleanse item immediately after the cheap detour"
    );
}

#[test]
fn a_full_completed_build_has_no_automatic_seventh_item() {
    let snap = live(&[3032, 3006, 6675, 3031, 3036, 3072], 5000.0);
    let p = planned(Some(&snap), &["Malzahar", "Zed", "Soraka"], true);
    assert!(
        p.next.is_none(),
        "the player must explicitly choose any sale, not be told to buy a seventh item"
    );
    assert_eq!(p.path.len(), 6);
    assert!(p.path.iter().all(|i| i.owned));
}
