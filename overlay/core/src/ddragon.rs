//! Data Dragon catalog: items, champions and runes by id and by *name*, cached per patch.
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, SystemTime};

pub const BASE: &str = "https://ddragon.leagueoflegends.com";
const SR_MAP: &str = "11";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Item {
    pub id: u32,
    pub name: String,
    pub total: u32,
    pub base: u32,
    pub from: Vec<u32>,
    pub into: Vec<u32>,
    pub on_sr: bool,
    pub purchasable: bool,
    pub in_store: bool,
    pub tags: Vec<String>,
    /// Explicit purchase-group membership, when supplied by the catalog.
    pub group_ids: Vec<String>,
    pub depth: u32,
    pub description: String,
    pub stats: HashMap<String, f64>,
    pub effects: ItemEffects,
    /// Both total and combine/base prices were present and valid in Data Dragon.
    pub price_known: bool,
    pub required_champion: Option<String>,
    pub required_ally: Option<String>,
    pub max_stacks: u32,
    /// Automatic quest/stack transformation predecessor, not a shop recipe.
    pub special_recipe: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanseEffect {
    /// Describes the item, not whether a particular champion effect is removable.
    CrowdControlExceptAirborne,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShieldEffect {
    Magic,
    AllDamage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrievousTrigger {
    PhysicalDamage,
    MagicDamage,
    AnyDamage,
    WhenAttacked,
}

/// A deliberately narrow description parser. Fractions use 0..1.
/// `None` means no verified value; it must not be interpreted as proof of no effect.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ItemEffects {
    pub grievous_wounds: Option<f64>,
    pub grievous_trigger: Option<GrievousTrigger>,
    pub percent_armor_pen: Option<f64>,
    pub flat_armor_pen: Option<f64>,
    pub percent_magic_pen: Option<f64>,
    pub flat_magic_pen: Option<f64>,
    pub armor: Option<f64>,
    pub magic_resist: Option<f64>,
    pub life_steal: Option<f64>,
    pub omnivamp: Option<f64>,
    pub cleanse: Option<CleanseEffect>,
    pub shield: Option<ShieldEffect>,
    pub spell_shield: bool,
    pub stasis: bool,
    pub boots: bool,
    pub unknown_passives: bool,
}

impl Default for ItemEffects {
    fn default() -> Self {
        Self {
            grievous_wounds: None,
            grievous_trigger: None,
            percent_armor_pen: None,
            flat_armor_pen: None,
            percent_magic_pen: None,
            flat_magic_pen: None,
            armor: None,
            magic_resist: None,
            life_steal: None,
            omnivamp: None,
            cleanse: None,
            shield: None,
            spell_shield: false,
            stasis: false,
            boots: false,
            unknown_passives: true,
        }
    }
}

impl Item {
    pub fn stat(&self, key: &str) -> Option<f64> {
        self.stats.get(key).copied()
    }

    pub fn is_owned_commitment(&self, cat: &Catalog) -> bool {
        self.is_finished(cat)
            || self
                .exclusive_groups()
                .iter()
                .any(|group| *group == "SupportQuest" || *group == "JungleCompanion")
    }

    pub fn is_finished(&self, cat: &Catalog) -> bool {
        if matches!(self.id, 3869 | 3870 | 3871 | 3876 | 3877) {
            return true;
        }
        if self
            .tags
            .iter()
            .any(|tag| tag == "Consumable" || tag == "Trinket")
        {
            return false;
        }
        if self.effects.boots {
            return self.total > 300;
        }
        // Terminal upgrades can be inexpensive (Mejai's costs 1500). Recipe
        // depth distinguishes them from terminal basic items and lane starters.
        // Automatic finished transformations inherit their predecessor's recipe.
        let is_upgrade = !self.from.is_empty()
            || self.depth > 1
            || self.special_recipe.is_some_and(|id| {
                cat.item(id)
                    .is_some_and(|previous| !previous.from.is_empty() || previous.depth > 1)
            });
        is_upgrade
            && self.into.iter().all(|id| {
                cat.item(*id).is_none_or(|upgrade| {
                    !upgrade.on_sr
                        || !upgrade.purchasable
                        || !upgrade.in_store
                        || upgrade.base == 0
                        || upgrade.required_ally.is_some()
                        || upgrade.required_champion.is_some()
                })
            })
    }

    /// Verified restriction families used by the supported itemization rules.
    /// Data Dragon publishes group limits but omits item-to-group membership.
    pub fn exclusive_groups(&self) -> Vec<&'static str> {
        let mut groups = Vec::new();
        // The current catalog supplies the DoransItems limit, but not each
        // item's membership. Its Doran names and Lane tags identify this family,
        // including newly added starters without a champion-specific rule.
        let doran = normalize(&self.name).starts_with("dorans")
            && self.tags.iter().any(|tag| tag == "Lane");
        let jungle_companion = self
            .description
            .contains("<passive>Jungle Companions</passive>");
        if doran {
            groups.push("DoransItems");
        }
        // Riot's 14.6 starter restriction excludes Doran, unfinished support
        // quests (Atlas/Compass), and jungle eggs from each other. Bounty of
        // Worlds and completed support choices no longer have that restriction.
        // This local family name is not an inferred Data Dragon group mapping.
        // https://www.leagueoflegends.com/en-us/news/game-updates/patch-14-6-notes/
        if doran || matches!(self.id, 3865 | 3866) || jungle_companion {
            groups.push("StartingItems");
        }
        if self.effects.boots {
            groups.push("Boots");
        }
        if self.effects.percent_armor_pen.is_some() || self.from.contains(&3035) {
            groups.push("LastWhisper");
        }
        if self.effects.percent_magic_pen.is_some() || self.from.contains(&4630) {
            groups.push("VoidPen");
        }
        if self.description.contains("<passive>Lifeline</passive>") {
            groups.push("Lifeline");
        }
        if self.effects.cleanse.is_some() {
            groups.push("Quicksilver");
        }
        if self.description.contains("<passive>Spellblade</passive>") {
            groups.push("Spellblade");
        }
        for (passive, group) in [
            ("Immolate", "Immolate"),
            ("Cleave", "Cleave"),
            ("Thorns", "Thornmail"),
        ] {
            if self
                .description
                .contains(&format!("<passive>{passive}</passive>"))
            {
                groups.push(group);
            }
        }
        if self.effects.spell_shield {
            groups.push("Spellshield");
        }
        if matches!(
            self.id,
            3865 | 3866 | 3867 | 3869 | 3870 | 3871 | 3876 | 3877
        ) {
            groups.push("SupportQuest");
        }
        if jungle_companion {
            groups.push("JungleCompanion");
        }
        if matches!(self.id, 3070 | 3003 | 3004 | 3040 | 3042 | 3119 | 3121) {
            groups.push("Tear");
        }
        groups
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Champion {
    pub key: u32,
    /// Data Dragon id, e.g. "MonkeyKing"
    pub id: String,
    /// Display name, e.g. "Wukong"
    pub name: String,
    /// Data Dragon class tags: Marksman, Support, Tank, Mage, Assassin, Fighter
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Rune {
    pub id: u32,
    pub name: String,
    pub style: u32,
    pub slot: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub version: String,
    pub items: HashMap<u32, Item>,
    /// Data Dragon MaxGroupOwnable values. -1 means no limit.
    pub group_limits: HashMap<String, i32>,
    pub champions: HashMap<u32, Champion>,
    pub runes: HashMap<u32, Rune>,
    /// Style (tree) name -> id, e.g. Precision -> 8000
    pub styles: HashMap<String, u32>,
    /// Style id -> display name
    pub style_names: HashMap<u32, String>,
    item_by_name: HashMap<String, u32>,
    champ_by_name: HashMap<String, u32>,
    rune_by_name: HashMap<String, u32>,
}

/// "B. F. Sword" == "B.F. Sword" == "bf sword" -> "bfsword"
pub fn normalize(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

fn u32_of(v: &Value, key: &str) -> u32 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0) as u32
}

fn ids_of(v: &Value, key: &str) -> Vec<u32> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().and_then(|s| s.parse().ok()))
                .collect()
        })
        .unwrap_or_default()
}

fn strings_of(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn price_of(v: &Value, key: &str) -> Option<u32> {
    v.get(key)
        .and_then(Value::as_u64)
        .and_then(|n| n.try_into().ok())
}

fn description_text(html: &str) -> String {
    let mut output = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                output.push(' ');
            }
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
}

fn described_number(text: &str, label: &str, percent: bool) -> Option<f64> {
    text.match_indices(label).find_map(|(index, _)| {
        let token = text[..index].split_whitespace().next_back()?;
        if token.ends_with('%') != percent {
            return None;
        }
        let number = token.trim_end_matches('%').parse::<f64>().ok()?;
        if !number.is_finite() || number < 0.0 || (percent && number > 100.0) {
            return None;
        }
        Some(if percent { number / 100.0 } else { number })
    })
}

fn item_effects(item: &Item, stats_text: &str) -> ItemEffects {
    let text = description_text(&item.description);
    let normalized_text = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let remainder = item
        .description
        .split_once("</stats>")
        .map(|(_, rest)| rest);
    let mut effects = ItemEffects {
        armor: item.stat("FlatArmorMod"),
        magic_resist: item.stat("FlatSpellBlockMod"),
        life_steal: item.stat("PercentLifeStealMod"),
        // Only unconditional stats are represented here. Maw's conditional
        // combat Omnivamp deliberately remains part of its unmodeled text.
        omnivamp: described_number(stats_text, "Omnivamp", true),
        percent_armor_pen: described_number(stats_text, "Armor Penetration", true),
        flat_armor_pen: described_number(stats_text, "Lethality", false)
            .or_else(|| described_number(stats_text, "Armor Penetration", false)),
        percent_magic_pen: described_number(stats_text, "Magic Penetration", true),
        flat_magic_pen: described_number(stats_text, "Magic Penetration", false),
        spell_shield: normalized_text.contains("spell shield that blocks the next enemy ability"),
        stasis: item.description.contains("<active>Time Stop</active>")
            && item.description.contains("<keyword>Stasis</keyword>"),
        boots: item.tags.iter().any(|tag| tag == "Boots"),
        unknown_passives: remainder.is_none_or(|rest| !description_text(rest).trim().is_empty()),
        ..Default::default()
    };
    if normalized_text.contains("wounds") {
        effects.grievous_wounds = described_number(&normalized_text, "wounds", true)
            .or_else(|| described_number(&normalized_text, "grievous wounds", true))
            .filter(|value| *value > 0.0 && *value <= 1.0);
        if effects.grievous_wounds.is_some() {
            effects.grievous_trigger = if normalized_text.contains("when struck by an attack")
                || normalized_text.contains("when hit by an attack")
            {
                Some(GrievousTrigger::WhenAttacked)
            } else if normalized_text.contains("physical damage") {
                Some(GrievousTrigger::PhysicalDamage)
            } else if normalized_text.contains("magic damage") {
                Some(GrievousTrigger::MagicDamage)
            } else if normalized_text.contains("dealing damage")
                || normalized_text.contains("dealing any damage")
            {
                Some(GrievousTrigger::AnyDamage)
            } else {
                None
            };
        }
    }
    if matches!(item.id, 3140 | 3139)
        && text.contains("Quicksilver")
        && text.contains("crowd control debuffs")
        && text.contains("Airborne")
    {
        effects.cleanse = Some(CleanseEffect::CrowdControlExceptAirborne);
    }
    if item
        .description
        .contains("<shield>magic damage Shield</shield>")
        || item.description.contains("<shield>magic shield</shield>")
    {
        effects.shield = Some(ShieldEffect::Magic);
    } else if (item.description.contains("<passive>Lifeline</passive>")
        || item.description.contains("<passive>Ichorshield</passive>"))
        && item.description.contains("<shield>")
    {
        effects.shield = Some(ShieldEffect::AllDamage);
    }
    effects
}

impl Catalog {
    pub fn from_json(version: &str, items: &Value, champions: &Value, runes: &Value) -> Catalog {
        let mut cat = Catalog {
            version: version.to_string(),
            ..Default::default()
        };

        for group in items
            .get("groups")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(id) = group
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
            else {
                continue;
            };
            let Some(limit) = group
                .get("MaxGroupOwnable")
                .and_then(|value| {
                    value
                        .as_str()
                        .and_then(|value| value.parse::<i32>().ok())
                        .or_else(|| value.as_i64().and_then(|value| value.try_into().ok()))
                })
                .filter(|limit| *limit >= -1)
            else {
                continue;
            };
            cat.group_limits.insert(id.to_string(), limit);
        }

        if let Some(data) = items.get("data").and_then(Value::as_object) {
            for (id_str, v) in data {
                let Ok(id) = id_str.parse::<u32>() else {
                    continue;
                };
                let gold = v.get("gold").cloned().unwrap_or(Value::Null);
                let mut item = Item {
                    id,
                    name: v
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    total: u32_of(&gold, "total"),
                    base: u32_of(&gold, "base"),
                    from: ids_of(v, "from"),
                    into: ids_of(v, "into"),
                    on_sr: v
                        .get("maps")
                        .and_then(|m| m.get(SR_MAP))
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    purchasable: gold
                        .get("purchasable")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    in_store: v.get("inStore").and_then(Value::as_bool).unwrap_or(true)
                        && !v
                            .get("hideFromAll")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    tags: strings_of(v, "tags"),
                    group_ids: strings_of(v, "groups"),
                    depth: u32_of(v, "depth"),
                    description: v
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    stats: v
                        .get("stats")
                        .and_then(Value::as_object)
                        .map(|stats| {
                            stats
                                .iter()
                                .filter_map(|(key, value)| {
                                    value
                                        .as_f64()
                                        .filter(|number| number.is_finite())
                                        .map(|number| (key.clone(), number))
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    price_known: price_of(&gold, "total").is_some()
                        && price_of(&gold, "base").is_some(),
                    required_champion: v
                        .get("requiredChampion")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    required_ally: v
                        .get("requiredAlly")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    max_stacks: price_of(v, "stacks").unwrap_or(1).max(1),
                    special_recipe: price_of(v, "specialRecipe"),
                    ..Default::default()
                };
                if let Some(group) = v
                    .get("group")
                    .and_then(Value::as_str)
                    .filter(|group| !group.is_empty())
                {
                    item.group_ids.push(group.to_string());
                }
                item.group_ids.retain(|group| !group.is_empty());
                item.group_ids.sort();
                item.group_ids.dedup();
                let stats_text = description_text(
                    item.description
                        .split_once("<stats>")
                        .and_then(|(_, rest)| rest.split_once("</stats>"))
                        .map(|(stats, _)| stats)
                        .unwrap_or(""),
                );
                if let Some(haste) = described_number(&stats_text, "Ability Haste", false) {
                    item.stats
                        .entry("AbilityHaste".to_string())
                        .or_insert(haste);
                }
                item.effects = item_effects(&item, &stats_text);
                let key = normalize(&item.name);
                let better = match cat
                    .item_by_name
                    .get(&key)
                    .and_then(|cur| cat.items.get(cur))
                {
                    None => true,
                    Some(cur) => rank(&item) < rank(cur),
                };
                if better && !item.name.is_empty() {
                    cat.item_by_name.insert(key, id);
                }
                cat.items.insert(id, item);
            }
        }
        // Special boot upgrades occasionally omit the Boots tag. Inherit the
        // category through their recipe instead of trusting a name substring.
        loop {
            let upgrades: Vec<u32> = cat
                .items
                .values()
                .filter(|item| {
                    !item.effects.boots
                        && item
                            .from
                            .iter()
                            .any(|id| cat.item(*id).is_some_and(|base| base.effects.boots))
                })
                .map(|item| item.id)
                .collect();
            if upgrades.is_empty() {
                break;
            }
            for id in upgrades {
                cat.items.get_mut(&id).unwrap().effects.boots = true;
            }
        }

        if let Some(data) = champions.get("data").and_then(Value::as_object) {
            for v in data.values() {
                let Some(key) = v
                    .get("key")
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse::<u32>().ok())
                else {
                    continue;
                };
                let champ = Champion {
                    key,
                    id: v
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    name: v
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    tags: strings_of(v, "tags"),
                };
                cat.champ_by_name.insert(normalize(&champ.name), key);
                cat.champ_by_name.entry(normalize(&champ.id)).or_insert(key);
                cat.champions.insert(key, champ);
            }
        }

        for style in runes.as_array().into_iter().flatten() {
            let style_id = u32_of(style, "id");
            if let Some(name) = style.get("name").and_then(Value::as_str) {
                cat.styles.insert(normalize(name), style_id);
                cat.style_names.insert(style_id, name.to_string());
            }
            for (slot, s) in style
                .get("slots")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                for r in s
                    .get("runes")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let rune = Rune {
                        id: u32_of(r, "id"),
                        name: r
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        style: style_id,
                        slot,
                    };
                    cat.rune_by_name.insert(normalize(&rune.name), rune.id);
                    cat.runes.insert(rune.id, rune);
                }
            }
        }
        cat
    }

    pub fn item(&self, id: u32) -> Option<&Item> {
        self.items.get(&id)
    }

    pub fn item_id(&self, name: &str) -> Option<u32> {
        self.item_by_name.get(&normalize(name)).copied()
    }

    pub fn item_name(&self, id: u32) -> String {
        self.items
            .get(&id)
            .map(|i| i.name.clone())
            .unwrap_or_else(|| format!("item {id}"))
    }

    pub fn item_cost(&self, id: u32) -> u32 {
        self.items.get(&id).map(|i| i.total).unwrap_or(0)
    }

    /// Direct recipe, in Data Dragon order (duplicates preserved).
    pub fn components(&self, id: u32) -> Vec<u32> {
        self.items
            .get(&id)
            .map(|i| i.from.clone())
            .unwrap_or_default()
    }

    pub fn champion(&self, key: u32) -> Option<&Champion> {
        self.champions.get(&key)
    }

    pub fn champion_name(&self, key: u32) -> String {
        self.champions
            .get(&key)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| format!("champ {key}"))
    }

    pub fn champion_key(&self, name: &str) -> Option<u32> {
        self.champ_by_name.get(&normalize(name)).copied()
    }

    pub fn rune_id(&self, name: &str) -> Option<u32> {
        self.rune_by_name.get(&normalize(name)).copied()
    }

    pub fn style_id(&self, name: &str) -> Option<u32> {
        self.styles.get(&normalize(name)).copied()
    }

    pub fn rune_name(&self, id: u32) -> String {
        self.runes
            .get(&id)
            .map(|r| r.name.clone())
            .unwrap_or_else(|| format!("rune {id}"))
    }

    pub fn style_name(&self, id: u32) -> String {
        self.style_names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("style {id}"))
    }
}

/// Among same-named entries prefer the real SR shop item (e.g. 6676 over 667666).
fn rank(item: &Item) -> (u8, u8, u8, usize, u32) {
    (
        u8::from(!item.on_sr),
        u8::from(!item.purchasable),
        u8::from(!item.in_store),
        item.id.to_string().len(),
        item.id,
    )
}

async fn fetch_text(url: &str) -> Result<String> {
    Ok(reqwest::get(url).await?.error_for_status()?.text().await?)
}

fn first_version(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Vec<String>>(&text)
        .ok()?
        .into_iter()
        .next()
}

/// Newest patch on Data Dragon (cached 6 h; falls back to the cache when offline).
pub async fn latest_version(cache_dir: &Path) -> Result<String> {
    let vfile = cache_dir.join("versions.json");
    let fresh = std::fs::metadata(&vfile)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|m| SystemTime::now().duration_since(m).ok())
        .map(|age| age < Duration::from_secs(6 * 3600))
        .unwrap_or(false);
    if fresh {
        if let Some(v) = first_version(&vfile) {
            return Ok(v);
        }
    }
    match fetch_text(&format!("{BASE}/api/versions.json")).await {
        Ok(text) => {
            std::fs::create_dir_all(cache_dir)?;
            std::fs::write(&vfile, &text)?;
            serde_json::from_str::<Vec<String>>(&text)?
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("empty versions.json"))
        }
        Err(e) => first_version(&vfile)
            .ok_or(e)
            .context("Data Dragon unreachable and no cached version"),
    }
}

/// kind: item | champion | runesReforged | summoner
pub async fn load_json(cache_dir: &Path, version: &str, kind: &str) -> Result<Value> {
    let path = cache_dir.join(version).join(format!("{kind}.json"));
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(v) = serde_json::from_str(&text) {
            return Ok(v);
        }
    }
    let text = fetch_text(&format!("{BASE}/cdn/{version}/data/en_US/{kind}.json")).await?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, &text)?;
    Ok(serde_json::from_str(&text)?)
}

pub async fn load(cache_dir: &Path) -> Result<Catalog> {
    let version = latest_version(cache_dir).await?;
    let items = load_json(cache_dir, &version, "item").await?;
    let champions = load_json(cache_dir, &version, "champion").await?;
    let runes = load_json(cache_dir, &version, "runesReforged").await?;
    log::info!("Data Dragon {version} loaded");
    Ok(Catalog::from_json(&version, &items, &champions, &runes))
}

#[cfg(test)]
pub mod test_support {
    use super::*;
    pub const ITEMS: &str = include_str!("../../../m0/tests/fixtures/item_subset.json");
    pub const CHAMPS: &str = include_str!("../../../m0/tests/fixtures/champion_subset.json");

    pub fn catalog() -> Catalog {
        Catalog::from_json(
            "16.17.1",
            &serde_json::from_str(ITEMS).unwrap(),
            &serde_json::from_str(CHAMPS).unwrap(),
            &Value::Array(vec![]),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::catalog;
    use super::*;

    #[test]
    fn names_resolve_with_punctuation_differences() {
        let cat = catalog();
        assert_eq!(normalize("B. F. Sword"), "bfsword");
        assert_eq!(cat.item_id("B.F. Sword"), Some(1038));
        assert_eq!(cat.item_id("dorans blade"), Some(1055));
        assert_eq!(
            cat.item_id("The Collector"),
            Some(6676),
            "real shop item beats 667666"
        );
        assert_eq!(cat.item_id("Sword of Nonexistence"), None);
    }

    #[test]
    fn recipes_and_champions() {
        let cat = catalog();
        assert_eq!(cat.components(3031), vec![1038, 1037, 1018]);
        assert_eq!(cat.item_cost(3031), 3500);
        assert_eq!(cat.champion_key("Xayah"), Some(498));
        assert_eq!(cat.champion_key("MonkeyKing"), Some(62));
        assert_eq!(cat.champion_key("Wukong"), Some(62));
        assert_eq!(cat.champion_name(16), "Soraka");
        assert!(cat.champion(516).unwrap().tags.iter().any(|t| t == "Tank"));
    }

    #[test]
    fn fixture_contains_every_recipe_component() {
        let cat = catalog();
        let mut missing = Vec::new();
        for item in cat.items.values() {
            for id in &item.from {
                if cat.item(*id).is_none() {
                    missing.push((item.id, *id));
                }
            }
        }
        missing.sort_unstable();
        assert!(
            missing.is_empty(),
            "incomplete patch-matched recipes: {missing:?}"
        );
    }

    #[test]
    fn item_effects_distinguish_percent_penetration_from_lethality() {
        let cat = catalog();
        let collector = cat.item(6676).unwrap();
        assert_eq!(collector.effects.flat_armor_pen, Some(10.0));
        assert_eq!(collector.effects.percent_armor_pen, None);
        let dominik = cat.item(3036).unwrap();
        assert_eq!(dominik.effects.percent_armor_pen, Some(0.35));
        assert_eq!(dominik.effects.flat_armor_pen, None);
        assert_eq!(cat.item(3033).unwrap().effects.grievous_wounds, Some(0.4));
        assert_eq!(cat.item(3123).unwrap().effects.grievous_wounds, Some(0.4));
    }

    #[test]
    fn item_effects_retain_verified_defense_sustain_and_cleanse_scope() {
        let cat = catalog();
        let mercurial = cat.item(3139).unwrap();
        assert_eq!(mercurial.stat("FlatPhysicalDamageMod"), Some(50.0));
        assert_eq!(mercurial.effects.magic_resist, Some(35.0));
        assert_eq!(mercurial.effects.life_steal, Some(0.1));
        assert_eq!(
            mercurial.effects.cleanse,
            Some(CleanseEffect::CrowdControlExceptAirborne)
        );
        assert_eq!(
            cat.item(3140).unwrap().effects.cleanse,
            Some(CleanseEffect::CrowdControlExceptAirborne)
        );
        assert_eq!(cat.item(3047).unwrap().effects.armor, Some(25.0));
        assert_eq!(
            cat.item(3156).unwrap().effects.shield,
            Some(ShieldEffect::Magic)
        );
        assert_eq!(
            cat.item(6673).unwrap().effects.shield,
            Some(ShieldEffect::AllDamage)
        );
        assert_eq!(cat.item(6695).unwrap().effects.shield, None);
        // Maw's Omnivamp is conditional; never report it as a permanent stat.
        assert_eq!(cat.item(3156).unwrap().effects.omnivamp, None);
    }

    #[test]
    fn descriptions_and_unmodeled_passives_remain_available() {
        let cat = catalog();
        let essence = cat.item(3508).unwrap();
        assert!(!essence.description.is_empty());
        assert!(essence.effects.unknown_passives);
        assert_eq!(essence.stat("AbilityHaste"), Some(20.0));
        assert!(!cat.item(1036).unwrap().effects.unknown_passives);
    }

    #[test]
    fn finished_items_are_retained_when_they_upgrade_into_special_forms() {
        let cat = catalog();
        assert!(cat.item(3006).unwrap().is_finished(&cat));
        assert!(cat.item(3047).unwrap().is_finished(&cat));
        assert!(cat.item(3004).unwrap().is_finished(&cat));
        assert!(cat.item(3042).unwrap().is_finished(&cat));
        assert!(!cat.item(1001).unwrap().is_finished(&cat));
        assert!(!cat.item(3133).unwrap().is_finished(&cat));
    }

    #[test]
    fn unavailable_price_fields_are_not_silently_valid_zeroes() {
        let valid = serde_json::json!({"data": {"1": {
            "name": "test", "gold": {"base": 0, "total": 0, "purchasable": true},
            "maps": {"11": true}
        }}});
        let cat = Catalog::from_json("test", &valid, &Value::Null, &Value::Null);
        assert!(cat.item(1).unwrap().price_known);
        for value in [
            Value::Null,
            serde_json::json!(-20),
            serde_json::json!(1.5),
            serde_json::json!(u64::MAX),
        ] {
            let mut invalid = valid.clone();
            invalid["data"]["1"]["gold"]["total"] = value;
            let cat = Catalog::from_json("test", &invalid, &Value::Null, &Value::Null);
            assert!(!cat.item(1).unwrap().price_known);
        }
    }

    #[test]
    fn mage_effects_distinguish_magic_penetration_and_verified_defensive_actives() {
        let cat = catalog();
        assert!(
            cat.item(3135).is_some(),
            "the mage fixture must use the same official patch"
        );
        assert_eq!(cat.item(3135).unwrap().effects.percent_magic_pen, Some(0.4));
        assert_eq!(cat.item(3137).unwrap().effects.percent_magic_pen, Some(0.3));
        assert_eq!(cat.item(3020).unwrap().effects.flat_magic_pen, Some(12.0));
        assert_eq!(cat.item(4645).unwrap().effects.flat_magic_pen, Some(15.0));
        assert_eq!(cat.item(3916).unwrap().effects.grievous_wounds, Some(0.4));
        assert_eq!(cat.item(3165).unwrap().effects.grievous_wounds, Some(0.4));
        assert!(cat.item(3102).unwrap().effects.spell_shield);
        assert!(cat.item(3157).unwrap().effects.stasis);
        // Cryptbloom has stale cleanse text in `plaintext`, but its description
        // specifies a heal. It must never become a Quicksilver recommendation.
        assert_eq!(cat.item(3137).unwrap().effects.cleanse, None);
    }

    #[test]
    fn support_and_jungle_progression_remain_owned_commitments() {
        let cat = catalog();
        assert!(
            cat.item(3865).is_some(),
            "the support quest fixture must be available"
        );
        for id in [3865, 3866, 3867, 3869, 3870, 1101, 1102, 1103] {
            assert!(
                cat.item(id).unwrap().is_owned_commitment(&cat),
                "missing quest commitment {id}"
            );
        }
        for id in [3869, 3870, 3871, 3876, 3877, 3040, 3121] {
            assert!(
                cat.item(id).unwrap().is_finished(&cat),
                "missing finished transformation {id}"
            );
        }
        assert!(!cat.item(3865).unwrap().is_finished(&cat));
        assert!(!cat.item(1101).unwrap().is_finished(&cat));
        assert!(!cat.item(2003).unwrap().is_owned_commitment(&cat));
    }

    #[test]
    fn antiheal_application_distinguishes_retaliation_from_dealt_damage() {
        let cat = catalog();
        assert_eq!(
            cat.item(3123).unwrap().effects.grievous_trigger,
            Some(GrievousTrigger::PhysicalDamage)
        );
        assert_eq!(
            cat.item(3916).unwrap().effects.grievous_trigger,
            Some(GrievousTrigger::MagicDamage)
        );
        assert_eq!(
            cat.item(3076).unwrap().effects.grievous_trigger,
            Some(GrievousTrigger::WhenAttacked)
        );
        assert_eq!(
            cat.item(3075).unwrap().effects.grievous_trigger,
            Some(GrievousTrigger::WhenAttacked)
        );
    }

    #[test]
    fn aggregate_item_ids_are_available_at_the_fixture_patch() {
        let cat = catalog();
        for raw in [
            include_str!("../../../m0/tests/fixtures/opgg_ahri_mid.json"),
            include_str!("../../../m0/tests/fixtures/opgg_aphelios_adc.json"),
            include_str!("../../../m0/tests/fixtures/opgg_darius_top.json"),
            include_str!("../../../m0/tests/fixtures/opgg_leesin_jungle.json"),
            include_str!("../../../m0/tests/fixtures/opgg_lulu_support.json"),
            include_str!("../../../m0/tests/fixtures/opgg_ornn_top.json"),
            include_str!("../../../m0/tests/fixtures/opgg_udyr_jungle.json"),
            include_str!("../../../m0/tests/fixtures/opgg_xayah_adc.json"),
        ] {
            let value: Value = serde_json::from_str(raw).unwrap();
            for section in [
                "core_items",
                "boots",
                "starter_items",
                "last_items",
                "mythic_items",
            ] {
                for row in value["data"][section].as_array().into_iter().flatten() {
                    for id in row["ids"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_u64)
                    {
                        assert!(
                            cat.item(id as u32).is_some(),
                            "aggregate item {id} is absent from the patch fixture"
                        );
                    }
                }
            }
        }
    }
}
