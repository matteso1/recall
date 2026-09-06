//! Aggregate build data per champion and position: what players on the current patch actually
//! run (summoner spells, rune page, skill order, starters, core items, boots, late items).
//!
//! Source: op.gg's champion API, the JSON the op.gg site itself renders (design doc 6.1). Public
//! aggregate statistics only, nothing about a live game. Cached under `<data dir>/aggregate/` for
//! `TTL`; a stale cache is used when the site is unreachable.
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const SOURCE: &str = "op.gg";
const BASE: &str = "https://lol-api-champion.op.gg/api";
pub const DEFAULT_REGION: &str = "global";
pub const DEFAULT_TIER: &str = "emerald_plus";
const TTL: Duration = Duration::from_secs(6 * 3600);
/// Below this many games in the assigned position the champion's main position is used instead.
pub const MIN_GAMES: u32 = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Position {
    Top,
    Jungle,
    Mid,
    Adc,
    Support,
}

impl Position {
    /// Champ select `assignedPosition` (top | jungle | middle | bottom | utility) or op.gg's names.
    pub fn parse(s: &str) -> Option<Position> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "top" => Position::Top,
            "jungle" | "jg" | "jung" => Position::Jungle,
            "middle" | "mid" => Position::Mid,
            "bottom" | "bot" | "adc" => Position::Adc,
            "utility" | "support" | "sup" | "supp" => Position::Support,
            _ => return None,
        })
    }
    pub fn slug(self) -> &'static str {
        match self {
            Position::Top => "top",
            Position::Jungle => "jungle",
            Position::Mid => "mid",
            Position::Adc => "adc",
            Position::Support => "support",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Position::Top => "Top",
            Position::Jungle => "Jungle",
            Position::Mid => "Mid",
            Position::Adc => "ADC",
            Position::Support => "Support",
        }
    }
    pub const ALL: [Position; 5] = [Position::Top, Position::Jungle, Position::Mid, Position::Adc, Position::Support];
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PositionStat {
    pub position: Position,
    pub games: u32,
    pub role_rate: f64,
    pub win_rate: f64,
}

/// The most-picked rune page: keystone, 3 primary, 2 secondary, 3 shards (ids the client takes as-is).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct RunePageIds {
    pub primary_style: u32,
    pub sub_style: u32,
    pub perks: Vec<u32>,
    pub games: u32,
    pub wins: u32,
    pub pick_rate: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Picked {
    pub ids: Vec<u32>,
    pub games: u32,
    pub wins: u32,
    pub pick_rate: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Aggregate {
    pub champion_key: u32,
    pub position: Position,
    /// Set when the assigned position was too rare for this champion and `position` is the main one.
    pub requested_position: Option<Position>,
    pub patch: String,
    pub region: String,
    pub tier: String,
    pub cached_at: String,
    /// Games in `position` on this patch: the sample everything below is drawn from.
    pub games: u32,
    pub win_rate: f64,
    pub positions: Vec<PositionStat>,
    pub spells: Picked,
    pub runes: Option<RunePageIds>,
    /// Ability per level for the most-picked order (usually 15 levels), e.g. Q E W E E R ...
    pub skill_order: Vec<char>,
    /// Max priority, e.g. E W Q
    pub skill_max: Vec<char>,
    pub starters: Picked,
    pub core: Picked,
    /// First items of the other popular core lines (e.g. Essence Reaver next to Yun Tal): alternatives, not late items.
    pub core_alternatives: Vec<u32>,
    pub boots: Option<Picked>,
    /// Finished items seen in final builds, by pick rate (components and boots included, filter them).
    pub late: Vec<Picked>,
    /// (enemy champion key, games, wins) in this position
    pub counters: Vec<(u32, u32, u32)>,
}

impl Aggregate {
    pub fn win_rate_of(p: &Picked) -> f64 {
        if p.games == 0 {
            0.0
        } else {
            p.wins as f64 / p.games as f64
        }
    }
    /// "op.gg emerald+ global, 88k games"
    pub fn describe(&self) -> String {
        let games = if self.games >= 1000 { format!("{}k", self.games / 1000) } else { self.games.to_string() };
        format!("{SOURCE} {} {}, {games} games", self.tier.replace("_plus", "+"), self.region)
    }
}

fn u32_of(v: &Value, key: &str) -> u32 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0) as u32
}

fn f64_of(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn ids_of(v: &Value, key: &str) -> Vec<u32> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_u64).map(|x| x as u32).collect())
        .unwrap_or_default()
}

fn picked(v: &Value) -> Picked {
    Picked { ids: ids_of(v, "ids"), games: u32_of(v, "play"), wins: u32_of(v, "win"), pick_rate: f64_of(v, "pick_rate") }
}

fn list<'a>(data: &'a Value, key: &str) -> Vec<&'a Value> {
    data.get(key).and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default()
}

fn letters(v: Option<&Value>) -> Vec<char> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).filter_map(|s| s.chars().next()).map(|c| c.to_ascii_uppercase()).collect())
        .unwrap_or_default()
}

/// Positions a champion is played in, from an op.gg champion summary (`summary.positions`).
pub fn decode_positions(summary: &Value) -> Vec<PositionStat> {
    let mut out = Vec::new();
    for p in list(summary, "positions") {
        let Some(position) = p.get("name").and_then(Value::as_str).and_then(Position::parse) else { continue };
        let stats = p.get("stats").cloned().unwrap_or(Value::Null);
        out.push(PositionStat {
            position,
            games: u32_of(&stats, "play"),
            role_rate: f64_of(&stats, "role_rate"),
            win_rate: f64_of(&stats, "win_rate"),
        });
    }
    out.sort_by(|a, b| b.games.cmp(&a.games));
    out
}

/// One op.gg champion response -> `Aggregate`.
pub fn decode(v: &Value, champion_key: u32, position: Position, region: &str, tier: &str) -> Result<Aggregate> {
    let data = v.get("data").unwrap_or(v);
    if !data.is_object() {
        bail!("unexpected aggregate payload");
    }
    let meta = v.get("meta").cloned().unwrap_or(Value::Null);
    let summary = data.get("summary").cloned().unwrap_or(Value::Null);
    let positions = decode_positions(&summary);
    let here = positions.iter().find(|p| p.position == position);
    let games = here.map(|p| p.games).unwrap_or_else(|| u32_of(&summary.get("average_stats").cloned().unwrap_or(Value::Null), "play"));
    let win_rate = here.map(|p| p.win_rate).unwrap_or(0.0);

    let spells = list(data, "summoner_spells").first().map(|v| picked(v)).unwrap_or_default();
    let runes = list(data, "runes").first().and_then(|r| {
        let mut perks = ids_of(r, "primary_rune_ids");
        perks.extend(ids_of(r, "secondary_rune_ids"));
        perks.extend(ids_of(r, "stat_mod_ids"));
        (perks.len() == 9).then(|| RunePageIds {
            primary_style: u32_of(r, "primary_page_id"),
            sub_style: u32_of(r, "secondary_page_id"),
            perks,
            games: u32_of(r, "play"),
            wins: u32_of(r, "win"),
            pick_rate: f64_of(r, "pick_rate"),
        })
    });
    let skill_order = letters(list(data, "skills").first().and_then(|s| s.get("order")));
    let skill_max = letters(list(data, "skill_masteries").first().and_then(|s| s.get("ids")));
    let starters = list(data, "starter_items").first().map(|v| picked(v)).unwrap_or_default();
    let cores = list(data, "core_items");
    let core = cores.first().map(|v| picked(v)).unwrap_or_default();
    let mut core_alternatives: Vec<u32> = Vec::new();
    for c in cores.iter().skip(1) {
        if let Some(&first) = ids_of(c, "ids").first() {
            if !core.ids.contains(&first) && !core_alternatives.contains(&first) {
                core_alternatives.push(first);
            }
        }
    }
    let boots = list(data, "boots").first().map(|v| picked(v)).filter(|p| !p.ids.is_empty());
    let mut late: Vec<Picked> = list(data, "last_items").iter().map(|v| picked(v)).filter(|p| !p.ids.is_empty()).collect();
    late.sort_by(|a, b| b.pick_rate.partial_cmp(&a.pick_rate).unwrap_or(std::cmp::Ordering::Equal));
    let counters = list(data, "counters")
        .iter()
        .map(|c| (u32_of(c, "champion_id"), u32_of(c, "play"), u32_of(c, "win")))
        .filter(|c| c.0 > 0)
        .collect();

    if core.ids.is_empty() && spells.ids.is_empty() && runes.is_none() {
        bail!("no build data for champion {champion_key} as {}", position.label());
    }
    Ok(Aggregate {
        champion_key,
        position,
        requested_position: None,
        patch: meta.get("version").and_then(Value::as_str).unwrap_or("").to_string(),
        region: region.to_string(),
        tier: tier.to_string(),
        cached_at: meta.get("cached_at").and_then(Value::as_str).unwrap_or("").to_string(),
        games,
        win_rate,
        positions,
        spells,
        runes,
        skill_order,
        skill_max,
        starters,
        core,
        core_alternatives,
        boots,
        late,
        counters,
    })
}

/// The position to fetch for: the assigned one when the champion is actually played there
/// (`MIN_GAMES` or more), otherwise the champion's main position. `None` only when nothing is known.
pub fn choose_position(requested: Option<Position>, positions: &[PositionStat]) -> Option<Position> {
    let main = positions.iter().max_by_key(|p| p.games).map(|p| p.position);
    match requested {
        Some(r) => {
            if positions.is_empty() {
                return Some(r);
            }
            match positions.iter().find(|p| p.position == r) {
                Some(p) if p.games >= MIN_GAMES => Some(r),
                _ => main.or(Some(r)),
            }
        }
        None => main,
    }
}

pub fn champion_url(region: &str, tier: &str, champion_key: u32, position: Position) -> String {
    format!("{BASE}/{region}/champions/ranked/{champion_key}/{}?tier={tier}", position.slug())
}

pub fn index_url(region: &str, tier: &str) -> String {
    format!("{BASE}/{region}/champions/ranked?tier={tier}")
}

fn fresh(path: &Path) -> bool {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|m| SystemTime::now().duration_since(m).ok())
        .map(|age| age < TTL)
        .unwrap_or(false)
}

/// GET with an on-disk cache: fresh file -> no request; request fails -> stale file if any.
async fn cached_json(path: &Path, url: &str) -> Result<Value> {
    if fresh(path) {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(v) = serde_json::from_str(&text) {
                return Ok(v);
            }
        }
    }
    let fetched = async {
        let resp = reqwest::get(url).await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            bail!("HTTP {status} from {url}: {}", text.chars().take(120).collect::<String>());
        }
        let v: Value = serde_json::from_str(&text).with_context(|| format!("not JSON: {url}"))?;
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, &text);
        Ok::<Value, anyhow::Error>(v)
    }
    .await;
    match fetched {
        Ok(v) => Ok(v),
        Err(e) => match std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
            Some(v) => {
                log::warn!("aggregate: {e}; using the cached copy");
                Ok(v)
            }
            None => Err(e),
        },
    }
}

fn cache_path(cache_dir: &Path, region: &str, tier: &str, name: &str) -> PathBuf {
    cache_dir.join(format!("{region}-{tier}")).join(name)
}

/// Positions per champion for the whole roster (one request per `TTL`).
pub async fn positions_index(cache_dir: &Path, region: &str, tier: &str) -> Result<HashMap<u32, Vec<PositionStat>>> {
    let v = cached_json(&cache_path(cache_dir, region, tier, "index.json"), &index_url(region, tier)).await?;
    let list = v.get("data").unwrap_or(&v).as_array().ok_or_else(|| anyhow!("index is not a list"))?;
    Ok(list
        .iter()
        .filter_map(|c| {
            let key = u32_of(c, "id");
            (key > 0).then(|| (key, decode_positions(c)))
        })
        .collect())
}

/// The aggregate for a champion in the position that makes sense: the assigned one when it is
/// really played, else the main one. `requested` is `None` in custom games and blind pick.
pub async fn load(cache_dir: &Path, region: &str, tier: &str, champion_key: u32, requested: Option<Position>) -> Result<Aggregate> {
    let positions = match positions_index(cache_dir, region, tier).await {
        Ok(index) => index.get(&champion_key).cloned().unwrap_or_default(),
        Err(e) => {
            log::warn!("aggregate: positions index unavailable: {e}");
            Vec::new()
        }
    };
    let position = choose_position(requested, &positions)
        .ok_or_else(|| anyhow!("no position known for champion {champion_key}"))?;
    let name = format!("{champion_key}-{}.json", position.slug());
    let v = cached_json(&cache_path(cache_dir, region, tier, &name), &champion_url(region, tier, champion_key, position)).await?;
    let mut agg = decode(&v, champion_key, position, region, tier)?;
    if requested.is_some() && requested != Some(position) {
        agg.requested_position = requested;
    }
    if agg.positions.is_empty() {
        agg.positions = positions;
    }
    Ok(agg)
}

#[cfg(test)]
mod tests {
    use super::*;

    const XAYAH_ADC: &str = include_str!("../../../m0/tests/fixtures/opgg_xayah_adc.json");

    fn xayah() -> Aggregate {
        decode(&serde_json::from_str(XAYAH_ADC).unwrap(), 498, Position::Adc, "na", "emerald_plus").unwrap()
    }

    #[test]
    fn positions_parse_from_both_naming_schemes() {
        assert_eq!(Position::parse("bottom"), Some(Position::Adc));
        assert_eq!(Position::parse("ADC"), Some(Position::Adc));
        assert_eq!(Position::parse("utility"), Some(Position::Support));
        assert_eq!(Position::parse("SUPPORT"), Some(Position::Support));
        assert_eq!(Position::parse("middle"), Some(Position::Mid));
        assert_eq!(Position::parse(""), None);
        assert_eq!(champion_url("global", "emerald_plus", 498, Position::Adc),
                   "https://lol-api-champion.op.gg/api/global/champions/ranked/498/adc?tier=emerald_plus");
    }

    #[test]
    fn decodes_the_real_xayah_response() {
        let a = xayah();
        assert_eq!(a.patch, "16.17");
        assert_eq!(a.spells.ids, vec![4, 21], "Flash + Barrier is the most picked");
        assert!(a.spells.pick_rate > 0.8);
        let runes = a.runes.clone().unwrap();
        assert_eq!((runes.primary_style, runes.sub_style), (8000, 8300));
        assert_eq!(runes.perks, vec![8008, 8009, 9103, 8014, 8304, 8345, 5005, 5008, 5001]);
        assert_eq!(a.skill_order.iter().collect::<String>(), "QEWEEREWEWRWWQQ");
        assert_eq!(a.skill_max, vec!['E', 'W', 'Q']);
        assert_eq!(a.starters.ids, vec![1086, 2003, 2003]);
        assert_eq!(a.core.ids, vec![3032, 6675, 3031]);
        assert!(a.core_alternatives.contains(&3508), "Essence Reaver is the other first item: {:?}", a.core_alternatives);
        assert!(!a.core_alternatives.contains(&3036), "LDR as a third core item is not a first-item alternative");
        assert_eq!(a.boots.clone().unwrap().ids, vec![3006]);
        assert_eq!(a.late[0].ids, vec![6675]);
        assert!(a.late.iter().any(|p| p.ids == vec![3036]), "LDR shows up as a late item");
        assert_eq!(a.positions[0].position, Position::Adc);
        assert!(a.positions[0].role_rate > 0.9);
        assert_eq!(a.games, a.positions[0].games);
        assert!(a.win_rate > 0.4 && a.win_rate < 0.6);
        assert_eq!(a.counters[0].0, 222);
        assert_eq!(a.describe(), "op.gg emerald+ na, 6k games");
    }

    #[test]
    fn position_choice_prefers_the_assigned_role_when_it_is_really_played() {
        let stats = xayah().positions;
        assert_eq!(choose_position(Some(Position::Adc), &stats), Some(Position::Adc));
        // Xayah support has (almost) no games: fall back to her main position.
        assert_eq!(choose_position(Some(Position::Support), &stats), Some(Position::Adc));
        assert_eq!(choose_position(None, &stats), Some(Position::Adc));
        // Nothing known: trust the assignment, or give up.
        assert_eq!(choose_position(Some(Position::Mid), &[]), Some(Position::Mid));
        assert_eq!(choose_position(None, &[]), None);
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(decode(&serde_json::json!({"data": {}}), 1, Position::Top, "na", "all").is_err());
        assert!(decode(&serde_json::json!([1, 2]), 1, Position::Top, "na", "all").is_err());
    }
}
