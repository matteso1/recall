//! Every game Recall watches is saved for later analysis, one file per game:
//! `%LOCALAPPDATA%\Recall\games\<UTC start>-<champion>.jsonl`. The first line holds the champion
//! select that led to the game; every later line is one Live Client Data observation (the
//! scoreboard view, nothing hidden) with the recommendation the panel showed at that moment. A line
//! is written when the recommendation or anyone's items or level change, and at least every 20 s of
//! game time. Runes and summoner spells are kept only in the first observation and each event is
//! written once, so a game is about a megabyte. The newest 40 games are kept. The files stay on
//! this PC; they carry the players' Riot IDs as the scoreboard shows them.
use serde_json::{json, Value};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const KEEP_GAMES: usize = 40;
const EVERY_GAME_SECONDS: f64 = 20.0;
/// No observation for this long means the next one belongs to another game (a remake included).
const NEW_GAME_GAP_MS: u64 = 3 * 60 * 1000;
/// A champion select this old no longer belongs to the game that starts.
const CHAMPSELECT_MAX_AGE_MS: u64 = 15 * 60 * 1000;

pub struct GameRecorder {
    dir: PathBuf,
    file: Option<File>,
    champion: String,
    last_game_time: f64,
    last_received_at_ms: u64,
    last_signature: String,
    last_panel: String,
    last_event_id: i64,
    champselect: Option<(u64, Value)>,
}

impl GameRecorder {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            file: None,
            champion: String::new(),
            last_game_time: f64::NEG_INFINITY,
            last_received_at_ms: 0,
            last_signature: String::new(),
            last_panel: String::new(),
            last_event_id: -1,
            champselect: None,
        }
    }

    /// Keep the latest champion select, without its chat credentials, for the next game's header.
    pub fn observe_champselect(&mut self, session: &Value, now_ms: u64) {
        let mut session = session.clone();
        if let Some(object) = session.as_object_mut() {
            object.remove("chatDetails");
        }
        self.champselect = Some((now_ms, session));
    }

    /// One live observation; writes only when something worth analysing changed.
    pub fn observe_live(&mut self, data: &Value, panel: Option<&str>, received_at_ms: u64) {
        if let Err(error) = self.record(data, panel.unwrap_or_default(), received_at_ms) {
            log::warn!("game recording paused for this game: {error}");
            self.file = None;
        }
    }

    fn record(&mut self, data: &Value, panel: &str, received_at_ms: u64) -> std::io::Result<()> {
        let Some(game_time) = data["gameData"]["gameTime"]
            .as_f64()
            .filter(|time| time.is_finite() && *time >= 0.0)
        else {
            return Ok(());
        };
        let Some(champion) = champion_of(data) else {
            return Ok(());
        };
        let signature = signature(data);
        let new_game = self.file.is_none()
            || champion != self.champion
            || game_time + 5.0 < self.last_game_time
            || received_at_ms.saturating_sub(self.last_received_at_ms) > NEW_GAME_GAP_MS;
        self.last_received_at_ms = received_at_ms;
        if new_game {
            self.start(&champion, received_at_ms)?;
        } else if game_time < self.last_game_time + EVERY_GAME_SECONDS
            && signature == self.last_signature
            && panel == self.last_panel
        {
            return Ok(());
        }
        let (data, newest_event) = compact(data, self.last_event_id, new_game);
        let line = json!({
            "kind": "live",
            "received_at_ms": received_at_ms,
            "game_time": game_time,
            "panel": panel,
            "data": data,
        });
        if let Some(file) = self.file.as_mut() {
            writeln!(file, "{line}")?;
        }
        self.last_game_time = game_time;
        self.last_signature = signature;
        self.last_panel = panel.to_string();
        self.last_event_id = newest_event;
        Ok(())
    }

    fn start(&mut self, champion: &str, now_ms: u64) -> std::io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let name = format!("{}-{}.jsonl", utc_stamp(now_ms), file_safe(champion));
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join(&name))?;
        let champselect = self
            .champselect
            .take()
            .filter(|(at, _)| now_ms.saturating_sub(*at) <= CHAMPSELECT_MAX_AGE_MS)
            .map(|(_, session)| session);
        let header = json!({
            "kind": "game",
            "started_at_ms": now_ms,
            "champion": champion,
            "recall_version": env!("CARGO_PKG_VERSION"),
            "champselect": champselect,
        });
        writeln!(file, "{header}")?;
        self.file = Some(file);
        self.champion = champion.to_string();
        self.last_game_time = f64::NEG_INFINITY;
        self.last_signature.clear();
        self.last_panel.clear();
        self.last_event_id = -1;
        prune(&self.dir, KEEP_GAMES);
        log::info!("recording this game to games/{name}");
        Ok(())
    }
}

/// The active player's champion, found by matching the Riot ID on the scoreboard.
fn champion_of(data: &Value) -> Option<String> {
    let me = &data["activePlayer"];
    let id = me["riotId"].as_str().or(me["summonerName"].as_str())?;
    data["allPlayers"]
        .as_array()?
        .iter()
        .find(|player| {
            player["riotId"].as_str() == Some(id) || player["summonerName"].as_str() == Some(id)
        })
        .and_then(|player| player["championName"].as_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

/// Everyone's level and items: a change here is worth a line.
fn signature(data: &Value) -> String {
    let mut out = String::new();
    for player in data["allPlayers"].as_array().into_iter().flatten() {
        out.push_str(player["championName"].as_str().unwrap_or("?"));
        out.push_str(&format!(":{}:", player["level"]));
        for item in player["items"].as_array().into_iter().flatten() {
            out.push_str(&format!("{}x{},", item["itemID"], item["count"]));
        }
        out.push('|');
    }
    out
}

/// Events already written are dropped; after the first observation so is static data.
fn compact(data: &Value, last_event_id: i64, first: bool) -> (Value, i64) {
    let mut out = data.clone();
    let mut newest = last_event_id;
    if let Some(events) = out
        .pointer_mut("/events/Events")
        .and_then(Value::as_array_mut)
    {
        events.retain(|event| {
            event["EventID"]
                .as_i64()
                .is_some_and(|id| id > last_event_id)
        });
        if let Some(id) = events
            .iter()
            .filter_map(|event| event["EventID"].as_i64())
            .max()
        {
            newest = newest.max(id);
        }
    }
    if !first {
        if let Some(active) = out.get_mut("activePlayer").and_then(Value::as_object_mut) {
            active.remove("fullRunes");
        }
        for player in out
            .get_mut("allPlayers")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            if let Some(player) = player.as_object_mut() {
                player.remove("runes");
                player.remove("summonerSpells");
            }
        }
    }
    (out, newest)
}

/// Oldest files go first: names start with a sortable UTC time.
fn prune(dir: &Path, keep: usize) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut games: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    games.sort();
    let excess = games.len().saturating_sub(keep);
    for old in &games[..excess] {
        let _ = fs::remove_file(old);
    }
}

fn file_safe(champion: &str) -> String {
    champion
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

/// `20260926-220909Z`: UTC, sortable, no locale or time-zone lookups.
fn utc_stamp(epoch_ms: u64) -> String {
    let secs = epoch_ms / 1000;
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(game_time: f64, my_items: &[u64], events: &[i64]) -> Value {
        json!({
            "activePlayer": {"riotId": "me#NA1", "fullRunes": {"keystone": {"id": 8229}}},
            "allPlayers": [
                {"championName": "Malphite", "riotId": "me#NA1", "level": 11,
                 "runes": {"keystone": {"id": 8229}}, "summonerSpells": {},
                 "items": my_items.iter().map(|id| json!({"itemID": id, "count": 1})).collect::<Vec<_>>()},
                {"championName": "Teemo", "riotId": "them#NA1", "level": 12, "items": []}
            ],
            "events": {"Events": events.iter().map(|id| json!({"EventID": id})).collect::<Vec<_>>()},
            "gameData": {"gameTime": game_time}
        })
    }

    fn lines(dir: &Path) -> Vec<Vec<Value>> {
        let mut files: Vec<PathBuf> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        files.sort();
        files
            .iter()
            .map(|file| {
                fs::read_to_string(file)
                    .unwrap()
                    .lines()
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect()
            })
            .collect()
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("recall-recorder-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn utc_stamps_sort_and_match_the_calendar() {
        assert_eq!(utc_stamp(0), "19700101-000000Z");
        assert_eq!(utc_stamp(1_790_460_549_000), "20260926-220909Z");
        assert_eq!(utc_stamp(951_782_400_000), "20000229-000000Z");
        assert_eq!(utc_stamp(4_107_542_399_000), "21000228-235959Z");
    }

    #[test]
    fn a_game_is_one_file_written_on_change_or_every_twenty_seconds() {
        let dir = scratch("game");
        let mut recorder = GameRecorder::new(dir.clone());
        recorder.observe_champselect(&json!({"myTeam": [], "chatDetails": {"token": "x"}}), 1_000);
        recorder.observe_live(
            &snapshot(60.0, &[1056], &[0, 1]),
            Some("next Sunfire"),
            2_000,
        );
        recorder.observe_live(
            &snapshot(62.0, &[1056], &[0, 1]),
            Some("next Sunfire"),
            4_000,
        );
        recorder.observe_live(
            &snapshot(64.0, &[1056, 1029], &[0, 1, 2]),
            Some("next Sunfire"),
            6_000,
        );
        recorder.observe_live(
            &snapshot(66.0, &[1056, 1029], &[0, 1, 2]),
            Some("next Thornmail"),
            8_000,
        );
        recorder.observe_live(
            &snapshot(80.0, &[1056, 1029], &[0, 1, 2]),
            Some("next Thornmail"),
            22_000,
        );
        recorder.observe_live(
            &snapshot(86.5, &[1056, 1029], &[0, 1, 2]),
            Some("next Thornmail"),
            28_500,
        );
        let games = lines(&dir);
        assert_eq!(games.len(), 1);
        let game = &games[0];
        assert_eq!(game[0]["kind"], "game");
        assert_eq!(game[0]["champion"], "Malphite");
        assert!(game[0]["champselect"]["chatDetails"].is_null());
        // First observation, the purchase, the new recommendation, then 20 s later.
        let times: Vec<f64> = game[1..]
            .iter()
            .map(|l| l["game_time"].as_f64().unwrap())
            .collect();
        assert_eq!(times, [60.0, 64.0, 66.0, 86.5]);
        // Static data only once; each event only once.
        assert!(game[1]["data"]["allPlayers"][0]["runes"].is_object());
        assert!(game[2]["data"]["allPlayers"][0].get("runes").is_none());
        assert!(game[2]["data"]["activePlayer"].get("fullRunes").is_none());
        assert_eq!(game[2]["data"]["events"]["Events"], json!([{"EventID": 2}]));
        assert_eq!(game[3]["data"]["events"]["Events"], json!([]));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_restarted_clock_or_a_long_gap_starts_a_new_file_and_old_games_are_pruned() {
        let dir = scratch("split");
        let mut recorder = GameRecorder::new(dir.clone());
        recorder.observe_live(&snapshot(900.0, &[1056], &[]), None, 1_000_000_000_000);
        recorder.observe_live(&snapshot(30.0, &[1056], &[]), None, 1_000_000_600_000);
        recorder.observe_live(&snapshot(40.0, &[1056], &[]), None, 1_000_001_000_000);
        assert_eq!(lines(&dir).len(), 3);
        for n in 0..5 {
            fs::write(dir.join(format!("1999010{n}-000000Z-Old.jsonl")), "{}\n").unwrap();
        }
        prune(&dir, 3);
        let kept: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(kept.len(), 3);
        assert!(kept.iter().all(|name| !name.starts_with("1999")));
        let _ = fs::remove_dir_all(&dir);
    }
}
