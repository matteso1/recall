//! The recorded Swiftplay failure of 2026-09-05: Irelia assigned Jungle, while op.gg only has
//! Irelia Top and Mid data at this rank. The panel said it was waiting for build data forever.
//! A same-champion fallback must give a legal purchase, keep the real role for every role rule,
//! label the source role everywhere, and respect Swiftplay's shop (no Doran's, 1400 gold, level 3).
use recall_core::{
    aggregate::{self, Aggregate, Position},
    ddragon::Catalog,
    engine::{self, GameMode, Inputs, PlannerPreferences},
    live::{self, LiveSnapshot},
    pack, shop,
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

fn irelia_mid() -> Aggregate {
    let raw: Value = serde_json::from_str(include_str!(
        "../../../m0/tests/fixtures/opgg_irelia_mid.json"
    ))
    .unwrap();
    aggregate::decode(&raw, 39, Position::Mid, "global", "emerald_plus").unwrap()
}

/// What `aggregate::load` returns for Irelia when Jungle is assigned and Mid is the role it
/// could load: the Mid build with the Jungle assignment kept alongside.
fn irelia_mid_for_jungle() -> Aggregate {
    let mut a = irelia_mid();
    assert_eq!(
        aggregate::choose_position(Some(Position::Jungle), &a.positions),
        None,
        "Irelia has no Jungle games at this rank in the fixture"
    );
    assert!(aggregate::fallback_position(Position::Jungle, &a.positions).is_some());
    a.requested_position = Some(Position::Jungle);
    a
}

/// Game time 1:20 of the recorded Swiftplay game, identifiers removed: Irelia JUNGLE at level 3
/// with a Mosstomper Seedling, 982 gold, Flash + Smite.
fn recorded_snapshot() -> LiveSnapshot {
    let raw: Value = serde_json::from_str(include_str!(
        "../../../m0/tests/fixtures/swiftplay_irelia_jungle_0120.json"
    ))
    .unwrap();
    live::summarize(&raw)
}

fn plan_for(a: &Aggregate, live: Option<&LiveSnapshot>, mode: GameMode) -> engine::Plan {
    let (cat, traits) = (catalog(), pack::load_traits().unwrap());
    let enemies: Vec<String> = live
        .map(|l| l.enemies.iter().map(|p| p.champion.clone()).collect())
        .unwrap_or_default();
    engine::plan_in_mode(
        &Inputs {
            champion: "Irelia",
            pack: None,
            aggregate: Some(a),
            traits: &traits,
            catalog: &cat,
            enemies: &enemies,
            live,
        },
        &PlannerPreferences::default(),
        mode,
    )
}

#[test]
fn the_recorded_irelia_jungle_state_gets_a_legal_labelled_purchase() {
    let cat = catalog();
    let snapshot = recorded_snapshot();
    let me = snapshot
        .me
        .as_ref()
        .expect("the recorded player is identified");
    assert_eq!(me.player.champion, "Irelia");
    assert_eq!(me.player.position, "JUNGLE");
    assert_eq!(snapshot.mode, "SWIFTPLAY");
    assert_eq!(me.spell_ids, vec![4, 11]);
    assert!(me.player.has_item(1103), "Mosstomper Seedling is owned");

    let a = irelia_mid_for_jungle();
    let p = plan_for(&a, Some(&snapshot), GameMode::parse(&snapshot.mode));
    assert_eq!(p.position.as_deref(), Some("Jungle"), "the real role stays");
    assert_eq!(
        p.source_position.as_deref(),
        Some("Mid"),
        "the data role is labelled"
    );
    let note = p.note.clone().expect("the fallback is explained");
    assert!(
        note.contains("No Jungle data for Irelia") && note.contains("Mid build"),
        "{note}"
    );
    assert!(p.source.as_deref().unwrap().ends_with("(Mid build)"));
    assert!(p.path.len() >= 4, "{:?}", p.path);
    assert!(
        p.path.iter().any(|item| item.id == 1103 && item.owned),
        "the owned companion is a commitment: {:?}",
        p.path
    );
    assert!(
        p.path.iter().any(|item| item.id == 3153),
        "Blade of the Ruined King leads Irelia's Mid core: {:?}",
        p.path
    );

    let next = p.next.clone().expect("a purchase recommendation exists");
    assert!(next.price_known, "{next:?}");
    assert_eq!(next.blocked, None, "{next:?}");
    let buy = next.buy_now.clone().expect("a concrete shop action");
    assert!(
        next.buy_now_affordable || next.save_gap.is_some(),
        "affordable now or an explicit saving gap: {next:?}"
    );
    let bought = cat.item(buy.id).unwrap();
    assert!(
        !bought.name.starts_with("Doran's"),
        "Doran's items are disabled in Swiftplay: {buy:?}"
    );
    assert!(
        !bought.exclusive_groups().contains(&"JungleCompanion"),
        "a second companion is never recommended: {buy:?}"
    );
    let quote = shop::quote_with_context(
        &cat,
        buy.id,
        &me.player.items,
        me.gold,
        &shop::ShopContext {
            champion: Some("Irelia"),
            spell_ids: Some(&me.spell_ids),
            boots_locked: false,
            swiftplay: true,
        },
    );
    assert_eq!(
        quote.blocked, None,
        "the recommended purchase is legal in this shop"
    );
    if next.buy_now_affordable {
        assert!(
            f64::from(buy.cost) <= me.gold,
            "{buy:?} vs {} gold",
            me.gold
        );
    }

    assert_eq!(
        p.spell_ids,
        vec![4, 11],
        "the actual loadout is never rewritten"
    );
    assert!(p.skill.next.is_some(), "Irelia has a standard skill system");
    assert!(
        !p.start
            .iter()
            .any(|item| cat.item(item.id).unwrap().name.starts_with("Doran's")),
        "no Doran's starter in Swiftplay: {:?}",
        p.start
    );
    assert!(
        p.start.iter().any(|item| item.id == 1103 && item.owned),
        "the jungle companion is the jungle start: {:?}",
        p.start
    );
    assert!(p
        .context
        .iter()
        .any(|line| line.contains("Swiftplay") && line.contains("1400")));
    assert!(p.warnings.iter().all(|line| !line.contains("Waiting")));
}

#[test]
fn pre_queue_jungle_fallback_prepares_smite_and_a_companion_not_lane_spells() {
    let cat = catalog();
    let a = irelia_mid_for_jungle();
    assert_eq!(
        a.spells.ids,
        vec![4, 14],
        "the Mid data says Flash + Ignite"
    );
    let p = plan_for(&a, None, GameMode::Swiftplay);
    assert_eq!(p.position.as_deref(), Some("Jungle"));
    assert_eq!(p.source_position.as_deref(), Some("Mid"));
    assert_eq!(
        p.spell_ids,
        vec![4, 11],
        "Jungle needs Smite; Ignite is not imported"
    );
    assert_eq!(p.spells, vec!["Flash", "Smite"]);
    let starters: Vec<u32> = p.start.iter().map(|item| item.id).collect();
    for companion in [1101, 1102, 1103] {
        assert!(starters.contains(&companion), "{starters:?}");
    }
    assert!(
        !starters
            .iter()
            .any(|id| cat.item(*id).unwrap().name.starts_with("Doran's")),
        "{starters:?}"
    );
    assert!(p.context.iter().any(|line| line.contains("Smite")));
    assert!(!p.path.is_empty());
    let next = p.next.expect("a first purchase target before the game");
    assert_eq!(next.blocked, None);
    // The classic shop keeps the same jungle rules; only the Swiftplay-specific line disappears.
    let classic = plan_for(&a, None, GameMode::Classic);
    assert_eq!(classic.spell_ids, vec![4, 11]);
    let classic_starters: Vec<u32> = classic.start.iter().map(|item| item.id).collect();
    assert!(classic_starters.contains(&1103));
    assert!(
        !classic_starters
            .iter()
            .any(|id| cat.item(*id).unwrap().name.starts_with("Doran's")),
        "a jungle start and a Doran's item are mutually exclusive: {classic_starters:?}"
    );
    assert!(!classic
        .context
        .iter()
        .any(|line| line.contains("Swiftplay")));
}

#[test]
fn a_lane_assignment_with_only_jungle_data_never_imports_smite() {
    let cat = catalog();
    let raw: Value = serde_json::from_str(include_str!(
        "../../../m0/tests/fixtures/opgg_leesin_jungle.json"
    ))
    .unwrap();
    let mut a = aggregate::decode(&raw, 64, Position::Jungle, "global", "emerald_plus").unwrap();
    assert!(a.spells.ids.contains(&11));
    a.requested_position = Some(Position::Top);
    let (traits, enemies) = (pack::load_traits().unwrap(), Vec::<String>::new());
    let p = engine::plan(&Inputs {
        champion: "Lee Sin",
        pack: None,
        aggregate: Some(&a),
        traits: &traits,
        catalog: &cat,
        enemies: &enemies,
        live: None,
    });
    assert_eq!(p.position.as_deref(), Some("Top"));
    assert_eq!(p.source_position.as_deref(), Some("Jungle"));
    assert!(
        p.spell_ids.is_empty(),
        "no spell pair is invented: {:?}",
        p.spell_ids
    );
    assert!(
        !p.start.iter().any(|item| cat
            .item(item.id)
            .unwrap()
            .exclusive_groups()
            .contains(&"JungleCompanion")),
        "{:?}",
        p.start
    );
    assert!(p
        .context
        .iter()
        .any(|line| line.contains("choose your own summoner spells")));
    assert!(!p.path.is_empty());
}

#[test]
fn exact_role_data_is_unchanged_by_the_fallback_machinery() {
    let a = irelia_mid();
    let p = plan_for(&a, None, GameMode::Classic);
    assert_eq!(p.position.as_deref(), Some("Mid"));
    assert_eq!(p.source_position, None);
    assert_eq!(p.spell_ids, a.spells.ids);
    assert!(p.note.is_none() || !p.note.as_deref().unwrap().contains("No Mid data"));
    assert!(
        p.start.iter().any(|item| item.id == 1055),
        "Doran's Blade starts a classic lane"
    );
}

#[test]
fn swiftplay_starts_lane_champions_without_dorans_items() {
    let cat = catalog();
    let a = irelia_mid();
    let p = plan_for(&a, None, GameMode::Swiftplay);
    assert!(
        !p.start
            .iter()
            .any(|item| cat.item(item.id).unwrap().name.starts_with("Doran's")),
        "{:?}",
        p.start
    );
    assert!(p
        .context
        .iter()
        .any(|line| line.contains("Doran's items are disabled")));
    assert_eq!(GameMode::parse("SWIFTPLAY"), GameMode::Swiftplay);
    assert_eq!(GameMode::parse("CLASSIC"), GameMode::Classic);
    assert_eq!(GameMode::parse("ARAM"), GameMode::Unknown);
}
