//! Live Client Data API (https://127.0.0.1:2999/liveclientdata) - only up while a game runs.
//! Everything here is information the player can already see by pressing Tab.
use serde::Serialize;
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
                .timeout(Duration::from_secs(4))
                .build()?,
            base: "https://127.0.0.1:2999/liveclientdata".to_string(),
        })
    }

    /// Full game state, or `None` when no game is running (loading screen included).
    pub async fn all_game_data(&self) -> Option<Value> {
        let resp = self.http.get(format!("{}/allgamedata", self.base)).send().await.ok()?;
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

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct InvItem {
    pub id: u32,
    pub name: String,
    pub count: u32,
    pub slot: u32,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
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
        self.items.iter().filter(|i| i.id == id).map(|i| i.count).sum()
    }

    pub fn has_item(&self, id: u32) -> bool {
        self.item_count(id) > 0
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq)]
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

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Me {
    pub player: Player,
    pub gold: f64,
    pub abilities: Abilities,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct LiveSnapshot {
    pub game_time: f64,
    pub mode: String,
    pub me: Option<Me>,
    pub allies: Vec<Player>,
    pub enemies: Vec<Player>,
}

fn u32_of(v: &Value, key: &str) -> u32 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0) as u32
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

pub fn summarize(data: &Value) -> LiveSnapshot {
    let active = data.get("activePlayer").cloned().unwrap_or(Value::Null);
    let my_id = {
        let riot = str_of(&active, "riotId");
        if riot.is_empty() {
            str_of(&active, "summonerName")
        } else {
            riot
        }
    };
    let game = data.get("gameData").cloned().unwrap_or(Value::Null);

    let mut me: Option<Me> = None;
    let mut others: Vec<Player> = Vec::new();
    for p in data.get("allPlayers").and_then(Value::as_array).into_iter().flatten() {
        let player = parse_player(p);
        if !my_id.is_empty() && player.name == my_id && me.is_none() {
            let ab = active.get("abilities").cloned().unwrap_or(Value::Null);
            let level = |k: &str| ab.get(k).and_then(|a| a.get("abilityLevel")).and_then(Value::as_u64).unwrap_or(0) as u8;
            me = Some(Me {
                player,
                gold: active.get("currentGold").and_then(Value::as_f64).unwrap_or(0.0),
                abilities: Abilities { q: level("Q"), w: level("W"), e: level("E"), r: level("R") },
            });
        } else {
            others.push(player);
        }
    }
    let my_team = me.as_ref().map(|m| m.player.team.clone());
    let (allies, enemies): (Vec<Player>, Vec<Player>) = others
        .into_iter()
        .partition(|p| my_team.as_deref().map(|t| p.team == t).unwrap_or(false));

    LiveSnapshot {
        game_time: game.get("gameTime").and_then(Value::as_f64).unwrap_or(0.0),
        mode: str_of(&game, "gameMode"),
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
        assert_eq!(me.abilities, Abilities { q: 1, w: 0, e: 0, r: 0 });
        assert!((me.gold - 613.2).abs() < 1.0);
        assert_eq!(me.player.items.len(), 1);
        assert!(me.player.has_item(2010));
        assert_eq!(fmt_time(s.game_time), "02:00");
    }

    #[test]
    fn splits_teams_in_a_full_game() {
        let s = summarize(&serde_json::from_str(FULL).unwrap());
        assert_eq!(s.allies.len(), 4);
        assert_eq!(s.enemies.iter().map(|p| p.champion.as_str()).collect::<Vec<_>>(),
                   vec!["Tristana", "Soraka", "Malphite", "Ornn", "Thresh"]);
        let me = s.me.unwrap();
        assert_eq!(me.player.position, "BOTTOM");
        assert_eq!(me.player.kda(), "0/0/0");
    }
}
