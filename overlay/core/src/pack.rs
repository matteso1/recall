//! The hand-curated data pack: per-champion build spec and champion trait tags.
//! Everything is by *name*; ids are resolved through the Data Dragon catalog at runtime.
use crate::ddragon::normalize;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const XAYAH_JSON: &str = include_str!("../../../data/pack/xayah.json");
pub const TRAITS_JSON: &str = include_str!("../../../data/pack/champion_traits.json");

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CoreItem {
    pub item: String,
    #[serde(default)]
    pub short: Option<String>,
    /// damage | boots | armor_pen | defensive
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub why: Option<String>,
    /// Purchase order of components; defaults to the Data Dragon recipe order.
    #[serde(default)]
    pub components: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub struct SkillOrder {
    /// Levels 1..n, e.g. ["Q", "E", "W", "E"]
    pub first: Vec<String>,
    /// Max order after that, e.g. ["E", "W", "Q"]; R is taken at 6/11/16
    pub max: Vec<String>,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub struct RunePage {
    pub name: String,
    pub primary: String,
    pub keystone: String,
    pub primary_perks: Vec<String>,
    pub secondary: String,
    pub secondary_perks: Vec<String>,
    pub shards: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub struct Alternatives {
    pub anti_heal: String,
    pub armor_pen: String,
    pub cleanse: String,
    pub defensive_ad: String,
    pub defensive_ap: String,
    pub anti_burst: String,
    pub sustain: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub struct Matchup {
    pub line: String,
    #[serde(default)]
    pub first_item: Option<String>,
    #[serde(default)]
    pub start: Option<Vec<String>>,
    #[serde(default)]
    pub spells: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChampionPack {
    pub champion: String,
    pub role: String,
    pub start: Vec<String>,
    pub core: Vec<CoreItem>,
    pub skill_order: SkillOrder,
    pub runes: RunePage,
    pub spells: Vec<String>,
    pub alternatives: Alternatives,
    #[serde(default)]
    pub matchups: HashMap<String, Matchup>,
    /// Short labels for items that may enter the path through rules
    #[serde(default)]
    pub shorts: HashMap<String, String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

impl ChampionPack {
    pub fn matchup(&self, enemy: &str) -> Option<&Matchup> {
        let key = normalize(enemy);
        self.matchups.iter().find(|(k, _)| normalize(k) == key).map(|(_, m)| m)
    }

    /// The pack's own short label for an item, if it has one.
    pub fn short_opt(&self, item: &str) -> Option<String> {
        let key = normalize(item);
        if let Some(c) = self.core.iter().find(|c| normalize(&c.item) == key) {
            if let Some(s) = &c.short {
                return Some(s.clone());
            }
        }
        self.shorts.iter().find(|(k, _)| normalize(k) == key).map(|(_, s)| s.clone())
    }

    pub fn short(&self, item: &str) -> String {
        self.short_opt(item).unwrap_or_else(|| item.split_whitespace().next().unwrap_or(item).to_string())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub struct ChampTraits {
    #[serde(default)]
    pub healing: bool,
    #[serde(default)]
    pub shielding: bool,
    #[serde(default)]
    pub tank: bool,
    #[serde(default)]
    pub lockdown_ult: bool,
    #[serde(default)]
    pub assassin: bool,
    #[serde(default)]
    pub burst: bool,
    #[serde(default)]
    pub poke: bool,
    /// ad | ap | mixed
    #[serde(default)]
    pub damage: String,
    #[serde(default)]
    pub roles: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub struct Traits {
    pub champions: HashMap<String, ChampTraits>,
}

impl Traits {
    pub fn get(&self, champion: &str) -> Option<&ChampTraits> {
        let key = normalize(champion);
        self.champions.iter().find(|(k, _)| normalize(k) == key).map(|(_, t)| t)
    }
}

pub fn load_xayah() -> Result<ChampionPack> {
    serde_json::from_str(XAYAH_JSON).context("data/pack/xayah.json")
}

pub fn load_traits() -> Result<Traits> {
    serde_json::from_str(TRAITS_JSON).context("data/pack/champion_traits.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_files_parse() {
        let pack = load_xayah().unwrap();
        assert_eq!(pack.champion, "Xayah");
        assert_eq!(pack.core.len(), 6);
        assert_eq!(pack.short("Essence Reaver"), "ER");
        assert_eq!(pack.short("Mortal Reminder"), "Mortal");
        assert!(pack.matchup("tristana").is_some());
        let traits = load_traits().unwrap();
        assert!(traits.get("Soraka").unwrap().healing);
        assert!(traits.get("ornn").unwrap().tank);
        assert!(traits.get("Malzahar").unwrap().lockdown_ult);
    }
}
