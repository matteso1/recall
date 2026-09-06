//! Live Client Data API (https://127.0.0.1:2999/liveclientdata) - only up while a game runs.
//! Everything here is information the player can already see by pressing Tab.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

pub struct LiveClient {
    http: reqwest::Client,
    base: String,
}

impl LiveClient {
    pub fn new() -> anyhow::Result<LiveClient> {
        Ok(LiveClient {
            http: reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .connect_timeout(Duration::from_millis(500))
                .timeout(Duration::from_millis(1600))
                .build()?,
            base: "https://127.0.0.1:2999/liveclientdata".to_string(),
        })
    }

    /// Full game state, or `None` when no game is running (loading screen included).
    pub async fn all_game_data(&self) -> Option<Value> {
        let resp = self
            .http
            .get(format!("{}/allgamedata", self.base))
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let v: Value = resp.json().await.ok()?;
        if v.get("activePlayer").is_some() {
            Some(v)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct InvItem {
    pub id: u32,
    pub name: String,
    pub count: u32,
    pub slot: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Player {
    pub name: String,
    pub champion: String,
    pub team: String,
    pub position: String,
    pub level: u32,
    pub items: Vec<InvItem>,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub cs: u32,
}

impl Player {
    pub fn kda(&self) -> String {
        format!("{}/{}/{}", self.kills, self.deaths, self.assists)
    }

    pub fn item_count(&self, id: u32) -> u32 {
        self.items
            .iter()
            .filter(|i| i.id == id)
            .map(|i| i.count)
            .sum()
    }

    pub fn has_item(&self, id: u32) -> bool {
        self.item_count(id) > 0
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Abilities {
    pub q: u8,
    pub w: u8,
    pub e: u8,
    pub r: u8,
}

impl Abilities {
    pub fn total(&self) -> u8 {
        self.q + self.w + self.e + self.r
    }

    pub fn get(&self, key: char) -> u8 {
        match key.to_ascii_uppercase() {
            'Q' => self.q,
            'W' => self.w,
            'E' => self.e,
            'R' => self.r,
            _ => 0,
        }
    }
}

/// Only the active player's visible combat stats. Missing/invalid numbers remain unknown.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CombatStats {
    pub current_health: Option<f64>,
    pub max_health: Option<f64>,
    pub attack_damage: Option<f64>,
    pub ability_power: Option<f64>,
    pub armor: Option<f64>,
    pub magic_resist: Option<f64>,
    pub attack_speed: Option<f64>,
    pub crit_chance: Option<f64>,
    pub attack_range: Option<f64>,
    pub ability_haste: Option<f64>,
    pub life_steal: Option<f64>,
    pub movement_speed: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Me {
    pub player: Player,
    pub gold: f64,
    pub abilities: Abilities,
    /// Actual selected perks and shards; `None` means the API did not report a usable full page.
    #[serde(default)]
    pub rune_ids: Option<Vec<u32>>,
    /// Actual summoner spells in D/F order; missing or unknown spells are omitted.
    #[serde(default)]
    pub spell_ids: Vec<u32>,
    #[serde(default)]
    pub stats: CombatStats,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct LiveSnapshot {
    pub game_time: f64,
    pub mode: String,
    pub me: Option<Me>,
    pub allies: Vec<Player>,
    pub enemies: Vec<Player>,
}

fn u32_of(v: &Value, key: &str) -> u32 {
    finite_of(v, key)
        .filter(|n| *n >= 0.0 && *n <= u32::MAX as f64 && n.fract() == 0.0)
        .unwrap_or(0.0) as u32
}

fn finite_of(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64).filter(|n| n.is_finite())
}

fn str_of(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

/// Practice Tool and non-SR modes report "NONE"; normalise to "".
fn position(raw: &str) -> String {
    let p = raw.to_ascii_uppercase();
    if p == "NONE" {
        String::new()
    } else {
        p
    }
}

pub fn parse_player(p: &Value) -> Player {
    let scores = p.get("scores").cloned().unwrap_or(Value::Null);
    let mut items: Vec<InvItem> = p
        .get("items")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|i| InvItem {
                    id: u32_of(i, "itemID"),
                    name: str_of(i, "displayName"),
                    count: u32_of(i, "count").max(1),
                    slot: u32_of(i, "slot"),
                })
                .collect()
        })
        .unwrap_or_default();
    items.sort_by_key(|i| i.slot);
    let name = {
        let riot = str_of(p, "riotId");
        if riot.is_empty() {
            str_of(p, "summonerName")
        } else {
            riot
        }
    };
    Player {
        name,
        champion: str_of(p, "championName"),
        team: str_of(p, "team"),
        position: position(&str_of(p, "position")),
        level: u32_of(p, "level"),
        items,
        kills: u32_of(&scores, "kills"),
        deaths: u32_of(&scores, "deaths"),
        assists: u32_of(&scores, "assists"),
        cs: u32_of(&scores, "creepScore"),
    }
}

fn identity_matches(active: &Value, player: &Value) -> bool {
    let present = |v: &Value, key: &str| {
        v.get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    if let (Some(a), Some(p)) = (present(active, "riotId"), present(player, "riotId")) {
        return a == p;
    }
    ["riotId", "summonerName"].iter().any(|a| {
        present(active, a).is_some_and(|name| {
            ["riotId", "summonerName"]
                .iter()
                .any(|p| present(player, p).as_deref() == Some(name.as_str()))
        })
    })
}

fn actual_runes(active: &Value) -> Option<Vec<u32>> {
    let full = active.get("fullRunes")?.as_object()?;
    let general = full.get("generalRunes")?.as_array()?;
    let shards = full.get("statRunes")?.as_array()?;
    let mut ids = Vec::with_capacity(general.len() + shards.len());
    for rune in general.iter().chain(shards) {
        let id = u32_of(rune, "id");
        if id == 0 {
            return None;
        }
        ids.push(id);
    }
    Some(ids)
}

fn actual_spell_id(spell: &Value) -> Option<u32> {
    for key in ["id", "spellId", "spellID"] {
        let id = u32_of(spell, key);
        if id > 0 {
            return Some(id);
        }
    }
    // Generated identifiers do not change with the player's display language.
    for key in ["rawDisplayName", "rawDescription", "id"] {
        for token in spell
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .split('_')
        {
            let id = match token {
                "SummonerBoost" => 1,
                "SummonerExhaust" => 3,
                "SummonerFlash" => 4,
                "SummonerHaste" => 6,
                "SummonerHeal" => 7,
                "SummonerSmite"
                | "SummonerSmiteAvatarOffensive"
                | "SummonerSmiteAvatarDefensive"
                | "SummonerSmiteAvatarUtility"
                | "SummonerSmitePlayerGanker"
                | "SummonerSmiteDuel" => 11,
                "SummonerTeleport" => 12,
                "SummonerMana" => 13,
                "SummonerDot" => 14,
                "SummonerBarrier" => 21,
                "SummonerPoroRecall" => 30,
                "SummonerPoroThrow" => 31,
                "SummonerSnowball" => 32,
                _ => continue,
            };
            return Some(id);
        }
    }
    crate::runes::spell_id(&str_of(spell, "displayName")).and_then(|id| u32::try_from(id).ok())
}

fn actual_stats(active: &Value) -> CombatStats {
    let s = &active["championStats"];
    CombatStats {
        current_health: finite_of(s, "currentHealth").filter(|n| *n >= 0.0),
        max_health: finite_of(s, "maxHealth").filter(|n| *n > 0.0),
        attack_damage: finite_of(s, "attackDamage"),
        ability_power: finite_of(s, "abilityPower"),
        armor: finite_of(s, "armor"),
        magic_resist: finite_of(s, "magicResist"),
        attack_speed: finite_of(s, "attackSpeed"),
        crit_chance: finite_of(s, "critChance"),
        attack_range: finite_of(s, "attackRange"),
        ability_haste: finite_of(s, "abilityHaste"),
        life_steal: finite_of(s, "lifeSteal"),
        movement_speed: finite_of(s, "moveSpeed"),
    }
}

pub fn summarize(data: &Value) -> LiveSnapshot {
    let active = &data["activePlayer"];
    let game = &data["gameData"];
    let players = data
        .get("allPlayers")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let matches: Vec<usize> = players
        .iter()
        .enumerate()
        .filter_map(|(i, p)| identity_matches(active, p).then_some(i))
        .collect();
    let own_index = (matches.len() == 1).then(|| matches[0]);
    let me = own_index.map(|i| {
        let ab = &active["abilities"];
        let level = |k: &str| {
            ab.get(k)
                .map(|a| u32_of(a, "abilityLevel"))
                .filter(|n| *n <= 6)
                .unwrap_or(0) as u8
        };
        let spells = &players[i]["summonerSpells"];
        Me {
            player: parse_player(&players[i]),
            gold: finite_of(active, "currentGold").unwrap_or(0.0).max(0.0),
            abilities: Abilities {
                q: level("Q"),
                w: level("W"),
                e: level("E"),
                r: level("R"),
            },
            rune_ids: actual_runes(active),
            spell_ids: ["summonerSpellOne", "summonerSpellTwo"]
                .iter()
                .filter_map(|k| actual_spell_id(&spells[*k]))
                .collect(),
            stats: actual_stats(active),
        }
    });
    let mut allies = Vec::new();
    let mut enemies = Vec::new();
    if let Some(m) = &me {
        if matches!(m.player.team.as_str(), "ORDER" | "CHAOS") {
            for (i, raw) in players.iter().enumerate() {
                if Some(i) == own_index {
                    continue;
                }
                let p = parse_player(raw);
                if !matches!(p.team.as_str(), "ORDER" | "CHAOS") {
                    continue;
                }
                if p.team == m.player.team {
                    allies.push(p);
                } else {
                    enemies.push(p);
                }
            }
        }
    }
    LiveSnapshot {
        game_time: finite_of(game, "gameTime").unwrap_or(0.0).max(0.0),
        mode: str_of(game, "gameMode"),
        me,
        allies,
        enemies,
    }
}

pub fn fmt_time(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRACTICE: &str = include_str!("../../../m0/tests/fixtures/allgamedata_practicetool.json");
    const FULL: &str = include_str!("../../../m0/tests/fixtures/allgamedata.json");

    #[test]
    fn summarizes_real_practice_tool_capture() {
        let s = summarize(&serde_json::from_str(PRACTICE).unwrap());
        let me = s.me.expect("me");
        assert_eq!(s.mode, "PRACTICETOOL");
        assert_eq!(me.player.champion, "Xayah");
        assert_eq!(me.player.position, "");
        assert_eq!(
            me.abilities,
            Abilities {
                q: 1,
                w: 0,
                e: 0,
                r: 0
            }
        );
        assert!((me.gold - 613.2).abs() < 1.0);
        assert_eq!(me.player.items.len(), 1);
        assert!(me.player.has_item(2010));
        assert_eq!(fmt_time(s.game_time), "02:00");
    }

    #[test]
    fn splits_teams_in_a_full_game() {
        let s = summarize(&serde_json::from_str(FULL).unwrap());
        assert_eq!(s.allies.len(), 4);
        assert_eq!(
            s.enemies
                .iter()
                .map(|p| p.champion.as_str())
                .collect::<Vec<_>>(),
            vec!["Tristana", "Soraka", "Malphite", "Ornn", "Thresh"]
        );
        let me = s.me.unwrap();
        assert_eq!(me.player.position, "BOTTOM");
        assert_eq!(me.player.kda(), "0/0/0");
    }

    #[test]
    fn retains_actual_loadout_and_own_combat_stats() {
        let s = summarize(&serde_json::from_str(PRACTICE).unwrap());
        let me = serde_json::to_value(s.me.unwrap()).unwrap();
        assert_eq!(
            me.get("rune_ids"),
            Some(&serde_json::json!([
                8008, 8009, 9103, 8014, 8304, 8345, 5005, 5008, 5001
            ]))
        );
        assert_eq!(me.get("spell_ids"), Some(&serde_json::json!([4, 21])));
        let stats = me.get("stats").expect("own combat stats are retained");
        assert_eq!(stats.get("armor"), Some(&serde_json::json!(25.0)));
        assert_eq!(stats.get("max_health"), Some(&serde_json::json!(640.0)));
        assert_eq!(stats.get("magic_resist"), Some(&serde_json::json!(33.0)));
    }

    #[test]
    fn missing_and_explicitly_empty_runes_remain_distinct() {
        let mut data: Value = serde_json::from_str(PRACTICE).unwrap();
        data["activePlayer"]
            .as_object_mut()
            .unwrap()
            .remove("fullRunes");
        let missing = serde_json::to_value(summarize(&data).me.unwrap()).unwrap();
        assert_eq!(missing.get("rune_ids"), Some(&Value::Null));

        data["activePlayer"]["fullRunes"] =
            serde_json::json!({"generalRunes": [], "statRunes": []});
        let empty = serde_json::to_value(summarize(&data).me.unwrap()).unwrap();
        assert_eq!(empty.get("rune_ids"), Some(&serde_json::json!([])));
    }

    #[test]
    fn missing_own_identity_does_not_turn_everyone_into_enemies() {
        let mut data: Value = serde_json::from_str(FULL).unwrap();
        data["activePlayer"] = serde_json::json!({"currentGold": 500.0});
        let s = summarize(&data);
        assert!(s.me.is_none());
        assert!(s.allies.is_empty());
        assert!(s.enemies.is_empty());
    }

    #[test]
    fn ambiguous_own_identity_does_not_choose_the_first_player() {
        let mut data: Value = serde_json::from_str(PRACTICE).unwrap();
        let duplicate = data["allPlayers"][0].clone();
        data["allPlayers"].as_array_mut().unwrap().push(duplicate);
        let s = summarize(&data);
        assert!(s.me.is_none());
        assert!(s.enemies.is_empty());
    }

    #[test]
    fn unknown_teams_are_not_reported_as_opponents() {
        let mut data: Value = serde_json::from_str(FULL).unwrap();
        data["allPlayers"][0]["team"] = Value::Null;
        let s = summarize(&data);
        assert!(s.enemies.iter().all(|p| p.champion != "Nasus"));
    }

    #[test]
    fn stable_spell_identifiers_work_with_localized_names() {
        let mut data: Value = serde_json::from_str(PRACTICE).unwrap();
        data["allPlayers"][0]["summonerSpells"]["summonerSpellOne"]["displayName"] =
            serde_json::json!("Saut éclair");
        data["allPlayers"][0]["summonerSpells"]["summonerSpellTwo"]["displayName"] =
            serde_json::json!("Barrière");
        let me = serde_json::to_value(summarize(&data).me.unwrap()).unwrap();
        assert_eq!(me.get("spell_ids"), Some(&serde_json::json!([4, 21])));
    }

    #[test]
    fn invalid_telemetry_stays_unknown_and_does_not_wrap_ability_ranks() {
        let mut data: Value = serde_json::from_str(PRACTICE).unwrap();
        data["activePlayer"]["championStats"]["armor"] = serde_json::json!("NaN");
        data["activePlayer"]["championStats"]["maxHealth"] = Value::Null;
        data["activePlayer"]["currentGold"] = serde_json::json!(-100.0);
        data["activePlayer"]["abilities"]["Q"]["abilityLevel"] = serde_json::json!(257);
        data["gameData"]["gameTime"] = serde_json::json!(-10.0);
        let s = summarize(&data);
        assert_eq!(s.game_time, 0.0);
        let me = s.me.unwrap();
        assert_eq!(me.gold, 0.0);
        assert_eq!(me.abilities.q, 0);
        let wire = serde_json::to_value(me).unwrap();
        assert_eq!(wire["stats"]["armor"], Value::Null);
        assert_eq!(wire["stats"]["max_health"], Value::Null);
    }
}
