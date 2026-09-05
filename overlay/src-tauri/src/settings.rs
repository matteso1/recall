//! Tiny persisted settings: panel position and collapsed state, plus the app data dir.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    pub x: Option<i32>,
    pub y: Option<i32>,
    #[serde(default)]
    pub collapsed: bool,
}

/// %LOCALAPPDATA%\Featherstorm (logs, Data Dragon cache, settings)
pub fn data_dir() -> PathBuf {
    let base = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir);
    let dir = base.join("Featherstorm");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn path() -> PathBuf {
    data_dir().join("settings.json")
}

pub fn load() -> Settings {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(s: &Settings) -> anyhow::Result<()> {
    std::fs::write(path(), serde_json::to_string_pretty(s)?)?;
    Ok(())
}
