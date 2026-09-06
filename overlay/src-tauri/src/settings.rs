//! Tiny persisted settings: the panel position, plus the app data dir.
//!
//! Only the position is remembered. Whether the panel is collapsed is a per-session choice: a
//! panel that starts collapsed looks broken, and the saved position is validated against the
//! monitors at startup (see `featherstorm_core::placement`), so a stale file cannot hide it.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    /// Outer position of the panel in physical pixels (`None` until it has been placed once).
    pub x: Option<i32>,
    pub y: Option<i32>,
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

/// Unknown or malformed content (older files, hand edits) yields the defaults; unknown keys are ignored.
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
