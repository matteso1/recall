//! Plan -> LCU item set, so the ordered build shows inside the in-game shop.
use crate::ddragon::Catalog;
use crate::engine::Plan;
use crate::pack::ChampionPack;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use uuid::Uuid;

/// The shop's item-set panel truncates longer block titles.
pub const MAX_BLOCK_TITLE: usize = 30;
const UID_NAMESPACE: Uuid = Uuid::from_bytes([
    0x6f, 0x1c, 0x3d, 0x8e, 0x0a, 0x2b, 0x4c, 0x5d, 0x9e, 0x7f, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab,
]);

pub fn title(champion: &str) -> String {
    crate::brand::loadout_name(champion, None)
}

fn clip(title: &str) -> String {
    if title.chars().count() <= MAX_BLOCK_TITLE {
        title.to_string()
    } else {
        title.chars().take(MAX_BLOCK_TITLE - 1).collect::<String>() + "…"
    }
}

fn block(title: &str, ids: &[u32]) -> Value {
    // Merge duplicates into counts (Dagger x2) while keeping first-seen order.
    let mut order: Vec<u32> = Vec::new();
    let mut counts: BTreeMap<u32, u32> = BTreeMap::new();
    for &id in ids {
        if !counts.contains_key(&id) {
            order.push(id);
        }
        *counts.entry(id).or_insert(0) += 1;
    }
    json!({
        "type": clip(title),
        "hideIfSummonerSpell": "",
        "showIfSummonerSpell": "",
        "items": order.iter().map(|id| json!({"id": id.to_string(), "count": counts[id]})).collect::<Vec<_>>()
    })
}

fn ids(cat: &Catalog, names: &[String]) -> Vec<u32> {
    names.iter().filter_map(|n| cat.item_id(n)).collect()
}

pub fn build(plan: &Plan, _pack: Option<&ChampionPack>, cat: &Catalog, champion_key: u32) -> Value {
    let mut blocks: Vec<Value> = Vec::new();
    // A same-champion fallback is labelled inside the shop too, not only on the panel.
    let start_title = match (&plan.source_position, &plan.source) {
        (Some(source), _) => format!("Start ({source} build)"),
        (None, Some(s)) => format!("Start ({})", s.split(',').next().unwrap_or("").trim()),
        (None, None) => "Start".to_string(),
    };
    blocks.push(block(
        &start_title,
        &plan.start.iter().map(|i| i.id).collect::<Vec<_>>(),
    ));
    for (n, item) in plan.path.iter().enumerate() {
        let comps = cat.components(item.id);
        let mut list = comps;
        list.push(item.id);
        let heading = match &item.tag {
            Some(tag) => {
                let full = format!("{}. {} ({})", n + 1, item.name, tag);
                if full.chars().count() <= MAX_BLOCK_TITLE {
                    full
                } else {
                    format!("{}. {} ({})", n + 1, item.short, tag)
                }
            }
            None => format!("{}. {}", n + 1, item.name),
        };
        blocks.push(block(&heading, &list));
    }
    let full_title = match (&plan.source_position, &plan.position) {
        (Some(source), Some(actual)) => format!("Full build: {source} data, {actual}"),
        _ => "Full build, in order".to_string(),
    };
    blocks.push(block(
        &full_title,
        &plan.path.iter().map(|i| i.id).collect::<Vec<_>>(),
    ));
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    if !plan.options.is_empty() {
        blocks.push(block(
            "Alternatives, not extra slots",
            &plan.options.iter().map(|i| i.id).collect::<Vec<_>>(),
        ));
    }
    blocks.push(block(
        "Vision",
        &ids(
            cat,
            &s(&["Control Ward", "Farsight Alteration", "Oracle Lens"]),
        ),
    ));

    let title = title(&plan.champion);
    json!({
        "uid": Uuid::new_v5(&UID_NAMESPACE, title.as_bytes()).to_string(),
        "title": title,
        "mode": "any",
        "map": "any",
        "type": "custom",
        "sortrank": 0,
        "startedFrom": "blank",
        "associatedChampions": [champion_key],
        "associatedMaps": [11],
        "blocks": blocks,
        "preferredItemSlots": []
    })
}

/// Swiftplay keeps role-specific sets even when both choices use one champion.
pub fn build_for_role(
    plan: &Plan,
    pack: Option<&ChampionPack>,
    cat: &Catalog,
    champion_key: u32,
) -> anyhow::Result<Value> {
    let role = plan
        .position
        .as_deref()
        .and_then(crate::aggregate::Position::parse)
        .ok_or_else(|| anyhow::anyhow!("role-specific item set requires a known role"))?;
    let title = format!("{} {}", title(&plan.champion), role.label());
    let mut set = build(plan, pack, cat, champion_key);
    set["uid"] = json!(Uuid::new_v5(&UID_NAMESPACE, title.as_bytes()).to_string());
    set["title"] = json!(title);
    Ok(set)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// New payload for PUT: existing sets kept, any set with the same title replaced.
pub fn upsert(payload: &Value, set: Value) -> Value {
    let title = set
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    // A set of the same title is replaced, and so is the one an older build wrote under the
    // project's previous name, so the shop never shows two tabs for one loadout.
    let legacy = crate::brand::legacy_name(&title);
    let mut sets: Vec<Value> = payload
        .get("itemSets")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|s| {
                    let existing = s.get("title").and_then(Value::as_str);
                    existing != Some(title.as_str()) && existing != legacy.as_deref()
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    sets.push(set);
    let mut out = payload.clone();
    if !out.is_object() {
        out = json!({});
    }
    out["itemSets"] = Value::Array(sets);
    out["timestamp"] = json!(now_ms());
    out
}

/// Every item-set write uploads the whole collection, and the client refuses a request body above
/// 64 KiB (HTTP 413 "Content length is too large"). Stay under that with room to spare.
pub const UPLOAD_BUDGET: usize = 60 * 1024;

/// [`upsert`] each set, then keep the collection within one upload: drop our own older sets, oldest
/// first (upsert appends, so the front is the oldest), never one written now and never a set we did
/// not make. Our sets are rewritten whenever that champion comes up again, so nothing is lost.
/// Errors, before any request, when the player's own sets alone are too large.
pub fn merge(payload: &Value, sets: &[Value]) -> Result<Value, String> {
    let mut out = sets
        .iter()
        .fold(payload.clone(), |out, set| upsert(&out, set.clone()));
    let written: Vec<&str> = sets
        .iter()
        .filter_map(|set| set.get("title").and_then(Value::as_str))
        .collect();
    loop {
        let size = serde_json::to_vec(&out).map_or(usize::MAX, |body| body.len());
        if size <= UPLOAD_BUDGET {
            return Ok(out);
        }
        let Some(list) = out.get_mut("itemSets").and_then(Value::as_array_mut) else {
            return Err("Item sets could not be read safely".into());
        };
        let oldest = list.iter().position(|set| {
            set.get("title")
                .and_then(Value::as_str)
                .is_some_and(|title| crate::brand::owns(title) && !written.contains(&title))
        });
        match oldest {
            Some(index) => {
                list.remove(index);
            }
            None => {
                return Err(format!(
                    "Your item sets fill the client's limit ({} KB); remove some of yours in the client",
                    size / 1024
                ))
            }
        }
    }
}

pub fn remove(payload: &Value, title: &str) -> Value {
    let sets: Vec<Value> = payload
        .get("itemSets")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|s| s.get("title").and_then(Value::as_str) != Some(title))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let mut out = payload.clone();
    out["itemSets"] = Value::Array(sets);
    out["timestamp"] = json!(now_ms());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ddragon::test_support::catalog;
    use crate::engine::{plan, Inputs};
    use crate::pack::{load_traits, load_xayah};

    /// A set about the size of a real one (ten blocks, about 2 KB). The account that hit the
    /// upload limit on 2026-09-26 held 30 sets of this size.
    fn sized_set(title: &str) -> Value {
        let blocks: Vec<Value> = (0..10)
            .map(|block| {
                json!({
                    "type": format!("{title} block {block} with a reason line"),
                    "items": (0..6).map(|item| json!({"id": format!("{}", 3000 + item), "count": 1})).collect::<Vec<_>>()
                })
            })
            .collect();
        json!({"uid": format!("uid-{title}"), "title": title, "type": "custom", "map": "any",
               "mode": "any", "priority": false, "sortrank": 0, "associatedChampions": [498],
               "associatedMaps": [11, 12], "blocks": blocks})
    }

    fn titles(payload: &Value) -> Vec<String> {
        payload["itemSets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|set| set["title"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn a_full_collection_drops_our_oldest_sets_and_keeps_everything_else() {
        let mut sets = vec![sized_set("OP.GG Xayah")];
        sets.extend((0..40).map(|n| sized_set(&format!("Recall Champion{n} Top"))));
        sets.push(sized_set("Featherstorm Irelia Jungle"));
        let payload = json!({"accountId": 1, "timestamp": 0, "itemSets": sets});
        assert!(serde_json::to_vec(&payload).unwrap().len() > UPLOAD_BUDGET);
        let out = merge(&payload, &[sized_set("Recall Malphite")]).unwrap();
        assert!(serde_json::to_vec(&out).unwrap().len() <= UPLOAD_BUDGET);
        let kept = titles(&out);
        assert_eq!(kept.first().map(String::as_str), Some("OP.GG Xayah"));
        assert_eq!(kept.last().map(String::as_str), Some("Recall Malphite"));
        assert!(kept.contains(&"Featherstorm Irelia Jungle".to_string()));
        assert!(!kept.contains(&"Recall Champion0 Top".to_string()));
        assert!(kept.contains(&"Recall Champion39 Top".to_string()));
        assert_eq!(out["accountId"], 1);
    }

    #[test]
    fn both_swiftplay_sets_survive_trimming_and_a_small_collection_is_untouched() {
        let sets: Vec<Value> = (0..40)
            .map(|n| sized_set(&format!("Recall Champion{n} Top")))
            .collect();
        let payload = json!({"itemSets": sets});
        let written = [sized_set("Recall Xayah ADC"), sized_set("Recall Zed Mid")];
        let kept = titles(&merge(&payload, &written).unwrap());
        assert!(kept.ends_with(&["Recall Xayah ADC".to_string(), "Recall Zed Mid".to_string()]));
        let small = json!({"itemSets": [sized_set("Mine"), sized_set("Recall Ahri")]});
        assert_eq!(
            titles(&merge(&small, &[sized_set("Recall Ahri")]).unwrap()),
            ["Mine", "Recall Ahri"]
        );
    }

    #[test]
    fn only_personal_sets_over_the_limit_is_an_error_before_any_upload() {
        let sets: Vec<Value> = (0..40).map(|n| sized_set(&format!("Mine {n}"))).collect();
        let error = merge(&json!({"itemSets": sets}), &[sized_set("Recall Malphite")]).unwrap_err();
        assert!(error.contains("remove some of yours"), "{error}");
    }

    #[test]
    fn upsert_replaces_the_set_an_older_build_wrote_under_the_previous_name() {
        let payload = json!({"itemSets": [
            {"uid": "a", "title": "Featherstorm Xayah"},
            {"uid": "b", "title": "OP.GG Xayah"}
        ]});
        let out = upsert(&payload, json!({"uid": "c", "title": "Recall Xayah"}));
        let titles: Vec<&str> = out["itemSets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["title"].as_str().unwrap())
            .collect();
        assert_eq!(titles, ["OP.GG Xayah", "Recall Xayah"]);
    }

    #[test]
    fn role_specific_itemsets_keep_both_choices_and_foreign_sets_on_reimport() {
        let cat = catalog();
        let mut plan = Plan {
            champion: "Xayah".into(),
            position: Some("ADC".into()),
            ..Default::default()
        };
        let adc = build_for_role(&plan, None, &cat, 498).unwrap();
        plan.position = Some("Mid".into());
        let mid = build_for_role(&plan, None, &cat, 498).unwrap();
        assert_ne!(adc["uid"], mid["uid"]);
        assert_eq!(adc["title"], "Recall Xayah ADC");
        assert_eq!(mid["title"], "Recall Xayah Mid");
        let original = json!({"accountId":42,"itemSets":[{"uid":"foreign","title":"My build","blocks":[{"type":"Keep"}]}]});
        let merged = upsert(&upsert(&upsert(&original, adc.clone()), mid), adc);
        let sets = merged["itemSets"].as_array().unwrap();
        assert_eq!(sets.len(), 3);
        assert_eq!(
            sets[0],
            json!({"uid":"foreign","title":"My build","blocks":[{"type":"Keep"}]})
        );
        assert_eq!(merged["accountId"], 42);
        for set in &sets[1..] {
            assert_eq!(set["associatedChampions"], json!([498]));
            assert_eq!(set["associatedMaps"], json!([11]));
        }
        assert_eq!(build(&plan, None, &cat, 498)["title"], "Recall Xayah");
    }

    #[test]
    fn role_itemset_identity_is_stable_for_equivalent_role_names() {
        let cat = catalog();
        let mut plan = Plan {
            champion: "Xayah".into(),
            position: Some("BOTTOM".into()),
            ..Default::default()
        };
        let bottom = build_for_role(&plan, None, &cat, 498).unwrap();
        plan.position = Some("ADC".into());
        let adc = build_for_role(&plan, None, &cat, 498).unwrap();
        assert_eq!(bottom["uid"], adc["uid"]);
        assert_eq!(bottom["title"], "Recall Xayah ADC");
    }

    #[test]
    fn role_itemsets_reject_missing_or_unknown_roles() {
        let cat = catalog();
        for role in [None, Some(""), Some("FILL")] {
            let plan = Plan {
                champion: "Xayah".into(),
                position: role.map(str::to_owned),
                ..Default::default()
            };
            assert!(build_for_role(&plan, None, &cat, 498).is_err());
        }
    }

    #[test]
    fn builds_a_valid_set_and_upserts_idempotently() {
        let (cat, pack, traits) = (catalog(), load_xayah().unwrap(), load_traits().unwrap());
        let enemies = vec!["Soraka".to_string()];
        let raw = serde_json::from_str(include_str!(
            "../../../m0/tests/fixtures/opgg_xayah_adc.json"
        ))
        .unwrap();
        let aggregate = crate::aggregate::decode(
            &raw,
            498,
            crate::aggregate::Position::Adc,
            "global",
            "emerald_plus",
        )
        .unwrap();
        let plan = plan(&Inputs {
            champion: "Xayah",
            pack: Some(&pack),
            aggregate: Some(&aggregate),
            traits: &traits,
            catalog: &cat,
            enemies: &enemies,
            live: None,
        });
        let set = build(&plan, Some(&pack), &cat, 498);
        assert_eq!(set["title"], "Recall Xayah");
        assert_eq!(set["associatedChampions"], json!([498]));
        let blocks = set["blocks"].as_array().unwrap();
        assert!(blocks.len() >= 10);
        for b in blocks {
            assert!(
                b["type"].as_str().unwrap().chars().count() <= MAX_BLOCK_TITLE,
                "{}",
                b["type"]
            );
        }
        assert_eq!(blocks[1]["type"], "1. Yun Tal Wildarrows");
        assert_eq!(
            blocks[1]["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["1038", "3144", "1036", "3032"]
        );
        let greaves = &blocks[2]["items"];
        assert!(greaves
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["id"] == "1042" && i["count"] == 2));
        // Against Soraka, Mortal Reminder is in the set: on the path, or among the alternatives (Master+
        // Xayah players finish it in 23% of games against healers; engine v3 keeps their usual order).
        assert!(blocks.iter().any(|b| b["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["id"] == "3033")));
        assert!(!blocks.iter().any(|b| b["type"] == "vs lockdown ult"));
        assert_eq!(set["associatedMaps"], json!([11]));

        let existing: Value = serde_json::from_str(include_str!(
            "../../../m0/tests/fixtures/itemsets_existing.json"
        ))
        .unwrap();
        let p1 = upsert(&existing, set.clone());
        assert_eq!(p1["itemSets"].as_array().unwrap().len(), 2);
        let p2 = upsert(&p1, set.clone());
        assert_eq!(p2["itemSets"].as_array().unwrap().len(), 2);
        let p3 = remove(&p2, "Recall Xayah");
        assert_eq!(p3["itemSets"].as_array().unwrap().len(), 1);
        assert_eq!(set["uid"], build(&plan, Some(&pack), &cat, 498)["uid"]);
    }
}
