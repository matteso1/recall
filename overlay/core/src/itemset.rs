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
    format!("Featherstorm {champion}")
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

pub fn build(plan: &Plan, pack: Option<&ChampionPack>, cat: &Catalog, champion_key: u32) -> Value {
    let mut blocks: Vec<Value> = Vec::new();
    let start_title = match &plan.source {
        Some(s) => format!("Start ({})", s.split(',').next().unwrap_or("").trim()),
        None => "Start".to_string(),
    };
    blocks.push(block(&start_title, &plan.start.iter().map(|i| i.id).collect::<Vec<_>>()));
    for (n, item) in plan.path.iter().enumerate() {
        let comps = pack
            .and_then(|p| p.core.iter().find(|c| crate::ddragon::normalize(&c.item) == crate::ddragon::normalize(&item.name)))
            .and_then(|c| c.components.as_ref())
            .map(|names| ids(cat, names))
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| cat.components(item.id));
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
    blocks.push(block("Full build, in order", &plan.path.iter().map(|i| i.id).collect::<Vec<_>>()));
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    if !plan.options.is_empty() {
        blocks.push(block("Other popular items", &plan.options.iter().map(|i| i.id).collect::<Vec<_>>()));
    }
    if let Some(p) = pack {
        let alt = &p.alternatives;
        blocks.push(block("vs healing", &ids(cat, &s(&[&alt.anti_heal]))));
        blocks.push(block("vs 2+ tanks", &ids(cat, &s(&[&alt.armor_pen]))));
        blocks.push(block("vs lockdown ult", &ids(cat, &s(&[&alt.cleanse]))));
        blocks.push(block("vs burst / assassins", &ids(cat, &s(&[&alt.defensive_ad, &alt.anti_burst, &alt.defensive_ap]))));
        blocks.push(block("vs long fights", &ids(cat, &s(&[&alt.sustain]))));
    }
    blocks.push(block("Vision", &ids(cat, &s(&["Control Ward", "Farsight Alteration", "Oracle Lens"]))));

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
        "associatedMaps": [],
        "blocks": blocks,
        "preferredItemSlots": []
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// New payload for PUT: existing sets kept, any set with the same title replaced.
pub fn upsert(payload: &Value, set: Value) -> Value {
    let title = set.get("title").and_then(Value::as_str).unwrap_or("").to_string();
    let mut sets: Vec<Value> = payload
        .get("itemSets")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter(|s| s.get("title").and_then(Value::as_str) != Some(title.as_str())).cloned().collect())
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

pub fn remove(payload: &Value, title: &str) -> Value {
    let sets: Vec<Value> = payload
        .get("itemSets")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter(|s| s.get("title").and_then(Value::as_str) != Some(title)).cloned().collect())
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

    #[test]
    fn builds_a_valid_set_and_upserts_idempotently() {
        let (cat, pack, traits) = (catalog(), load_xayah().unwrap(), load_traits().unwrap());
        let enemies = vec!["Soraka".to_string()];
        let plan = plan(&Inputs { champion: "Xayah", pack: Some(&pack), aggregate: None, traits: &traits, catalog: &cat, enemies: &enemies, live: None });
        let set = build(&plan, Some(&pack), &cat, 498);
        assert_eq!(set["title"], "Featherstorm Xayah");
        assert_eq!(set["associatedChampions"], json!([498]));
        let blocks = set["blocks"].as_array().unwrap();
        assert!(blocks.len() >= 10);
        for b in blocks {
            assert!(b["type"].as_str().unwrap().chars().count() <= MAX_BLOCK_TITLE, "{}", b["type"]);
        }
        assert_eq!(blocks[1]["type"], "1. Essence Reaver");
        assert_eq!(blocks[1]["items"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap()).collect::<Vec<_>>(),
                   vec!["3057", "3133", "1018", "3508"]);
        let greaves = &blocks[2]["items"];
        assert!(greaves.as_array().unwrap().iter().any(|i| i["id"] == "1042" && i["count"] == 2));
        assert!(blocks.iter().any(|b| b["type"].as_str().unwrap().starts_with("5. Mortal Reminder (Soraka")));

        let existing: Value = serde_json::from_str(include_str!("../../../m0/tests/fixtures/itemsets_existing.json")).unwrap();
        let p1 = upsert(&existing, set.clone());
        assert_eq!(p1["itemSets"].as_array().unwrap().len(), 2);
        let p2 = upsert(&p1, set.clone());
        assert_eq!(p2["itemSets"].as_array().unwrap().len(), 2);
        let p3 = remove(&p2, "Featherstorm Xayah");
        assert_eq!(p3["itemSets"].as_array().unwrap().len(), 1);
        assert_eq!(set["uid"], build(&plan, Some(&pack), &cat, 498)["uid"]);
    }
}
