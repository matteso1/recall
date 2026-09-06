//! Data Dragon catalog: items, champions and runes by id and by *name*, cached per patch.
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, SystemTime};

pub const BASE: &str = "https://ddragon.leagueoflegends.com";
const SR_MAP: &str = "11";

#[derive(Clone, Debug, Default, Serialize)]
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
    pub depth: u32,
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
    name.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()
}

fn u32_of(v: &Value, key: &str) -> u32 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0) as u32
}

fn ids_of(v: &Value, key: &str) -> Vec<u32> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_str().and_then(|s| s.parse().ok())).collect())
        .unwrap_or_default()
}

fn strings_of(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

impl Catalog {
    pub fn from_json(version: &str, items: &Value, champions: &Value, runes: &Value) -> Catalog {
        let mut cat = Catalog { version: version.to_string(), ..Default::default() };

        if let Some(data) = items.get("data").and_then(Value::as_object) {
            for (id_str, v) in data {
                let Ok(id) = id_str.parse::<u32>() else { continue };
                let gold = v.get("gold").cloned().unwrap_or(Value::Null);
                let item = Item {
                    id,
                    name: v.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                    total: u32_of(&gold, "total"),
                    base: u32_of(&gold, "base"),
                    from: ids_of(v, "from"),
                    into: ids_of(v, "into"),
                    on_sr: v.get("maps").and_then(|m| m.get(SR_MAP)).and_then(Value::as_bool).unwrap_or(false),
                    purchasable: gold.get("purchasable").and_then(Value::as_bool).unwrap_or(false),
                    in_store: v.get("inStore").and_then(Value::as_bool).unwrap_or(true),
                    tags: strings_of(v, "tags"),
                    depth: u32_of(v, "depth"),
                };
                let key = normalize(&item.name);
                let better = match cat.item_by_name.get(&key).and_then(|cur| cat.items.get(cur)) {
                    None => true,
                    Some(cur) => rank(&item) < rank(cur),
                };
                if better && !item.name.is_empty() {
                    cat.item_by_name.insert(key, id);
                }
                cat.items.insert(id, item);
            }
        }

        if let Some(data) = champions.get("data").and_then(Value::as_object) {
            for v in data.values() {
                let Some(key) = v.get("key").and_then(Value::as_str).and_then(|s| s.parse::<u32>().ok()) else { continue };
                let champ = Champion {
                    key,
                    id: v.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
                    name: v.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
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
            for (slot, s) in style.get("slots").and_then(Value::as_array).into_iter().flatten().enumerate() {
                for r in s.get("runes").and_then(Value::as_array).into_iter().flatten() {
                    let rune = Rune {
                        id: u32_of(r, "id"),
                        name: r.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
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
        self.items.get(&id).map(|i| i.name.clone()).unwrap_or_else(|| format!("item {id}"))
    }

    pub fn item_cost(&self, id: u32) -> u32 {
        self.items.get(&id).map(|i| i.total).unwrap_or(0)
    }

    /// Direct recipe, in Data Dragon order (duplicates preserved).
    pub fn components(&self, id: u32) -> Vec<u32> {
        self.items.get(&id).map(|i| i.from.clone()).unwrap_or_default()
    }

    pub fn champion(&self, key: u32) -> Option<&Champion> {
        self.champions.get(&key)
    }

    pub fn champion_name(&self, key: u32) -> String {
        self.champions.get(&key).map(|c| c.name.clone()).unwrap_or_else(|| format!("champ {key}"))
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
        self.runes.get(&id).map(|r| r.name.clone()).unwrap_or_else(|| format!("rune {id}"))
    }

    pub fn style_name(&self, id: u32) -> String {
        self.style_names.get(&id).cloned().unwrap_or_else(|| format!("style {id}"))
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
    serde_json::from_str::<Vec<String>>(&text).ok()?.into_iter().next()
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
        Err(e) => first_version(&vfile).ok_or(e).context("Data Dragon unreachable and no cached version"),
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
        assert_eq!(cat.item_id("The Collector"), Some(6676), "real shop item beats 667666");
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
}
