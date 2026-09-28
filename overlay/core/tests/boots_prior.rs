use recall_core::{aggregate::Position, bootsprior};

#[test]
fn measured_composition_changes_lee_sins_boots() {
    let physical = bootsprior::distribution(64, Position::Jungle, 0).unwrap();
    let magical = bootsprior::distribution(64, Position::Jungle, 4).unwrap();
    assert_eq!(physical.first().map(|p| p.0), Some(3047));
    assert_eq!(magical.first().map(|p| p.0), Some(3111));
    assert!(physical.iter().all(|p| p.1.is_finite() && p.1 > 0.0));
}

#[test]
fn missing_champion_role_has_no_invented_boots() {
    assert!(bootsprior::distribution(99999, Position::Adc, 0).is_none());
}

#[test]
fn production_plans_use_the_boots_distribution() {
    use recall_core::{
        aggregate,
        ddragon::Catalog,
        engine::{self, Inputs},
        pack,
    };
    let cat = Catalog::from_json(
        "16.19.1",
        &serde_json::from_str(include_str!("../../../m0/tests/fixtures/item_subset.json")).unwrap(),
        &serde_json::from_str(include_str!(
            "../../../m0/tests/fixtures/champion_subset.json"
        ))
        .unwrap(),
        &serde_json::json!([]),
    );
    let agg = aggregate::decode(
        &serde_json::from_str(include_str!(
            "../../../m0/tests/fixtures/opgg_leesin_jungle.json"
        ))
        .unwrap(),
        64,
        Position::Jungle,
        "global",
        "emerald_plus",
    )
    .unwrap();
    let traits = pack::load_traits().unwrap();
    for (enemies, expected) in [
        (["Zed", "Talon", "Draven", "Tryndamere", "Master Yi"], 3047),
        (["Ahri", "Lux", "Lissandra", "Viktor", "Kassadin"], 3111),
    ] {
        let enemies = enemies.map(str::to_string);
        let plan = engine::plan(&Inputs {
            champion: "Lee Sin",
            pack: None,
            aggregate: Some(&agg),
            traits: &traits,
            catalog: &cat,
            enemies: &enemies,
            live: None,
        });
        assert_eq!(
            plan.path.iter().find(|i| i.role == "boots").map(|i| i.id),
            Some(expected)
        );
    }
    let unknown_draft = engine::plan(&Inputs {
        champion: "Lee Sin",
        pack: None,
        aggregate: Some(&agg),
        traits: &traits,
        catalog: &cat,
        enemies: &[],
        live: None,
    });
    assert!(
        unknown_draft
            .path
            .iter()
            .filter(|i| i.role == "boots")
            .all(|i| !i
                .why
                .as_deref()
                .is_some_and(|why| why.starts_with("Common Master+"))),
        "an unknown draft is not five physical enemies"
    );
}
