//! LCU (League Client) API over its lockfile-authenticated local HTTPS endpoint.
//!
//! The same mechanism op.gg and Blitz use. Everything is local: `https://127.0.0.1:<port>`
//! with HTTP Basic auth `riot:<password>`, both read from the client's lockfile.
use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lockfile {
    pub process: String,
    pub pid: u32,
    pub port: u16,
    pub password: String,
    pub protocol: String,
}

impl Lockfile {
    /// Format: `LeagueClient:<pid>:<port>:<password>:https`
    pub fn parse(text: &str) -> Result<Lockfile> {
        let parts: Vec<&str> = text.trim().split(':').collect();
        if parts.len() != 5 {
            bail!("unexpected lockfile format ({} fields)", parts.len());
        }
        Ok(Lockfile {
            process: parts[0].to_string(),
            pid: parts[1].parse().context("lockfile pid")?,
            port: parts[2].parse().context("lockfile port")?,
            password: parts[3].to_string(),
            protocol: parts[4].to_string(),
        })
    }

    pub fn base_url(&self) -> String {
        format!("{}://127.0.0.1:{}", self.protocol, self.port)
    }

    /// Safe to log.
    pub fn masked(&self) -> String {
        let head: String = self.password.chars().take(2).collect();
        format!("{}:{}:{}:{}***:{}", self.process, self.pid, self.port, head, self.protocol)
    }
}

/// League install dirs listed in `C:\ProgramData\Riot Games\RiotClientInstalls.json`.
pub fn league_dirs_from_installs(json_text: &str) -> Vec<PathBuf> {
    let Ok(v) = serde_json::from_str::<Value>(json_text) else {
        return Vec::new();
    };
    v.get("associated_client")
        .and_then(Value::as_object)
        .map(|m| {
            m.keys()
                .filter(|k| k.to_lowercase().contains("league of legends"))
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

pub fn find_league_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("FEATHERSTORM_LEAGUE_DIR") {
        return Some(PathBuf::from(p));
    }
    let program_data = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".to_string());
    let installs = Path::new(&program_data).join("Riot Games").join("RiotClientInstalls.json");
    if let Ok(text) = std::fs::read_to_string(&installs) {
        for dir in league_dirs_from_installs(&text) {
            if dir.is_dir() {
                return Some(dir);
            }
        }
    }
    let fallback = PathBuf::from(r"C:\Riot Games\League of Legends");
    if fallback.is_dir() {
        Some(fallback)
    } else {
        None
    }
}

pub fn lockfile_path() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("FEATHERSTORM_LOCKFILE") {
        return Ok(PathBuf::from(p));
    }
    Ok(find_league_dir()
        .ok_or_else(|| anyhow!("League install not found (set FEATHERSTORM_LEAGUE_DIR)"))?
        .join("lockfile"))
}

/// Fails when the client is not running (no lockfile).
pub fn read_lockfile() -> Result<Lockfile> {
    let path = lockfile_path()?;
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("no lockfile at {} - is the League client running?", path.display()))?;
    Lockfile::parse(&text)
}

#[derive(Clone)]
pub struct Lcu {
    pub lockfile: Lockfile,
    base: String,
    http: reqwest::Client,
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

impl Lcu {
    pub fn connect() -> Result<Lcu> {
        Self::from_lockfile(read_lockfile()?)
    }

    pub fn from_lockfile(lockfile: Lockfile) -> Result<Lcu> {
        let http = reqwest::Client::builder()
            .danger_accept_invalid_certs(true) // the client uses a self-signed certificate
            .timeout(Duration::from_secs(5))
            .build()?;
        Ok(Lcu { base: lockfile.base_url(), lockfile, http })
    }

    pub fn port(&self) -> u16 {
        self.lockfile.port
    }

    async fn call(&self, method: reqwest::Method, path: &str, body: Option<&Value>) -> Result<(u16, Value)> {
        let mut req = self
            .http
            .request(method.clone(), format!("{}{}", self.base, path))
            .basic_auth("riot", Some(&self.lockfile.password))
            .header("Accept", "application/json");
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req.send().await.with_context(|| format!("{method} {path}: client unreachable"))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        let json = if text.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        };
        Ok((status, json))
    }

    fn check(method: &str, path: &str, status: u16, body: Value) -> Result<Value> {
        if (200..300).contains(&status) {
            Ok(body)
        } else {
            bail!("{method} {path} -> HTTP {status}: {}", truncate(&body.to_string(), 300))
        }
    }

    pub async fn get(&self, path: &str) -> Result<Value> {
        let (s, b) = self.call(reqwest::Method::GET, path, None).await?;
        Self::check("GET", path, s, b)
    }

    /// `None` on 404 (e.g. no champ select session right now).
    pub async fn get_opt(&self, path: &str) -> Result<Option<Value>> {
        let (s, b) = self.call(reqwest::Method::GET, path, None).await?;
        if s == 404 {
            return Ok(None);
        }
        Self::check("GET", path, s, b).map(Some)
    }

    pub async fn put(&self, path: &str, body: &Value) -> Result<Value> {
        let (s, b) = self.call(reqwest::Method::PUT, path, Some(body)).await?;
        Self::check("PUT", path, s, b)
    }

    pub async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let (s, b) = self.call(reqwest::Method::POST, path, Some(body)).await?;
        Self::check("POST", path, s, b)
    }

    pub async fn patch(&self, path: &str, body: &Value) -> Result<Value> {
        let (s, b) = self.call(reqwest::Method::PATCH, path, Some(body)).await?;
        Self::check("PATCH", path, s, b)
    }

    pub async fn delete(&self, path: &str) -> Result<Value> {
        let (s, b) = self.call(reqwest::Method::DELETE, path, None).await?;
        Self::check("DELETE", path, s, b)
    }

    // ---------------------------------------------------------------- gameflow

    /// None | Lobby | Matchmaking | ReadyCheck | ChampSelect | GameStart | InProgress |
    /// WaitingForStats | PreEndOfGame | EndOfGame | Reconnect | ...
    pub async fn gameflow_phase(&self) -> Result<String> {
        Ok(self.get("/lol-gameflow/v1/gameflow-phase").await?.as_str().unwrap_or("None").to_string())
    }

    // ---------------------------------------------------------------- summoner

    pub async fn current_summoner(&self) -> Result<Value> {
        self.get("/lol-summoner/v1/current-summoner").await
    }

    // ------------------------------------------------------------- champ select

    pub async fn champ_select_session(&self) -> Result<Option<Value>> {
        self.get_opt("/lol-champ-select/v1/session").await
    }

    pub async fn set_summoner_spells(&self, spell1: u64, spell2: u64) -> Result<()> {
        self.patch(
            "/lol-champ-select/v1/session/my-selection",
            &serde_json::json!({ "spell1Id": spell1, "spell2Id": spell2 }),
        )
        .await
        .map(|_| ())
    }

    // ---------------------------------------------------------------- item sets

    /// `{accountId, timestamp, itemSets: [...]}` - every custom set on the account.
    pub async fn item_sets(&self, summoner_id: u64) -> Result<Value> {
        self.get(&format!("/lol-item-sets/v1/item-sets/{summoner_id}/sets")).await
    }

    /// Replaces ALL item sets: always GET, modify (`itemset::upsert`), PUT.
    pub async fn put_item_sets(&self, summoner_id: u64, payload: &Value) -> Result<()> {
        self.put(&format!("/lol-item-sets/v1/item-sets/{summoner_id}/sets"), payload).await.map(|_| ())
    }

    // -------------------------------------------------------------------- runes

    pub async fn perk_inventory(&self) -> Result<Value> {
        self.get("/lol-perks/v1/inventory").await
    }

    pub async fn perk_pages(&self) -> Result<Value> {
        self.get("/lol-perks/v1/pages").await
    }

    /// `page = {name, primaryStyleId, subStyleId, selectedPerkIds: [9 ids], current: true}`
    pub async fn create_perk_page(&self, page: &Value) -> Result<Value> {
        self.post("/lol-perks/v1/pages", page).await
    }

    pub async fn delete_perk_page(&self, id: u64) -> Result<()> {
        self.delete(&format!("/lol-perks/v1/pages/{id}")).await.map(|_| ())
    }

    pub async fn set_current_perk_page(&self, id: u64) -> Result<()> {
        self.put("/lol-perks/v1/currentpage", &Value::from(id)).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lockfile() {
        let lf = Lockfile::parse("LeagueClient:17896:56139:TmSecret_-x:https\n").unwrap();
        assert_eq!((lf.pid, lf.port, lf.protocol.as_str()), (17896, 56139, "https"));
        assert_eq!(lf.base_url(), "https://127.0.0.1:56139");
        assert!(!lf.masked().contains("Secret"));
        assert!(Lockfile::parse("garbage").is_err());
    }

    #[test]
    fn finds_league_dirs_in_installs_json() {
        let text = r#"{"associated_client": {"C:/Riot Games/League of Legends/": "x", "D:/VALORANT/live/": "y"}}"#;
        assert_eq!(league_dirs_from_installs(text), vec![PathBuf::from("C:/Riot Games/League of Legends/")]);
        assert!(league_dirs_from_installs("{nope").is_empty());
    }
}
