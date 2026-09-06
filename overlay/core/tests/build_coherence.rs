//! From the recorded Swiftplay game of 2026-09-06 (Katarina Mid vs Jax, Brand, Yasuo, Yone,
//! Yuumi). op.gg's most-played line for Katarina is the on-hit one (Kraken Slayer, Blade of the
//! Ruined King, Terminus), but her late items and boots are dominated by the larger AP crowd, so
//! the panel showed Sorcerer's Shoes, then Zhonya's and Shadowflame behind an on-hit core.
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
        "../../../m0/tests/fixtures/opgg_katarina_mid.json"
    ))
    .unwrap();
    aggregate::decode(&raw, 55, Position::Mid, "global", "emerald_plus").unwrap()
}

fn snapshot(fixture: &str, items: Option<&[u32]>) -> LiveSnapshot {
    let mut raw: Value = serde_json::from_str(fixture).unwrap();
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

fn plan(snapshot: Option<&LiveSnapshot>) -> engine::Plan {
    let (cat, traits, a) = (catalog(), pack::load_traits().unwrap(), aggregate());
    let enemies: Vec<String> = snapshot
        .map(|s| s.enemies.iter().map(|p| p.champion.clone()).collect())
        .unwrap_or_default();
    engine::plan_with_preferences(
        &Inputs {
            champion: "Katarina",
            pack: None,
            aggregate: Some(&a),
            traits: &traits,
            catalog: &cat,
            enemies: &enemies,
            live: snapshot,
        },
        &PlannerPreferences::default(),
    )
}

const AT_0200: &str = include_str!("../../../m0/tests/fixtures/swiftplay_katarina_mid_0200.json");
const AT_1126: &str = include_str!("../../../m0/tests/fixtures/swiftplay_katarina_mid_1126.json");
const ON_HIT_CORE: [u32; 3] = [6672, 3153, 3302];
const AP_ONLY: [u32; 7] = [3100, 4645, 3089, 3157, 1082, 3041, 3135];
const SORCERERS_SHOES: u32 = 3020;

fn ids(items: &[engine::PlanItem]) -> Vec<u32> {
    items.iter().map(|i| i.id).collect()
}

#[test]
fn the_tail_and_boots_follow_the_core_lines_damage_family() {
    let p = plan(None);
    let path = ids(&p.path);
    for core in ON_HIT_CORE {
        assert!(path.contains(&core), "{path:?}");
    }
    for ap in AP_ONLY {
        assert!(
            !path.contains(&ap),
            "AP-only item {ap} behind an on-hit core: {path:?}"
        );
    }
    let boots = p
        .path
        .iter()
        .find(|i| i.role == "boots")
        .expect("boots on the path");
    assert_ne!(boots.id, SORCERERS_SHOES, "{boots:?}");
    assert!(
        !p.options.iter().any(|o| AP_ONLY.contains(&o.id)),
        "{:?}",
        ids(&p.options)
    );
}

#[test]
fn the_recorded_states_never_recommend_an_ap_item_behind_the_on_hit_core() {
    // 11:26: Sorcerer's Shoes and Kraken Slayer owned. The owned boots stay (a commitment), the
    // rest of the build does not drift into the AP crowd's items.
    let p = plan(Some(&snapshot(AT_1126, None)));
    let path = ids(&p.path);
    assert!(
        path.contains(&SORCERERS_SHOES),
        "owned boots are kept: {path:?}"
    );
    for ap in AP_ONLY {
        assert!(!path.contains(&ap), "{path:?}");
    }
    let next = p.next.expect("a recommendation");
    assert!(!AP_ONLY.contains(&next.id), "{next:?}");
    // 2:00 with only a Dagger: the first purchase is on-hit, not Sorcerer's Shoes.
    let early = plan(Some(&snapshot(AT_0200, Some(&[1042]))));
    let next = early.next.expect("a recommendation");
    assert!(
        !AP_ONLY.contains(&next.id) && next.id != SORCERERS_SHOES,
        "{next:?}"
    );
}

#[test]
fn mixed_items_and_plain_defense_stay_available_to_either_family() {
    // Nashor's Tooth (AP + attack speed), Guinsoo's (both) and Mercury's Treads carry no single
    // family; they are legal tail items for the on-hit core.
    let p = plan(None);
    let path = ids(&p.path);
    let pool: Vec<u32> = path.iter().copied().chain(ids(&p.options)).collect();
    assert!(
        pool.iter()
            .any(|id| [3115, 3124, 3111, 3091, 6333, 3146].contains(id)),
        "{pool:?}"
    );
}
