//! Aggregate build data per champion and position: what players on the current patch actually
//! run (summoner spells, rune page, skill order, starters, core items, boots, late items).
//!
//! Source: op.gg's champion API, the JSON the op.gg site itself renders (design doc 6.1). Public
//! aggregate statistics only, nothing about a live game. Cached under `<data dir>/aggregate/` for
//! `CACHE_TTL`; a validated stale cache is used when the site is unreachable.
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

pub const SOURCE: &str = "op.gg";
const BASE: &str = "https://lol-api-champion.op.gg/api";
pub const DEFAULT_REGION: &str = "global";
pub const DEFAULT_TIER: &str = "emerald_plus";
pub const CACHE_TTL: Duration = Duration::from_secs(6 * 3600);
const CACHE_SCHEMA_VERSION: u32 = 1;
const HTTP_TIMEOUT: Duration = Duration::from_secs(12);
const MAX_PAYLOAD_BYTES: u64 = 8 * 1024 * 1024;
/// Below this many games, role data is explicitly labelled limited; the role never changes.
pub const MIN_GAMES: u32 = 200;
/// Legacy thresholds retained for API compatibility; they do not establish causal superiority
/// and are no longer used to replace the popular baseline.
pub const CORE_MIN_PICK: f64 = 0.10;
pub const CORE_MIN_GAMES: u32 = 500;
pub const CORE_MIN_WR_GAIN: f64 = 0.02;

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
    pub const ALL: [Position; 5] = [
        Position::Top,
        Position::Jungle,
        Position::Mid,
        Position::Adc,
        Position::Support,
    ];
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

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum CacheStatus {
    #[default]
    Unknown,
    Network,
    Fresh,
    Stale,
}

/// Local cache provenance. Patch, source population, and the provider's unparsed cache time
/// remain on `Aggregate`; this timestamp records retrieval, not when underlying games occurred.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct AggregateProvenance {
    pub schema_version: u32,
    pub fetched_at_unix_ms: Option<u64>,
    pub cache_status: CacheStatus,
    pub warning: Option<String>,
}

impl AggregateProvenance {
    pub fn age_at(&self, now_unix_ms: u64) -> Option<Duration> {
        now_unix_ms
            .checked_sub(self.fetched_at_unix_ms?)
            .map(Duration::from_millis)
    }

    pub fn is_fresh_at(&self, now_unix_ms: u64) -> bool {
        matches!(self.cache_status, CacheStatus::Network | CacheStatus::Fresh)
            && self.age_at(now_unix_ms).is_some_and(|age| age < CACHE_TTL)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Aggregate {
    pub champion_key: u32,
    pub position: Position,
    /// The assigned role when this aggregate is a same-champion fallback: the champion has no
    /// games in that role at this rank, so `position` is the most-played role instead. The
    /// assignment is never replaced by it; the planner keeps both and labels the build.
    pub requested_position: Option<Position>,
    pub patch: String,
    pub region: String,
    pub tier: String,
    pub cached_at: String,
    #[serde(default)]
    pub provenance: AggregateProvenance,
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
    /// The most-played core line. Observed win rates do not automatically replace this baseline.
    pub core: Picked,
    /// The most-picked core line (equal to `core`; retained for compatibility).
    pub core_most_picked: Picked,
    /// Individual observations from this response, ordered by games then pick rate. Counts from
    /// different lines, refreshes, regions, tiers, or patches must not be added together.
    #[serde(default)]
    pub core_lines: Vec<Picked>,
    /// First items of the other popular core lines (e.g. Essence Reaver next to Yun Tal): alternatives, not late items.
    pub core_alternatives: Vec<u32>,
    pub boots: Option<Picked>,
    /// Every boots line by pick rate, so a build can take the most-played pair of its own damage family.
    #[serde(default)]
    pub boots_lines: Vec<Picked>,
    /// Finished items seen in final builds, by pick rate (components and boots included, filter them).
    pub late: Vec<Picked>,
    /// (enemy champion key, games, wins) in this position
    pub counters: Vec<(u32, u32, u32)>,
}

impl Aggregate {
    pub fn win_rate_of(p: &Picked) -> f64 {
        if p.games == 0 || p.wins > p.games {
            0.0
        } else {
            p.wins as f64 / p.games as f64
        }
    }
    /// "op.gg emerald+ global, 88k games"
    pub fn describe(&self) -> String {
        let games = if self.games >= 1000 {
            format!("{}k", self.games / 1000)
        } else {
            self.games.to_string()
        };
        format!(
            "{SOURCE} {} {}, {games} games",
            self.tier.replace("_plus", "+"),
            self.region
        )
    }
}

fn u32_of(v: &Value, key: &str) -> u32 {
    count(v, key).unwrap_or(0)
}

fn f64_of(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn ids_of(v: &Value, key: &str) -> Vec<u32> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_u64)
                .filter_map(|x| u32::try_from(x).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn picked(v: &Value) -> Picked {
    Picked {
        ids: ids_of(v, "ids"),
        games: u32_of(v, "play"),
        wins: u32_of(v, "win"),
        pick_rate: f64_of(v, "pick_rate"),
    }
}

fn list<'a>(data: &'a Value, key: &str) -> Vec<&'a Value> {
    let mut rows: Vec<_> = data
        .get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    rows.retain(|row| row.get("play").is_none() || u32_of(row, "play") > 0);
    rows.sort_by(|a, b| {
        u32_of(b, "play")
            .cmp(&u32_of(a, "play"))
            .then_with(|| f64_of(b, "pick_rate").total_cmp(&f64_of(a, "pick_rate")))
            .then_with(|| a.to_string().cmp(&b.to_string()))
    });
    rows
}

fn count(v: &Value, key: &str) -> Result<u32> {
    v.get(key)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| anyhow!("{key} must be a nonnegative integer within u32 range"))
}

fn rate(v: &Value, key: &str) -> Result<f64> {
    v.get(key)
        .and_then(Value::as_f64)
        .filter(|r| r.is_finite() && (0.0..=1.0).contains(r))
        .ok_or_else(|| anyhow!("{key} must be a finite rate between zero and one"))
}

fn validate_observation(v: &Value, has_pick_rate: bool) -> Result<()> {
    let games = count(v, "play")?;
    let wins = count(v, "win")?;
    if wins > games {
        bail!("wins exceed games");
    }
    if has_pick_rate {
        rate(v, "pick_rate")?;
    }
    Ok(())
}

fn validate_ids(v: &Value, key: &str) -> Result<Vec<u32>> {
    let values = v
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("{key} must be an array"))?;
    if values.is_empty() {
        bail!("{key} must not be empty");
    }
    values
        .iter()
        .map(|id| {
            id.as_u64()
                .and_then(|id| u32::try_from(id).ok())
                .filter(|id| *id > 0)
                .ok_or_else(|| anyhow!("{key} contains an invalid id"))
        })
        .collect()
}

fn validate_rune_page(r: &Value) -> Result<()> {
    let primary_style = count(r, "primary_page_id")?;
    let sub_style = count(r, "secondary_page_id")?;
    let styles = [8000, 8100, 8200, 8300, 8400];
    if !styles.contains(&primary_style)
        || !styles.contains(&sub_style)
        || primary_style == sub_style
    {
        bail!("rune styles must be known and distinct");
    }
    let primary = validate_ids(r, "primary_rune_ids")?;
    let secondary = validate_ids(r, "secondary_rune_ids")?;
    let shards = validate_ids(r, "stat_mod_ids")?;
    if primary.len() != 4 || secondary.len() != 2 || shards.len() != 3 {
        bail!("rune page needs four primary runes, two secondary runes, and three shards");
    }
    let mut perks = primary;
    perks.extend(secondary);
    perks.sort_unstable();
    perks.dedup();
    if perks.len() != 6 || perks.iter().any(|id| (5000..6000).contains(id)) {
        bail!("rune page has duplicate runes or shards in a rune slot");
    }
    // A shard may repeat across slots; rune selections may not. This decoder checks structure;
    // it cannot establish individual rune-tree membership without the current catalog.
    if shards.iter().any(|id| !(5000..6000).contains(id)) {
        bail!("rune page has a rune in a shard slot");
    }
    Ok(())
}

fn validate_summary(summary: &Value, champion_key: Option<u32>) -> Result<()> {
    if !summary.is_object() {
        bail!("champion summary must be an object");
    }
    if summary.get("id").is_some() {
        let id = count(summary, "id")?;
        if id == 0 || champion_key.is_some_and(|key| id != key) {
            bail!("champion summary identity does not match request");
        }
    }
    if let Some(average) = summary.get("average_stats") {
        count(average, "play")?;
        rate(average, "win_rate")?;
    }
    if let Some(positions) = summary.get("positions") {
        let positions = positions
            .as_array()
            .ok_or_else(|| anyhow!("positions must be a list"))?;
        for p in positions {
            let stats = p
                .get("stats")
                .ok_or_else(|| anyhow!("position has no stats"))?;
            count(stats, "play")?;
            rate(stats, "role_rate")?;
            rate(stats, "win_rate")?;
        }
    }
    Ok(())
}

fn validate_build_data(data: &Value, champion_key: u32) -> Result<()> {
    if let Some(summary) = data.get("summary") {
        validate_summary(summary, Some(champion_key))?;
    }
    for key in [
        "summoner_spells",
        "runes",
        "skills",
        "skill_masteries",
        "starter_items",
        "core_items",
        "boots",
        "last_items",
        "counters",
    ] {
        let Some(rows) = data.get(key) else { continue };
        let rows = rows
            .as_array()
            .ok_or_else(|| anyhow!("{key} must be a list"))?;
        for row in rows {
            validate_observation(row, key != "counters")
                .with_context(|| format!("invalid {key} observation"))?;
            match key {
                "runes" => validate_rune_page(row)?,
                "skills" | "skill_masteries" => {
                    let field = if key == "skills" { "order" } else { "ids" };
                    let values = row
                        .get(field)
                        .and_then(Value::as_array)
                        .ok_or_else(|| anyhow!("{key} has no {field}"))?;
                    if values.is_empty()
                        || values
                            .iter()
                            .any(|v| !matches!(v.as_str(), Some("Q" | "W" | "E" | "R")))
                    {
                        bail!("{key} contains an invalid ability");
                    }
                }
                "counters" => {
                    if count(row, "champion_id")? == 0 {
                        bail!("counter has an invalid champion id");
                    }
                }
                _ => {
                    let ids = validate_ids(row, "ids")?;
                    if key == "summoner_spells" && (ids.len() != 2 || ids[0] == ids[1]) {
                        bail!("summoner spell selection must contain two distinct spells");
                    }
                }
            }
        }
    }
    Ok(())
}

fn letters(v: Option<&Value>) -> Vec<char> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .filter_map(|s| s.chars().next())
                .map(|c| c.to_ascii_uppercase())
                .collect()
        })
        .unwrap_or_default()
}

/// Positions a champion is played in, from an op.gg champion summary (`summary.positions`).
pub fn decode_positions(summary: &Value) -> Vec<PositionStat> {
    let mut out = Vec::new();
    for p in list(summary, "positions") {
        let Some(position) = p
            .get("name")
            .and_then(Value::as_str)
            .and_then(Position::parse)
        else {
            continue;
        };
        let stats = p.get("stats").cloned().unwrap_or(Value::Null);
        let (Ok(games), Ok(role_rate), Ok(win_rate)) = (
            count(&stats, "play"),
            rate(&stats, "role_rate"),
            rate(&stats, "win_rate"),
        ) else {
            continue;
        };
        out.push(PositionStat {
            position,
            games,
            role_rate,
            win_rate,
        });
    }
    out.sort_by(|a, b| {
        b.games
            .cmp(&a.games)
            .then_with(|| a.position.slug().cmp(b.position.slug()))
    });
    out
}

/// One op.gg champion response -> `Aggregate`.
pub fn decode(
    v: &Value,
    champion_key: u32,
    position: Position,
    region: &str,
    tier: &str,
) -> Result<Aggregate> {
    let data = v.get("data").unwrap_or(v);
    if !data.is_object() {
        bail!("unexpected aggregate payload");
    }
    validate_build_data(data, champion_key)?;
    let meta = v.get("meta").cloned().unwrap_or(Value::Null);
    let summary = data.get("summary").cloned().unwrap_or(Value::Null);
    let positions = decode_positions(&summary);
    let here = positions.iter().find(|p| p.position == position);
    if summary.get("positions").is_some() && !here.is_some_and(|p| p.games > 0) {
        bail!(
            "no aggregate games for champion {champion_key} as {}",
            position.label()
        );
    }
    let games = here.map(|p| p.games).unwrap_or_else(|| {
        u32_of(
            &summary.get("average_stats").cloned().unwrap_or(Value::Null),
            "play",
        )
    });
    let win_rate = here
        .map(|p| p.win_rate)
        .unwrap_or_else(|| f64_of(&summary["average_stats"], "win_rate"));

    let spells = list(data, "summoner_spells")
        .first()
        .map(|v| picked(v))
        .unwrap_or_default();
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
    let skill_max = letters(
        list(data, "skill_masteries")
            .first()
            .and_then(|s| s.get("ids")),
    );
    let starters = list(data, "starter_items")
        .first()
        .map(|v| picked(v))
        .unwrap_or_default();
    let cores: Vec<Picked> = list(data, "core_items").iter().map(|v| picked(v)).collect();
    let (core, core_most_picked) = choose_core(&cores);
    let mut core_alternatives: Vec<u32> = Vec::new();
    for c in &cores {
        if let Some(&first) = c.ids.first() {
            if !core.ids.contains(&first) && !core_alternatives.contains(&first) {
                core_alternatives.push(first);
            }
        }
    }
    let boots_lines: Vec<Picked> = list(data, "boots")
        .iter()
        .map(|v| picked(v))
        .filter(|p| !p.ids.is_empty())
        .collect();
    let boots = boots_lines.first().cloned();
    let mut late: Vec<Picked> = list(data, "last_items")
        .iter()
        .map(|v| picked(v))
        .filter(|p| !p.ids.is_empty())
        .collect();
    late.sort_by(picked_popularity);
    let counters = list(data, "counters")
        .iter()
        .map(|c| {
            (
                u32_of(c, "champion_id"),
                u32_of(c, "play"),
                u32_of(c, "win"),
            )
        })
        .filter(|c| c.0 > 0)
        .collect();

    if core.ids.is_empty() && spells.ids.is_empty() && runes.is_none() {
        bail!(
            "no build data for champion {champion_key} as {}",
            position.label()
        );
    }
    Ok(Aggregate {
        champion_key,
        position,
        requested_position: None,
        patch: meta
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        region: region.to_string(),
        tier: tier.to_string(),
        cached_at: meta
            .get("cached_at")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        provenance: AggregateProvenance {
            warning: here
                .filter(|p| p.games > 0 && p.games < MIN_GAMES)
                .map(|p| {
                    format!(
                        "Limited {} data: {} observed games; use this build cautiously.",
                        position.label(),
                        p.games
                    )
                }),
            ..AggregateProvenance::default()
        },
        games,
        win_rate,
        positions,
        spells,
        runes,
        skill_order,
        skill_max,
        starters,
        core,
        core_most_picked,
        core_lines: cores,
        core_alternatives,
        boots,
        boots_lines,
        late,
        counters,
    })
}

fn picked_popularity(a: &Picked, b: &Picked) -> std::cmp::Ordering {
    b.games
        .cmp(&a.games)
        .then_with(|| b.pick_rate.total_cmp(&a.pick_rate))
        .then_with(|| a.ids.cmp(&b.ids))
}

/// (baseline, most-picked line), retained as a pair for API compatibility. Source order and raw
/// win rate cannot establish that another build is better. The planner may consider the other
/// observations using visible game context; statistical uncertainty lives in `statistics`.
pub fn choose_core(lines: &[Picked]) -> (Picked, Picked) {
    let most = lines
        .iter()
        .filter(|line| {
            !line.ids.is_empty()
                && !line.ids.contains(&0)
                && line.games > 0
                && line.wins <= line.games
                && line.pick_rate.is_finite()
                && (0.0..=1.0).contains(&line.pick_rate)
        })
        .min_by(|a, b| picked_popularity(a, b))
        .cloned()
        .unwrap_or_default();
    (most.clone(), most)
}

/// Use a nonzero sample for the assigned role, even when it is small. Never substitute another
/// role's loadout. Only an unassigned player may use the champion's most-played known role.
pub fn choose_position(
    requested: Option<Position>,
    positions: &[PositionStat],
) -> Option<Position> {
    positions
        .iter()
        .filter(|p| {
            p.games > 0
                && p.role_rate.is_finite()
                && p.win_rate.is_finite()
                && (0.0..=1.0).contains(&p.role_rate)
                && (0.0..=1.0).contains(&p.win_rate)
                && requested.is_none_or(|role| role == p.position)
        })
        .min_by(|a, b| {
            b.games
                .cmp(&a.games)
                .then_with(|| a.position.slug().cmp(b.position.slug()))
        })
        .map(|p| p.position)
}

/// When the assigned role has no games at all, the champion's most-played role is the only
/// same-champion evidence. Callers label the result as a fallback; it never becomes the assignment.
/// `None` when the assigned role has data (no fallback needed) or nothing else is known.
pub fn fallback_position(requested: Position, positions: &[PositionStat]) -> Option<Position> {
    if choose_position(Some(requested), positions).is_some() {
        return None;
    }
    choose_position(None, positions).filter(|main| *main != requested)
}

pub fn champion_url(region: &str, tier: &str, champion_key: u32, position: Position) -> String {
    format!(
        "{BASE}/{region}/champions/ranked/{champion_key}/{}?tier={tier}",
        position.slug()
    )
}

pub fn index_url(region: &str, tier: &str) -> String {
    format!("{BASE}/{region}/champions/ranked?tier={tier}")
}

#[derive(Clone, Copy)]
enum PayloadKind {
    Index,
    Champion { key: u32, position: Position },
}

impl PayloadKind {
    fn validate(self, value: &Value) -> Result<()> {
        match self {
            Self::Champion { key, position } => {
                let data = value.get("data").unwrap_or(value);
                let summary = data
                    .get("summary")
                    .ok_or_else(|| anyhow!("aggregate has no champion identity"))?;
                if count(summary, "id")? != key {
                    bail!("aggregate champion identity does not match request");
                }
                let aggregate = decode(value, key, position, DEFAULT_REGION, DEFAULT_TIER)?;
                if !aggregate
                    .positions
                    .iter()
                    .any(|p| p.position == position && p.games > 0)
                {
                    bail!(
                        "no aggregate games for champion {key} as {}",
                        position.label()
                    );
                }
            }
            Self::Index => {
                let rows = value
                    .get("data")
                    .unwrap_or(value)
                    .as_array()
                    .ok_or_else(|| anyhow!("index is not a list"))?;
                if rows.is_empty() {
                    bail!("empty champion index");
                }
                for summary in rows {
                    if count(summary, "id")? == 0 {
                        bail!("index has an invalid champion id");
                    }
                    validate_summary(summary, None)?;
                }
                if !rows
                    .iter()
                    .any(|summary| !decode_positions(summary).is_empty())
                {
                    bail!("champion index has no usable positions");
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct CacheRecord {
    schema_version: u32,
    source_url: String,
    fetched_at_unix_ms: Option<u64>,
    payload: Value,
}

struct CachedJson {
    value: Value,
    provenance: AggregateProvenance,
}

impl CacheRecord {
    fn into_response(self, cache_status: CacheStatus, warning: Option<String>) -> CachedJson {
        CachedJson {
            value: self.payload,
            provenance: AggregateProvenance {
                schema_version: self.schema_version,
                fetched_at_unix_ms: self.fetched_at_unix_ms,
                cache_status,
                warning,
            },
        }
    }
}

fn unix_ms(time: SystemTime) -> Option<u64> {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .and_then(|age| u64::try_from(age.as_millis()).ok())
}

fn read_cached(path: &Path, url: &str, kind: PayloadKind) -> Result<CacheRecord> {
    let metadata = std::fs::metadata(path)?;
    if metadata.len() > MAX_PAYLOAD_BYTES + 4096 {
        bail!("aggregate cache is too large");
    }
    let value: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let record = if value.get("schema_version").is_some() {
        let record: CacheRecord = serde_json::from_value(value)?;
        if record.schema_version != CACHE_SCHEMA_VERSION || record.source_url != url {
            bail!("aggregate cache schema or request does not match");
        }
        record
    } else {
        // Legacy cache files contain the original response. Preserve their mtime as retrieval
        // provenance; merely reading or migrating an old file must not extend its six-hour TTL.
        CacheRecord {
            schema_version: CACHE_SCHEMA_VERSION,
            source_url: url.to_string(),
            fetched_at_unix_ms: metadata.modified().ok().and_then(unix_ms),
            payload: value,
        }
    };
    kind.validate(&record.payload)?;
    Ok(record)
}

fn read_last_good(path: &Path, url: &str, kind: PayloadKind) -> Option<CacheRecord> {
    if let Ok(record) = read_cached(path, url, kind) {
        return Some(record);
    }
    let archives = path.parent()?.join("patches");
    let name = path.file_name()?;
    std::fs::read_dir(archives)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| read_cached(&entry.path().join(name), url, kind).ok())
        .max_by_key(|record| record.fetched_at_unix_ms)
}

fn archive_path(path: &Path, payload: &Value) -> Option<PathBuf> {
    let patch = payload.get("meta")?.get("version")?.as_str()?;
    // The version is remote data, never a free-form filesystem path.
    if patch.is_empty()
        || patch.len() > 40
        || !patch.chars().all(|c| c.is_ascii_digit() || c == '.')
        || patch == "."
        || patch == ".."
    {
        return None;
    }
    Some(
        path.parent()?
            .join("patches")
            .join(patch)
            .join(path.file_name()?),
    )
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("aggregate cache has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".aggregate-{}-{}.tmp",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    // If create_new fails, do not remove a file this call did not create.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let result = (|| -> Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn store_cached(path: &Path, record: &CacheRecord, previous: Option<&CacheRecord>) -> Result<()> {
    // One latest response per patch and request, not an accumulating dataset. Archive the
    // previous patch before replacing the active file; overlapping refresh counts are not added.
    if let Some(previous) = previous {
        if let Some(archive) = archive_path(path, &previous.payload) {
            atomic_write(&archive, &serde_json::to_vec(previous)?)?;
        }
    }
    let bytes = serde_json::to_vec(record)?;
    if let Some(archive) = archive_path(path, &record.payload) {
        atomic_write(&archive, &bytes)?;
    }
    atomic_write(path, &bytes)
}

/// A valid fresh response avoids a request. Refreshes are bounded, validated before any write,
/// and atomically promoted. A failed refresh returns only a previously validated stale response.
async fn cached_json(path: &Path, url: &str, kind: PayloadKind) -> Result<CachedJson> {
    cached_json_with_timeout(path, url, kind, HTTP_TIMEOUT).await
}

async fn cached_json_with_timeout(
    path: &Path,
    url: &str,
    kind: PayloadKind,
    timeout: Duration,
) -> Result<CachedJson> {
    let previous = read_last_good(path, url, kind);
    if let Some(record) = previous.as_ref() {
        let provenance = record
            .clone()
            .into_response(CacheStatus::Fresh, None)
            .provenance;
        if unix_ms(SystemTime::now()).is_some_and(|now| provenance.is_fresh_at(now)) {
            return Ok(record.clone().into_response(CacheStatus::Fresh, None));
        }
    }
    let fetched = async {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3).min(timeout))
            .timeout(timeout)
            .build()?;
        let mut response = client.get(url).send().await?.error_for_status()?;
        if response
            .content_length()
            .is_some_and(|len| len > MAX_PAYLOAD_BYTES)
        {
            bail!("aggregate response is too large");
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if (bytes.len() as u64).saturating_add(chunk.len() as u64) > MAX_PAYLOAD_BYTES {
                bail!("aggregate response is too large");
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value =
            serde_json::from_slice(&bytes).with_context(|| format!("not JSON: {url}"))?;
        kind.validate(&value)?;
        Ok::<_, anyhow::Error>(CacheRecord {
            schema_version: CACHE_SCHEMA_VERSION,
            source_url: url.to_string(),
            fetched_at_unix_ms: unix_ms(SystemTime::now()),
            payload: value,
        })
    }
    .await;
    match fetched {
        Ok(record) => {
            let warning = store_cached(path, &record, previous.as_ref())
                .err()
                .map(|error| {
                    log::warn!("aggregate: could not store refreshed data: {error:#}");
                    "Refreshed data could not be cached; it is available for this session."
                        .to_string()
                });
            Ok(record.into_response(CacheStatus::Network, warning))
        }
        Err(error) => match previous {
            Some(record) => {
                log::warn!("aggregate: {error:#}; using the last good cached response");
                Ok(record.into_response(
                    CacheStatus::Stale,
                    Some("Refresh failed; using an older cached aggregate.".to_string()),
                ))
            }
            None => Err(error),
        },
    }
}

fn cache_path(cache_dir: &Path, region: &str, tier: &str, name: &str) -> PathBuf {
    cache_dir.join(format!("{region}-{tier}")).join(name)
}

/// Positions per champion for the whole roster (one request per `CACHE_TTL`).
pub async fn positions_index(
    cache_dir: &Path,
    region: &str,
    tier: &str,
) -> Result<HashMap<u32, Vec<PositionStat>>> {
    Ok(load_positions_index(cache_dir, region, tier).await?.0)
}

async fn load_positions_index(
    cache_dir: &Path,
    region: &str,
    tier: &str,
) -> Result<(HashMap<u32, Vec<PositionStat>>, AggregateProvenance)> {
    let cached = cached_json(
        &cache_path(cache_dir, region, tier, "index.json"),
        &index_url(region, tier),
        PayloadKind::Index,
    )
    .await?;
    let v = cached.value;
    let list = v
        .get("data")
        .unwrap_or(&v)
        .as_array()
        .ok_or_else(|| anyhow!("index is not a list"))?;
    let positions = list
        .iter()
        .filter_map(|c| {
            let key = u32_of(c, "id");
            (key > 0).then(|| (key, decode_positions(c)))
        })
        .collect();
    Ok((positions, cached.provenance))
}

/// A champion's aggregate for the requested role. Small samples are labelled. When the champion
/// has no games in the requested role at this rank, the most-played role's aggregate is returned
/// with `requested_position` set and a warning, so the planner can show a labelled same-champion
/// starting point instead of nothing. Without a role assignment, the most-played known role is used.
pub async fn load(
    cache_dir: &Path,
    region: &str,
    tier: &str,
    champion_key: u32,
    requested: Option<Position>,
) -> Result<Aggregate> {
    let (positions, index_provenance) = match load_positions_index(cache_dir, region, tier).await {
        Ok((index, provenance)) => (
            index.get(&champion_key).cloned().unwrap_or_default(),
            Some(provenance),
        ),
        Err(e) => {
            log::warn!("aggregate: positions index unavailable: {e}");
            (Vec::new(), None)
        }
    };
    let index_unavailable_or_stale = index_provenance
        .as_ref()
        .is_none_or(|provenance| provenance.cache_status == CacheStatus::Stale);
    let exact = choose_position(requested, &positions);
    let fallback_from = match (exact, requested) {
        (None, Some(role)) => fallback_position(role, &positions).map(|main| (role, main)),
        _ => None,
    };
    let position = exact
        .or(fallback_from.map(|(_, main)| main))
        // A failed index must not hide a usable exact-role cache/endpoint. The champion payload
        // still has to identify that role and contain a nonzero sample before cache promotion.
        .or_else(|| requested.filter(|_| index_unavailable_or_stale))
        .ok_or_else(|| {
            anyhow!(
                "no aggregate games for champion {champion_key}{}",
                requested
                    .map(|p| format!(" as {}", p.label()))
                    .unwrap_or_default()
            )
        })?;
    // For a fallback, the most-played role comes first; if it cannot be fetched (offline with
    // only another role cached), any other role the champion is actually played in will do.
    let mut candidates = vec![position];
    if fallback_from.is_some() {
        candidates.extend(
            positions
                .iter()
                .filter(|p| p.games > 0 && p.position != position && Some(p.position) != requested)
                .map(|p| p.position),
        );
    }
    let mut loaded = None;
    let mut last_error = None;
    for candidate in candidates {
        let name = format!("{champion_key}-{}.json", candidate.slug());
        match cached_json(
            &cache_path(cache_dir, region, tier, &name),
            &champion_url(region, tier, champion_key, candidate),
            PayloadKind::Champion {
                key: champion_key,
                position: candidate,
            },
        )
        .await
        {
            Ok(cached) => {
                loaded = Some((candidate, cached));
                break;
            }
            Err(error) => last_error = Some(error),
        }
    }
    let Some((position, cached)) = loaded else {
        return Err(last_error.unwrap_or_else(|| anyhow!("no aggregate available")));
    };
    let mut agg = decode(&cached.value, champion_key, position, region, tier)?;
    let limited_data_warning = agg.provenance.warning.take();
    agg.provenance = cached.provenance;
    let mut warnings: Vec<String> = agg
        .provenance
        .warning
        .take()
        .into_iter()
        .chain(limited_data_warning)
        .collect();
    if let Some((requested_role, _)) = fallback_from {
        // The planner composes the user-facing label from this; see engine::plan_in_mode.
        agg.requested_position = Some(requested_role);
    }
    if index_provenance.is_some_and(|provenance| provenance.cache_status == CacheStatus::Stale) {
        agg.provenance.cache_status = CacheStatus::Stale;
        warnings.push("Position information uses an older cached source.".to_string());
    }
    agg.provenance.warning = (!warnings.is_empty()).then(|| warnings.join(" "));
    if agg.positions.is_empty() {
        agg.positions = positions;
    }
    Ok(agg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    const XAYAH_ADC: &str = include_str!("../../../m0/tests/fixtures/opgg_xayah_adc.json");

    fn xayah() -> Aggregate {
        decode(
            &serde_json::from_str(XAYAH_ADC).unwrap(),
            498,
            Position::Adc,
            "na",
            "emerald_plus",
        )
        .unwrap()
    }

    struct TestCache(PathBuf);

    impl TestCache {
        fn new() -> Self {
            let stamp = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("recall-aggregate-{}-{stamp}", std::process::id()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> PathBuf {
            self.0.join("498-adc.json")
        }

        fn write_old(&self, text: &str) {
            std::fs::write(self.path(), text).unwrap();
            let old = SystemTime::now() - Duration::from_secs(7 * 3600);
            std::fs::File::options()
                .write(true)
                .open(self.path())
                .unwrap()
                .set_modified(old)
                .unwrap();
        }
    }

    impl Drop for TestCache {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn serve_once(body: String) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0_u8; 4096];
            assert!(
                stream.read(&mut request).unwrap() > 0,
                "client sends an HTTP request"
            );
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        (format!("http://{address}/champion"), worker)
    }

    #[tokio::test]
    async fn invalid_network_payload_keeps_the_last_good_cache() {
        let cache = TestCache::new();
        cache.write_old(XAYAH_ADC);
        let (url, server) = serve_once(serde_json::json!({"data": {}}).to_string());
        let cached = cached_json(
            &cache.path(),
            &url,
            PayloadKind::Champion {
                key: 498,
                position: Position::Adc,
            },
        )
        .await
        .unwrap();
        let value = cached.value;
        server.join().unwrap();
        assert_eq!(value["data"]["summary"]["id"], 498);
        assert_eq!(std::fs::read_to_string(cache.path()).unwrap(), XAYAH_ADC);
    }

    #[tokio::test]
    async fn syntactically_valid_but_empty_cache_is_refetched() {
        let cache = TestCache::new();
        std::fs::write(cache.path(), "{}").unwrap();
        let (url, server) = serve_once(XAYAH_ADC.to_string());
        let cached = cached_json(
            &cache.path(),
            &url,
            PayloadKind::Champion {
                key: 498,
                position: Position::Adc,
            },
        )
        .await
        .unwrap();
        let value = cached.value;
        server.join().unwrap();
        assert_eq!(value["data"]["summary"]["id"], 498);
    }

    #[tokio::test]
    async fn unusable_response_without_valid_cache_is_an_error() {
        let cache = TestCache::new();
        let (url, server) = serve_once(serde_json::json!({"error": "maintenance"}).to_string());
        let result = cached_json(
            &cache.path(),
            &url,
            PayloadKind::Champion {
                key: 498,
                position: Position::Adc,
            },
        )
        .await;
        server.join().unwrap();
        assert!(result.is_err());
        assert!(!cache.path().exists());
    }

    #[tokio::test]
    async fn network_build_data_must_identify_the_requested_champion_and_role() {
        for change in ["no_identity", "wrong_champion", "wrong_role"] {
            let cache = TestCache::new();
            cache.write_old(XAYAH_ADC);
            let mut response: Value = serde_json::from_str(XAYAH_ADC).unwrap();
            match change {
                "no_identity" => {
                    response["data"].as_object_mut().unwrap().remove("summary");
                }
                "wrong_champion" => response["data"]["summary"]["id"] = serde_json::json!(103),
                _ => {
                    response["data"]["summary"]["positions"][0]["name"] =
                        serde_json::json!("SUPPORT")
                }
            }
            let (url, server) = serve_once(response.to_string());
            let cached = cached_json(
                &cache.path(),
                &url,
                PayloadKind::Champion {
                    key: 498,
                    position: Position::Adc,
                },
            )
            .await
            .unwrap();
            server.join().unwrap();
            assert_eq!(
                cached.provenance.cache_status,
                CacheStatus::Stale,
                "accepted {change}"
            );
            assert_eq!(cached.value["data"]["summary"]["id"], 498);
            assert_eq!(std::fs::read_to_string(cache.path()).unwrap(), XAYAH_ADC);
        }
    }

    #[tokio::test]
    async fn a_corrupt_active_cache_can_fall_back_to_a_valid_patch_snapshot() {
        let cache = TestCache::new();
        std::fs::write(cache.path(), "{broken").unwrap();
        let (url, server) = serve_once("{}".to_string());
        let record = CacheRecord {
            schema_version: CACHE_SCHEMA_VERSION,
            source_url: url.clone(),
            fetched_at_unix_ms: Some(unix_ms(SystemTime::now()).unwrap() - 7 * 3600 * 1000),
            payload: serde_json::from_str(XAYAH_ADC).unwrap(),
        };
        let archive = cache.0.join("patches").join("16.17").join("498-adc.json");
        std::fs::create_dir_all(archive.parent().unwrap()).unwrap();
        std::fs::write(&archive, serde_json::to_vec(&record).unwrap()).unwrap();
        let result = cached_json(
            &cache.path(),
            &url,
            PayloadKind::Champion {
                key: 498,
                position: Position::Adc,
            },
        )
        .await;
        server.join().unwrap();
        let cached = result.unwrap();
        assert_eq!(cached.value["data"]["core_items"][0]["play"], 1542);
        assert_eq!(cached.provenance.cache_status, CacheStatus::Stale);
        assert_eq!(
            cached.provenance.fetched_at_unix_ms,
            record.fetched_at_unix_ms
        );
    }

    #[tokio::test]
    async fn valid_cache_avoids_network_and_touching_a_file_does_not_renew_its_ttl() {
        let cache = TestCache::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/champion", listener.local_addr().unwrap());
        let received = unix_ms(SystemTime::now()).unwrap() - 60_000;
        let mut record = CacheRecord {
            schema_version: CACHE_SCHEMA_VERSION,
            source_url: url.clone(),
            fetched_at_unix_ms: Some(received),
            payload: serde_json::from_str(XAYAH_ADC).unwrap(),
        };
        std::fs::write(cache.path(), serde_json::to_vec(&record).unwrap()).unwrap();
        let cached = cached_json_with_timeout(
            &cache.path(),
            &url,
            PayloadKind::Champion {
                key: 498,
                position: Position::Adc,
            },
            Duration::from_millis(30),
        )
        .await
        .unwrap();
        assert_eq!(cached.provenance.cache_status, CacheStatus::Fresh);
        assert_eq!(cached.provenance.fetched_at_unix_ms, Some(received));
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );

        record.fetched_at_unix_ms = Some(received - 7 * 3600 * 1000);
        std::fs::write(cache.path(), serde_json::to_vec(&record).unwrap()).unwrap();
        let cached = cached_json_with_timeout(
            &cache.path(),
            &url,
            PayloadKind::Champion {
                key: 498,
                position: Position::Adc,
            },
            Duration::from_millis(30),
        )
        .await
        .unwrap();
        assert_eq!(cached.provenance.cache_status, CacheStatus::Stale);
        assert_eq!(
            cached.provenance.fetched_at_unix_ms,
            record.fetched_at_unix_ms
        );
        assert!(!cached
            .provenance
            .is_fresh_at(unix_ms(SystemTime::now()).unwrap()));
    }

    #[tokio::test]
    async fn roster_cache_validation_does_not_treat_a_roster_as_a_champion_response() {
        let cache = TestCache::new();
        let fixture: Value = serde_json::from_str(XAYAH_ADC).unwrap();
        let roster = serde_json::json!({"meta": fixture["meta"], "data": [fixture["data"]["summary"].clone()]});
        let (url, server) = serve_once(roster.to_string());
        let path = cache.0.join("index.json");
        let cached = cached_json(&path, &url, PayloadKind::Index).await.unwrap();
        server.join().unwrap();
        assert_eq!(cached.value["data"][0]["id"], 498);
        assert_eq!(
            decode_positions(&cached.value["data"][0])[0].position,
            Position::Adc
        );
        assert_eq!(cached.provenance.cache_status, CacheStatus::Network);
        assert!(read_cached(&path, &url, PayloadKind::Index).is_ok());
        assert!(read_cached(
            &path,
            &url,
            PayloadKind::Champion {
                key: 498,
                position: Position::Adc
            }
        )
        .is_err());
    }

    #[test]
    fn fallback_position_is_the_most_played_role_only_when_the_assigned_one_has_no_games() {
        let stats = xayah().positions;
        assert_eq!(
            fallback_position(Position::Jungle, &stats),
            Some(Position::Adc)
        );
        assert_eq!(
            fallback_position(Position::Adc, &stats),
            None,
            "exact data exists"
        );
        assert_eq!(fallback_position(Position::Jungle, &[]), None);
        let irelia: Vec<PositionStat> = [(Position::Top, 83637), (Position::Mid, 53921)]
            .into_iter()
            .map(|(position, games)| PositionStat {
                position,
                games,
                role_rate: 0.5,
                win_rate: 0.5,
            })
            .collect();
        assert_eq!(
            fallback_position(Position::Jungle, &irelia),
            Some(Position::Top)
        );
    }

    #[tokio::test]
    async fn load_preserves_a_small_real_roles_warning_and_labels_a_missing_role_as_a_fallback() {
        let cache = TestCache::new();
        let mut source: Value = serde_json::from_str(include_str!(
            "../../../m0/tests/fixtures/opgg_lulu_support.json"
        ))
        .unwrap();
        source["data"]["summary"]["positions"][0]["stats"]["play"] = serde_json::json!(87);
        let index = serde_json::json!({"meta": source["meta"], "data": [source["data"]["summary"].clone()]});
        let directory = cache.0.join("global-emerald_plus");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(
            directory.join("index.json"),
            serde_json::to_vec(&index).unwrap(),
        )
        .unwrap();
        std::fs::write(
            directory.join("117-support.json"),
            serde_json::to_vec(&source).unwrap(),
        )
        .unwrap();

        let aggregate = load(
            &cache.0,
            "global",
            "emerald_plus",
            117,
            Some(Position::Support),
        )
        .await
        .unwrap();
        assert_eq!(
            (aggregate.position, aggregate.games),
            (Position::Support, 87)
        );
        assert_eq!(aggregate.provenance.cache_status, CacheStatus::Fresh);
        assert!(aggregate
            .provenance
            .warning
            .as_deref()
            .unwrap()
            .contains("87"));
        assert_eq!(aggregate.requested_position, None);
        // Lulu has no Jungle games: the Support build comes back labelled, never relabelled.
        let fallback = load(
            &cache.0,
            "global",
            "emerald_plus",
            117,
            Some(Position::Jungle),
        )
        .await
        .unwrap();
        assert_eq!(
            (fallback.position, fallback.requested_position),
            (Position::Support, Some(Position::Jungle))
        );
        assert_eq!(
            fallback.games, 87,
            "the Support sample, still labelled small"
        );
        assert_eq!(fallback.provenance.cache_status, CacheStatus::Fresh);
    }

    #[tokio::test]
    async fn fallback_tries_the_most_played_role_first_and_never_the_network_when_a_role_is_cached()
    {
        // Lulu listed as Support (87) and Mid (40); only the Support response is cached.
        // A Jungle assignment must use the cached Support build without any request.
        let cache = TestCache::new();
        let mut source: Value = serde_json::from_str(include_str!(
            "../../../m0/tests/fixtures/opgg_lulu_support.json"
        ))
        .unwrap();
        source["data"]["summary"]["positions"][0]["stats"]["play"] = serde_json::json!(87);
        let mut mid = source["data"]["summary"]["positions"][0].clone();
        mid["name"] = serde_json::json!("MID");
        mid["stats"]["play"] = serde_json::json!(40);
        source["data"]["summary"]["positions"]
            .as_array_mut()
            .unwrap()
            .push(mid);
        let index = serde_json::json!({"meta": source["meta"], "data": [source["data"]["summary"].clone()]});
        let directory = cache.0.join("global-emerald_plus");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(
            directory.join("index.json"),
            serde_json::to_vec(&index).unwrap(),
        )
        .unwrap();
        std::fs::write(
            directory.join("117-support.json"),
            serde_json::to_vec(&source).unwrap(),
        )
        .unwrap();
        let fallback = load(
            &cache.0,
            "global",
            "emerald_plus",
            117,
            Some(Position::Jungle),
        )
        .await
        .unwrap();
        assert_eq!(fallback.position, Position::Support);
        assert_eq!(fallback.requested_position, Some(Position::Jungle));
        assert_eq!(fallback.provenance.cache_status, CacheStatus::Fresh);
    }

    #[test]
    fn archives_keep_patch_and_population_separate_without_adding_refresh_counts() {
        let cache = TestCache::new();
        let path = cache_path(&cache.0, "global", "emerald_plus", "498-adc.json");
        let mut previous = CacheRecord {
            schema_version: CACHE_SCHEMA_VERSION,
            source_url: champion_url("global", "emerald_plus", 498, Position::Adc),
            fetched_at_unix_ms: Some(1000),
            payload: serde_json::from_str(XAYAH_ADC).unwrap(),
        };
        previous.payload["meta"]["version"] = serde_json::json!("16.16");
        let mut current = previous.clone();
        current.payload["meta"]["version"] = serde_json::json!("16.17");
        current.fetched_at_unix_ms = Some(2000);
        store_cached(&path, &current, Some(&previous)).unwrap();
        let mut updated = current.clone();
        updated.payload["data"]["core_items"][0]["play"] = serde_json::json!(1543);
        updated.fetched_at_unix_ms = Some(3000);
        store_cached(&path, &updated, Some(&current)).unwrap();

        let kind = PayloadKind::Champion {
            key: 498,
            position: Position::Adc,
        };
        let older = read_cached(
            &path.parent().unwrap().join("patches/16.16/498-adc.json"),
            &current.source_url,
            kind,
        )
        .unwrap();
        let latest = read_cached(
            &path.parent().unwrap().join("patches/16.17/498-adc.json"),
            &current.source_url,
            kind,
        )
        .unwrap();
        assert_eq!(older.payload["data"]["core_items"][0]["play"], 1542);
        assert_eq!(latest.payload["data"]["core_items"][0]["play"], 1543);
        assert_eq!(
            read_cached(&path, &current.source_url, kind)
                .unwrap()
                .payload["data"]["core_items"][0]["play"],
            1543
        );
        let other_population = cache_path(&cache.0, "na", "diamond_plus", "498-adc.json");
        assert_ne!(
            archive_path(&other_population, &current.payload),
            archive_path(&path, &current.payload)
        );
        let mut unsafe_version = current.payload.clone();
        unsafe_version["meta"]["version"] = serde_json::json!("../../outside");
        assert_eq!(archive_path(&path, &unsafe_version), None);
        assert!(
            !path.parent().unwrap().read_dir().unwrap().any(|entry| entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|extension| extension == "tmp"))
        );
    }

    #[tokio::test]
    async fn failed_archive_write_leaves_the_active_good_cache_intact() {
        let cache = TestCache::new();
        cache.write_old(XAYAH_ADC);
        std::fs::write(cache.0.join("patches"), "not a directory").unwrap();
        let (url, server) = serve_once(XAYAH_ADC.to_string());
        let cached = cached_json(
            &cache.path(),
            &url,
            PayloadKind::Champion {
                key: 498,
                position: Position::Adc,
            },
        )
        .await
        .unwrap();
        server.join().unwrap();
        assert_eq!(cached.provenance.cache_status, CacheStatus::Network);
        assert!(cached.provenance.warning.is_some());
        assert_eq!(std::fs::read_to_string(cache.path()).unwrap(), XAYAH_ADC);
    }

    #[test]
    fn old_serialized_aggregates_keep_unknown_provenance_and_no_invented_alternatives() {
        let mut value = serde_json::to_value(xayah()).unwrap();
        value.as_object_mut().unwrap().remove("core_lines");
        value.as_object_mut().unwrap().remove("provenance");
        let aggregate: Aggregate = serde_json::from_value(value).unwrap();
        assert_eq!(aggregate.core.ids, vec![3032, 6675, 3031]);
        assert!(aggregate.core_lines.is_empty());
        assert_eq!(aggregate.provenance.cache_status, CacheStatus::Unknown);
        assert_eq!(aggregate.provenance.fetched_at_unix_ms, None);
    }

    #[test]
    fn provenance_treats_missing_future_and_expired_timestamps_as_unknown_or_stale() {
        let now = 30_000_000;
        let mut provenance = AggregateProvenance {
            schema_version: 1,
            fetched_at_unix_ms: None,
            cache_status: CacheStatus::Fresh,
            warning: None,
        };
        assert_eq!(provenance.age_at(now), None);
        assert!(!provenance.is_fresh_at(now));
        provenance.fetched_at_unix_ms = Some(now + 1);
        assert_eq!(provenance.age_at(now), None);
        assert!(!provenance.is_fresh_at(now));
        provenance.fetched_at_unix_ms = Some(now - 6 * 3600 * 1000);
        assert!(!provenance.is_fresh_at(now));
        provenance.fetched_at_unix_ms = Some(now - 6 * 3600 * 1000 + 1);
        assert!(provenance.is_fresh_at(now));
        provenance.cache_status = CacheStatus::Stale;
        assert!(!provenance.is_fresh_at(now));
    }

    #[test]
    fn decoding_a_fixture_does_not_invent_a_local_fetch_time() {
        let value = serde_json::to_value(xayah()).unwrap();
        assert_eq!(value["provenance"]["cache_status"], "Unknown");
        assert_eq!(value["provenance"]["fetched_at_unix_ms"], Value::Null);
    }

    #[test]
    fn positions_parse_from_both_naming_schemes() {
        assert_eq!(Position::parse("bottom"), Some(Position::Adc));
        assert_eq!(Position::parse("ADC"), Some(Position::Adc));
        assert_eq!(Position::parse("utility"), Some(Position::Support));
        assert_eq!(Position::parse("SUPPORT"), Some(Position::Support));
        assert_eq!(Position::parse("middle"), Some(Position::Mid));
        assert_eq!(Position::parse(""), None);
        assert_eq!(
            champion_url("global", "emerald_plus", 498, Position::Adc),
            "https://lol-api-champion.op.gg/api/global/champions/ranked/498/adc?tier=emerald_plus"
        );
    }

    #[test]
    fn decodes_the_real_xayah_response() {
        let a = xayah();
        assert_eq!(a.patch, "16.17");
        assert_eq!(
            a.spells.ids,
            vec![4, 21],
            "Flash + Barrier is the most picked"
        );
        assert!(a.spells.pick_rate > 0.8);
        let runes = a.runes.clone().unwrap();
        assert_eq!((runes.primary_style, runes.sub_style), (8000, 8300));
        assert_eq!(
            runes.perks,
            vec![8008, 8009, 9103, 8014, 8304, 8345, 5005, 5008, 5001]
        );
        assert_eq!(a.skill_order.iter().collect::<String>(), "QEWEEREWEWRWWQQ");
        assert_eq!(a.skill_max, vec!['E', 'W', 'Q']);
        assert_eq!(a.starters.ids, vec![1086, 2003, 2003]);
        // The 639-game alternative's higher observed rate does not establish a better build.
        assert_eq!(a.core_most_picked.ids, vec![3032, 6675, 3031]);
        assert_eq!(a.core.ids, vec![3032, 6675, 3031]);
        assert!(
            a.core_alternatives.contains(&3508),
            "Essence Reaver is the other first item: {:?}",
            a.core_alternatives
        );
        assert!(
            !a.core_alternatives.contains(&3036),
            "LDR as a third core item is not a first-item alternative"
        );
        assert_eq!(a.boots.clone().unwrap().ids, vec![3006]);
        assert_eq!(a.late[0].ids, vec![6675]);
        assert!(
            a.late.iter().any(|p| p.ids == vec![3036]),
            "LDR shows up as a late item"
        );
        assert_eq!(a.positions[0].position, Position::Adc);
        assert!(a.positions[0].role_rate > 0.9);
        assert_eq!(a.games, a.positions[0].games);
        assert!(a.win_rate > 0.4 && a.win_rate < 0.6);
        assert_eq!(a.counters[0].0, 222);
        assert_eq!(a.describe(), "op.gg emerald+ na, 6k games");
    }

    #[test]
    fn real_cross_role_responses_preserve_their_own_loadouts_and_samples() {
        let cases = [
            (
                include_str!("../../../m0/tests/fixtures/opgg_ahri_mid.json"),
                103,
                Position::Mid,
                [3118, 4645, 3157],
                13104,
                6893,
                [4, 14],
            ),
            (
                include_str!("../../../m0/tests/fixtures/opgg_ornn_top.json"),
                516,
                Position::Top,
                [3068, 3075, 6665],
                1328,
                803,
                [4, 12],
            ),
            (
                include_str!("../../../m0/tests/fixtures/opgg_darius_top.json"),
                122,
                Position::Top,
                [3142, 3742, 6333],
                10756,
                6289,
                [4, 6],
            ),
            (
                include_str!("../../../m0/tests/fixtures/opgg_lulu_support.json"),
                117,
                Position::Support,
                [3504, 6617, 2065],
                6405,
                3848,
                [4, 7],
            ),
            (
                include_str!("../../../m0/tests/fixtures/opgg_leesin_jungle.json"),
                64,
                Position::Jungle,
                [6692, 6610, 6333],
                35055,
                19179,
                [4, 11],
            ),
            (
                include_str!("../../../m0/tests/fixtures/opgg_aphelios_adc.json"),
                523,
                Position::Adc,
                [6676, 3031, 3036],
                10752,
                6177,
                [4, 21],
            ),
            (
                include_str!("../../../m0/tests/fixtures/opgg_udyr_jungle.json"),
                77,
                Position::Jungle,
                [3161, 3073, 6333],
                3758,
                2171,
                [4, 11],
            ),
        ];
        for (raw, key, role, core, games, wins, spells) in cases {
            let mut value: Value = serde_json::from_str(raw).unwrap();
            for field in [
                "summoner_spells",
                "runes",
                "skills",
                "skill_masteries",
                "starter_items",
                "core_items",
                "boots",
                "last_items",
            ] {
                value["data"][field].as_array_mut().unwrap().reverse();
            }
            PayloadKind::Champion {
                key,
                position: role,
            }
            .validate(&value)
            .unwrap();
            let aggregate = decode(&value, key, role, "global", "emerald_plus").unwrap();
            assert_eq!((aggregate.champion_key, aggregate.position), (key, role));
            assert_eq!(aggregate.patch, "16.17");
            assert_eq!(aggregate.core.ids, core, "champion {key}");
            assert_eq!(
                (aggregate.core.games, aggregate.core.wins),
                (games, wins),
                "champion {key}"
            );
            assert_eq!(aggregate.core, aggregate.core_most_picked);
            assert_eq!(aggregate.spells.ids, spells, "champion {key}");
            assert_eq!(aggregate.runes.as_ref().unwrap().perks.len(), 9);
            assert!(aggregate
                .core_lines
                .windows(2)
                .all(|pair| pair[0].games >= pair[1].games));
        }
    }

    #[test]
    fn position_choice_never_substitutes_another_role_for_an_assignment() {
        let mut stats = xayah().positions;
        assert_eq!(
            choose_position(Some(Position::Adc), &stats),
            Some(Position::Adc)
        );
        assert_eq!(choose_position(Some(Position::Support), &stats), None);
        assert_eq!(choose_position(None, &stats), Some(Position::Adc));
        assert_eq!(choose_position(Some(Position::Mid), &[]), None);
        assert_eq!(choose_position(None, &[]), None);
        stats.push(PositionStat {
            position: Position::Support,
            games: 12,
            role_rate: 0.001,
            win_rate: 0.25,
        });
        assert_eq!(
            choose_position(Some(Position::Support), &stats),
            Some(Position::Support)
        );
    }

    #[test]
    fn small_role_samples_are_visible_without_changing_the_role() {
        let mut v: Value = serde_json::from_str(XAYAH_ADC).unwrap();
        v["data"]["summary"]["positions"][0]["stats"]["play"] = serde_json::json!(87);
        let aggregate = decode(&v, 498, Position::Adc, "na", "emerald_plus").unwrap();
        assert_eq!(aggregate.position, Position::Adc);
        assert_eq!(aggregate.games, 87);
        assert!(aggregate
            .provenance
            .warning
            .as_deref()
            .is_some_and(|warning| warning.contains("87")));
    }

    #[test]
    fn decode_cannot_relabel_another_roles_observations() {
        let value: Value = serde_json::from_str(XAYAH_ADC).unwrap();
        assert!(decode(&value, 498, Position::Support, "global", "emerald_plus").is_err());
        assert!(decode(&value, 498, Position::Jungle, "global", "emerald_plus").is_err());
    }

    #[test]
    fn zero_game_rows_cannot_become_a_recommended_loadout() {
        let mut v: Value = serde_json::from_str(XAYAH_ADC).unwrap();
        for key in [
            "summoner_spells",
            "runes",
            "skills",
            "skill_masteries",
            "starter_items",
            "core_items",
            "boots",
            "last_items",
            "counters",
        ] {
            for row in v["data"][key].as_array_mut().unwrap() {
                row["play"] = serde_json::json!(0);
                row["win"] = serde_json::json!(0);
                if key != "counters" {
                    row["pick_rate"] = serde_json::json!(0);
                }
            }
        }
        assert!(decode(&v, 498, Position::Adc, "na", "emerald_plus").is_err());
    }

    #[test]
    fn core_choice_preserves_popularity_even_when_an_alternative_wins_more() {
        let line = |ids: &[u32], games: u32, wins: u32, pick: f64| Picked {
            ids: ids.to_vec(),
            games,
            wins,
            pick_rate: pick,
        };
        let most = line(&[1, 2, 3], 1500, 855, 0.33); // 57.0%
                                                      // Better but too rare (6%), better but too few games, better by only 1 point: all rejected.
        for other in [
            line(&[1, 3, 4], 260, 170, 0.06),
            line(&[1, 3, 2], 400, 260, 0.12),
            line(&[1, 3, 2], 700, 406, 0.15),
        ] {
            assert_eq!(choose_core(&[most.clone(), other]).0.ids, vec![1, 2, 3]);
        }
        // These are observational samples, not a controlled comparison of item strength.
        let better = line(&[1, 3, 2], 640, 384, 0.14); // 60.0%
        let (chosen, mp) = choose_core(&[most.clone(), better.clone()]);
        assert_eq!((chosen.ids, mp.ids), (vec![1, 2, 3], vec![1, 2, 3]));
        assert_eq!(choose_core(&[better, most]).0.ids, vec![1, 2, 3]);
        assert_eq!(choose_core(&[]).0.ids, Vec::<u32>::new());
    }

    #[test]
    fn shuffled_source_lists_still_select_the_most_played_loadout() {
        let mut v: Value = serde_json::from_str(XAYAH_ADC).unwrap();
        for key in [
            "summoner_spells",
            "runes",
            "skills",
            "skill_masteries",
            "starter_items",
            "core_items",
            "boots",
            "last_items",
        ] {
            v["data"][key].as_array_mut().unwrap().reverse();
        }
        let actual = decode(&v, 498, Position::Adc, "na", "emerald_plus").unwrap();
        let expected = xayah();
        assert_eq!(actual.spells.ids, vec![4, 21]);
        assert_eq!(actual.runes, expected.runes);
        assert_eq!(actual.skill_order, expected.skill_order);
        assert_eq!(actual.skill_max, expected.skill_max);
        assert_eq!(actual.starters, expected.starters);
        assert_eq!(actual.core.ids, vec![3032, 6675, 3031]);
        assert_eq!(actual.boots, expected.boots);
        assert_eq!(actual.late, expected.late);
    }

    #[test]
    fn all_core_lines_retain_their_own_observed_samples() {
        let value = serde_json::to_value(xayah()).unwrap();
        assert_eq!(value["core_lines"][0]["games"], 1542);
        assert_eq!(value["core_lines"][0]["wins"], 882);
        let alternate = value["core_lines"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["ids"] == serde_json::json!([3032, 3031, 6675]))
            .unwrap();
        assert_eq!(alternate["games"], 639);
        assert_eq!(alternate["wins"], 384);
    }

    #[test]
    fn malformed_counts_and_rates_do_not_become_build_evidence() {
        for (key, value) in [
            ("play", serde_json::json!(-1)),
            ("play", serde_json::json!(1.5)),
            ("play", serde_json::json!(4_294_967_296_u64)),
            ("win", serde_json::json!(2000)),
            ("win", serde_json::json!(null)),
            ("pick_rate", serde_json::json!(-0.1)),
            ("pick_rate", serde_json::json!(1.1)),
            ("pick_rate", serde_json::json!(null)),
        ] {
            let mut v: Value = serde_json::from_str(XAYAH_ADC).unwrap();
            v["data"]["core_items"][0][key] = value.clone();
            assert!(
                decode(&v, 498, Position::Adc, "na", "emerald_plus").is_err(),
                "accepted {key}={value}"
            );
        }
    }

    #[test]
    fn nine_perks_alone_do_not_make_a_valid_rune_page() {
        let cases = [
            (
                "primary_rune_ids",
                serde_json::json!([8008, 8009, 9103, 9103]),
            ),
            ("primary_page_id", serde_json::json!(0)),
            ("secondary_page_id", serde_json::json!(8000)),
            ("secondary_rune_ids", serde_json::json!([8304, 0])),
            ("stat_mod_ids", serde_json::json!([5005, 5008, 8008])),
        ];
        for (key, value) in cases {
            let mut v: Value = serde_json::from_str(XAYAH_ADC).unwrap();
            v["data"]["runes"][0][key] = value;
            assert!(
                decode(&v, 498, Position::Adc, "na", "emerald_plus").is_err(),
                "accepted invalid {key}"
            );
        }
        let mut v: Value = serde_json::from_str(XAYAH_ADC).unwrap();
        v["data"]["runes"][0]["primary_rune_ids"] = serde_json::json!([8008, 8009, 9103]);
        v["data"]["runes"][0]["secondary_rune_ids"] = serde_json::json!([8304, 8345, 8347]);
        assert!(decode(&v, 498, Position::Adc, "na", "emerald_plus").is_err());
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(decode(
            &serde_json::json!({"data": {}}),
            1,
            Position::Top,
            "na",
            "all"
        )
        .is_err());
        assert!(decode(&serde_json::json!([1, 2]), 1, Position::Top, "na", "all").is_err());
    }
}
