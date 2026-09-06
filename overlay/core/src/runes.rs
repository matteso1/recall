//! Rune pages and summoner spells: pack names or aggregate ids -> LCU perk page / spell selection.
use crate::aggregate::RunePageIds;
use crate::ddragon::{normalize, Catalog};
use crate::lcu::Lcu;
use crate::pack::RunePage;
use anyhow::{bail, Result};
use serde_json::{json, Value};

/// Stat shards are not in Data Dragon's runesReforged.json.
pub fn shard_id(name: &str) -> Option<u32> {
    Some(match normalize(name).as_str() {
        "adaptiveforce" | "adaptive" => 5008,
        "attackspeed" => 5005,
        "abilityhaste" | "haste" => 5007,
        "movespeed" | "movementspeed" => 5010,
        "healthscaling" | "scalinghealth" => 5001,
        "health" | "flathealth" => 5011,
        "tenacity" | "tenacityandslowresist" => 5013,
        _ => return None,
    })
}

pub fn shard_name(id: u32) -> Option<&'static str> {
    Some(match id {
        5008 => "Adaptive Force",
        5005 => "Attack Speed",
        5007 => "Ability Haste",
        5010 => "Move Speed",
        5001 => "Health Scaling",
        5011 => "Health",
        5013 => "Tenacity and Slow Resist",
        _ => return None,
    })
}

pub const SUMMONER_SPELL_IDS: &[(&str, u64)] = &[
    ("Cleanse", 1), ("Exhaust", 3), ("Flash", 4), ("Ghost", 6), ("Heal", 7),
    ("Smite", 11), ("Teleport", 12), ("Clarity", 13), ("Ignite", 14), ("Barrier", 21),
    ("To the King!", 30), ("Poro Toss", 31), ("Mark", 32),
];
pub const FLASH: u32 = 4;

pub fn spell_id(name: &str) -> Option<u64> {
    let key = normalize(name);
    SUMMONER_SPELL_IDS.iter().find(|(n, _)| normalize(n) == key).map(|(_, id)| *id)
}

pub fn spell_name(id: u32) -> Option<&'static str> {
    SUMMONER_SPELL_IDS.iter().find(|(_, i)| *i == id as u64).map(|(n, _)| *n)
}

/// The two spells to set, keeping Flash on the key the player has it on now (D or F).
pub fn order_spells(picked: &[u32], current: Option<(u32, u32)>) -> Option<(u32, u32)> {
    if picked.len() != 2 {
        return None;
    }
    let (a, b) = (picked[0], picked[1]);
    match current {
        Some((_, f)) if f == FLASH && a == FLASH => Some((b, a)),
        Some((d, _)) if d == FLASH && b == FLASH => Some((b, a)),
        _ => Some((a, b)),
    }
}

/// `{name, primaryStyleId, subStyleId, selectedPerkIds, current}` from resolved ids.
pub fn page_value(page: &RunePageIds, name: &str) -> Value {
    json!({
        "name": name,
        "primaryStyleId": page.primary_style,
        "subStyleId": page.sub_style,
        "selectedPerkIds": page.perks,
        "current": true
    })
}

/// Resolve a pack page (names) to ids: keystone, 3 primary, 2 secondary, 3 shards.
pub fn page_ids(page: &RunePage, cat: &Catalog) -> Result<RunePageIds> {
    let mut missing: Vec<String> = Vec::new();
    let mut perks: Vec<u32> = Vec::new();
    let mut rune = |n: &str| match cat.rune_id(n) {
        Some(id) => perks.push(id),
        None => missing.push(n.to_string()),
    };
    rune(&page.keystone);
    for p in &page.primary_perks {
        rune(p);
    }
    for p in &page.secondary_perks {
        rune(p);
    }
    for s in &page.shards {
        match shard_id(s) {
            Some(id) => perks.push(id),
            None => missing.push(s.to_string()),
        }
    }
    let primary = cat.style_id(&page.primary);
    let secondary = cat.style_id(&page.secondary);
    if primary.is_none() {
        missing.push(page.primary.clone());
    }
    if secondary.is_none() {
        missing.push(page.secondary.clone());
    }
    if !missing.is_empty() {
        bail!("unknown rune names for this patch: {}", missing.join(", "));
    }
    if perks.len() != 9 {
        bail!("a rune page needs 9 perks (keystone + 3 + 2 + 3 shards), got {}", perks.len());
    }
    Ok(RunePageIds { primary_style: primary.unwrap(), sub_style: secondary.unwrap(), perks, ..Default::default() })
}

/// Pack page (names) -> LCU perk page value.
pub fn build_page(page: &RunePage, cat: &Catalog, name: &str) -> Result<Value> {
    Ok(page_value(&page_ids(page, cat)?, name))
}

/// Replace any earlier Featherstorm page, then create this one and make it current.
pub async fn import(lcu: &Lcu, page: Value) -> Result<String> {
    let name = page.get("name").and_then(Value::as_str).unwrap_or("Featherstorm").to_string();
    let pages = lcu.perk_pages().await?;
    let mut deletable = 0usize;
    for p in pages.as_array().into_iter().flatten() {
        let is_deletable = p.get("isDeletable").and_then(Value::as_bool).unwrap_or(false);
        let pname = p.get("name").and_then(Value::as_str).unwrap_or("");
        if is_deletable {
            if pname.starts_with("Featherstorm") {
                if let Some(id) = p.get("id").and_then(Value::as_u64) {
                    lcu.delete_perk_page(id).await?;
                    continue;
                }
            }
            deletable += 1;
        }
    }
    let owned = lcu
        .perk_inventory()
        .await
        .ok()
        .and_then(|v| v.get("ownedPageCount").and_then(Value::as_u64))
        .unwrap_or(2) as usize;
    if deletable >= owned {
        bail!("all {owned} rune pages are in use; delete one in the client and retry");
    }
    lcu.create_perk_page(&page).await?;
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shards_and_spells() {
        assert_eq!(shard_id("Attack Speed"), Some(5005));
        assert_eq!(shard_id("Adaptive Force"), Some(5008));
        assert_eq!(shard_id("Health"), Some(5011));
        assert_eq!(shard_id("nope"), None);
        assert_eq!(spell_id("Flash"), Some(4));
        assert_eq!(spell_name(21), Some("Barrier"));
        assert_eq!(spell_name(99), None);
        // Flash stays on the player's key.
        assert_eq!(order_spells(&[4, 21], Some((7, 4))), Some((21, 4)));
        assert_eq!(order_spells(&[21, 4], Some((4, 7))), Some((4, 21)));
        assert_eq!(order_spells(&[4, 21], Some((4, 7))), Some((4, 21)));
        assert_eq!(order_spells(&[4, 21], None), Some((4, 21)));
        assert_eq!(order_spells(&[4], None), None);
        let page = page_value(&RunePageIds { primary_style: 8000, sub_style: 8300, perks: vec![8008, 8009, 9103, 8014, 8304, 8345, 5005, 5008, 5001], ..Default::default() }, "Featherstorm Xayah");
        assert_eq!(page["primaryStyleId"], 8000);
        assert_eq!(page["selectedPerkIds"].as_array().unwrap().len(), 9);
        assert_eq!(spell_id("heal"), Some(7));
    }
}
