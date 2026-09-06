use featherstorm_core::ddragon::Catalog;
use featherstorm_core::live::InvItem;
use featherstorm_core::shop::{
    compatible, compatible_with_context, quote, quote_with_context, ShopContext,
};
use serde_json::Value;

fn catalog() -> Catalog {
    let items =
        serde_json::from_str(include_str!("../../../m0/tests/fixtures/item_subset.json")).unwrap();
    let champions = serde_json::from_str(include_str!(
        "../../../m0/tests/fixtures/champion_subset.json"
    ))
    .unwrap();
    Catalog::from_json("16.17.1", &items, &champions, &Value::Null)
}

fn catalog_with_group(group: &str, limit: i32, ids: &[u32]) -> Catalog {
    let mut items: Value =
        serde_json::from_str(include_str!("../../../m0/tests/fixtures/item_subset.json")).unwrap();
    // Exercise the published group schema using real item prices/recipes. The
    // test family is deliberately not a claim about these items' game groups.
    items["groups"] = serde_json::json!([
        { "id": group, "MaxGroupOwnable": limit.to_string() }
    ]);
    for id in ids {
        items["data"][id.to_string()]["group"] = Value::String(group.to_string());
    }
    Catalog::from_json("16.17.1", &items, &Value::Null, &Value::Null)
}

fn inventory(ids: &[u32]) -> Vec<InvItem> {
    ids.iter()
        .enumerate()
        .map(|(slot, &id)| InvItem {
            id,
            name: String::new(),
            count: 1,
            slot: slot as u32,
        })
        .collect()
}

#[test]
fn context_actual_smite_allows_affordable_jungle_companions() {
    let cat = catalog();
    let context = ShopContext {
        champion: Some("Lee Sin"),
        spell_ids: Some(&[4, 11]),
        boots_locked: false,
    };
    for id in [1101, 1102, 1103] {
        let q = quote_with_context(&cat, id, &[], 450.0, &context);
        assert!(q.affordable, "verified Smite should allow item {id}");
        assert_eq!(q.remaining_cost, Some(450));
        assert_eq!(
            q.buy_now.as_ref().map(|item| (item.id, item.cost)),
            Some((id, 450))
        );
        assert!(compatible_with_context(&cat, id, &[], &context));
    }
    let saving = quote_with_context(&cat, 1101, &[], 449.0, &context);
    assert!(!saving.affordable);
    assert!(saving.buy_now.is_none());
    assert_eq!(
        saving.save_for.as_ref().map(|item| (item.id, item.cost)),
        Some((1101, 450))
    );
}

#[test]
fn context_missing_or_absent_smite_blocks_jungle_companion_purchases() {
    let cat = catalog();
    for spell_ids in [None, Some(&[4, 14][..]), Some(&[][..])] {
        let context = ShopContext {
            champion: Some("Lee Sin"),
            spell_ids,
            boots_locked: false,
        };
        let q = quote_with_context(&cat, 1101, &[], 5000.0, &context);
        assert!(!q.affordable);
        assert!(q.buy_now.is_none());
        assert!(q.blocked.is_some());
        assert!(!compatible_with_context(&cat, 1101, &[], &context));
    }
    assert!(!quote(&cat, 1101, &[], 5000.0, false).affordable);
    assert!(!compatible(&cat, 1101, &[]));
}

#[test]
fn context_smite_preserves_jungle_exclusivity_and_inventory_capacity() {
    let cat = catalog();
    let context = ShopContext {
        spell_ids: Some(&[11, 4]),
        ..Default::default()
    };
    assert!(compatible_with_context(&cat, 1101, &[], &context));
    assert!(!compatible_with_context(&cat, 1102, &[1101], &context));
    let full = inventory(&[1055, 3508, 6675, 3031, 3006, 2003]);
    let q = quote_with_context(&cat, 1101, &full, 450.0, &context);
    assert!(!q.affordable);
    assert!(q.buy_now.is_none());
    assert!(q.blocked.is_some());
}

#[test]
fn context_unknown_spells_with_an_owned_jungle_item_allow_ordinary_upgrades() {
    let cat = catalog();
    let context = ShopContext::default();
    let full = inventory(&[1038, 1037, 1018, 1101, 3006, 2003]);
    let q = quote_with_context(&cat, 3031, &full, 725.0, &context);
    assert!(q.affordable);
    assert_eq!(
        q.buy_now.as_ref().map(|item| (item.id, item.cost)),
        Some((3031, 725))
    );
    assert!(compatible_with_context(&cat, 3031, &[1101], &context));
}

#[test]
fn context_verified_champion_matches_restrictions_by_catalog_identity() {
    let mut cat = catalog();
    // Exercise the identity gate without inventing a champion-specific item.
    cat.items.get_mut(&1036).unwrap().required_champion = Some("MonkeyKing".to_string());
    let context = ShopContext {
        champion: Some("Wukong"),
        ..Default::default()
    };
    assert!(quote_with_context(&cat, 1036, &[], 350.0, &context).affordable);
    assert!(compatible_with_context(&cat, 1036, &[], &context));
    for champion in [None, Some(""), Some("Xayah")] {
        let other = ShopContext {
            champion,
            ..Default::default()
        };
        let q = quote_with_context(&cat, 1036, &[], 350.0, &other);
        assert!(!q.affordable);
        assert!(q.blocked.is_some());
        assert!(!compatible_with_context(&cat, 1036, &[], &other));
    }
    assert!(!quote(&cat, 1036, &[], 350.0, false).affordable);
    assert!(!compatible(&cat, 1036, &[]));
}

#[test]
fn context_champion_identity_does_not_infer_required_allies_or_unlocks() {
    let mut cat = catalog();
    cat.items.get_mut(&1036).unwrap().required_champion = Some("Wukong".to_string());
    cat.items.get_mut(&1036).unwrap().required_ally = Some("Ornn".to_string());
    let context = ShopContext {
        champion: Some("Wukong"),
        spell_ids: Some(&[11, 4]),
        boots_locked: false,
    };
    assert!(!quote_with_context(&cat, 1036, &[], 350.0, &context).affordable);
    assert!(!quote_with_context(&cat, 3172, &inventory(&[3006]), 0.0, &context).affordable);
    assert!(!compatible_with_context(&cat, 3172, &[3006], &context));
    let footwear = ShopContext {
        boots_locked: true,
        ..context
    };
    assert!(!quote_with_context(&cat, 1001, &[], 300.0, &footwear).affordable);
    assert!(!compatible_with_context(&cat, 1001, &[], &footwear));
}

#[test]
fn dorans_blade_blocks_other_doran_starters() {
    let cat = catalog();
    for id in [1086, 1056, 1054, 1120] {
        let q = quote(&cat, id, &inventory(&[1055]), 5000.0, false);
        assert!(!q.affordable, "Doran's Blade must exclude starter {id}");
        assert!(q.buy_now.is_none());
        assert!(q.basket.is_empty());
        assert!(q.blocked.is_some());
        assert!(!compatible(&cat, id, &[1055]));
    }
}

#[test]
fn actual_smite_does_not_bypass_doran_and_jungle_starter_exclusions() {
    let cat = catalog();
    let context = ShopContext {
        spell_ids: Some(&[11, 4]),
        ..Default::default()
    };
    for jungle in [1101, 1102, 1103] {
        // Establish the spell gate is open, so it cannot mask this restriction.
        assert!(quote_with_context(&cat, jungle, &[], 450.0, &context).affordable);
        for doran in [1055, 1056, 1054, 1086, 1120] {
            for (target, owned) in [(jungle, doran), (doran, jungle)] {
                let q = quote_with_context(&cat, target, &inventory(&[owned]), 5000.0, &context);
                assert!(!q.affordable, "starter {owned} must exclude {target}");
                assert!(q.buy_now.is_none());
                assert!(q.blocked.is_some());
                assert!(!compatible_with_context(&cat, target, &[owned], &context));
            }
        }
    }
}

#[test]
fn atlas_and_runic_compass_exclude_lane_and_jungle_starters() {
    let cat = catalog();
    let context = ShopContext {
        spell_ids: Some(&[11, 4]),
        ..Default::default()
    };
    for owned in [3865, 3866] {
        for target in [1055, 1056, 1054, 1086, 1120, 1101, 1102, 1103] {
            let q = quote_with_context(&cat, target, &inventory(&[owned]), 5000.0, &context);
            assert!(
                !q.affordable,
                "unfinished support quest {owned} must exclude {target}"
            );
            assert!(q.buy_now.is_none());
            assert!(q.blocked.is_some());
            assert!(!compatible_with_context(&cat, target, &[owned], &context));
        }
    }
    for owned in [1055, 1086, 1101] {
        assert!(!quote_with_context(&cat, 3865, &inventory(&[owned]), 5000.0, &context).affordable);
        assert!(!compatible_with_context(&cat, 3865, &[owned], &context));
    }
}

#[test]
fn finished_support_quests_do_not_keep_the_starter_exclusion() {
    let cat = catalog();
    for support in [3867, 3869, 3870, 3871, 3876, 3877] {
        let q = quote(&cat, 1055, &inventory(&[support]), 450.0, false);
        assert!(
            q.affordable,
            "completed support quest {support} should permit a Doran item"
        );
        assert_eq!(
            q.buy_now.as_ref().map(|item| (item.id, item.cost)),
            Some((1055, 450))
        );
        assert!(compatible(&cat, 1055, &[support]));
    }
}

#[test]
fn starter_exclusions_leave_ordinary_completed_upgrades_legal() {
    let cat = catalog();
    for starter in [1055, 3865, 3866, 1101] {
        let full = inventory(&[1038, 1037, 1018, starter, 3006, 2003]);
        let q = quote(&cat, 3031, &full, 725.0, false);
        assert!(
            q.affordable,
            "starter {starter} must not prevent a normal upgrade"
        );
        assert_eq!(
            q.buy_now.as_ref().map(|item| (item.id, item.cost)),
            Some((3031, 725))
        );
        assert!(compatible(&cat, 3031, &[starter]));
    }
}

#[test]
fn bounty_of_worlds_upgrades_for_free_with_six_occupied_slots() {
    let cat = catalog();
    let full = inventory(&[3867, 3006, 3508, 6675, 3031, 2003]);
    let q = quote(&cat, 3870, &full, 0.0, false);
    assert!(q.affordable);
    assert_eq!(q.remaining_cost, Some(0));
    assert_eq!(
        q.buy_now.as_ref().map(|item| (item.id, item.cost)),
        Some((3870, 0))
    );
    assert_eq!(q.basket.len(), 1);
    assert_eq!(q.basket_cost, 0);
    assert!(q.blocked.is_none());
    assert!(compatible(
        &cat,
        3870,
        &[3867, 3006, 3508, 6675, 3031, 2003]
    ));
}

#[test]
fn explicit_group_metadata_excludes_a_second_family_member() {
    let cat = catalog_with_group("TestEquipmentFamily", 1, &[1036, 1037]);
    let q = quote(&cat, 1037, &inventory(&[1036]), 5000.0, false);
    assert!(!q.affordable);
    assert!(q.blocked.is_some());
    assert!(!compatible(&cat, 1037, &[1036]));
}

#[test]
fn explicit_group_limits_count_quantities_and_preserve_unlimited_groups() {
    let cat = catalog_with_group("TestEquipmentFamily", 2, &[1036, 1037]);
    assert!(compatible(&cat, 1037, &[1036]));
    let mut stacked = inventory(&[1036]);
    stacked[0].count = 2;
    assert!(!quote(&cat, 1037, &stacked, 5000.0, false).affordable);
    assert!(!compatible(&cat, 1037, &[1036, 1036]));

    let unlimited = catalog_with_group("TestEquipmentFamily", -1, &[1036, 1037]);
    assert!(quote(&unlimited, 1037, &stacked, 5000.0, false).affordable);
    assert!(compatible(&unlimited, 1037, &[1036, 1036]));
}

#[test]
fn explicit_group_restrictions_apply_after_consuming_owned_components() {
    let cat = catalog_with_group("TestRecipeFamily", 1, &[1036, 3133]);
    let q = quote(&cat, 3133, &inventory(&[1036]), 5000.0, false);
    assert!(q.affordable);
    assert!(q.blocked.is_none());
    assert!(compatible(&cat, 3133, &[1036]));
}

#[test]
fn mejais_is_a_finished_commitment_without_promoting_starters_or_components() {
    let cat = catalog();
    let mejais = cat
        .item(3041)
        .expect("official patch fixture includes Mejai's");
    assert_eq!(mejais.total, 1500);
    assert_eq!(mejais.from, [1082]);
    assert!(mejais.is_finished(&cat));
    assert!(mejais.is_owned_commitment(&cat));
    for id in [
        1055, 1056, 1086, 1082, 1083, 1036, 1038, 3133, 2003, 1001, 2422,
    ] {
        assert!(
            !cat.item(id).unwrap().is_finished(&cat),
            "starter/component {id} is not a finished item"
        );
    }
}
