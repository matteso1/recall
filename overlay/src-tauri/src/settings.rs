//! Persisted settings: panel position, auto-import switches, aggregate source knobs; plus the data dir.
//!
//! Only the position is written by the app itself (on drag). Whether the panel is collapsed is a
//! per-session choice, and the saved position is validated against the monitors at startup (see
//! `featherstorm_core::placement`), so a stale file cannot hide the panel. The other keys are for
//! hand edits in `%LOCALAPPDATA%\Featherstorm\settings.json`; unknown keys are ignored.
use featherstorm_core::aggregate;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn yes() -> bool {
    true
}
fn default_region() -> String {
    aggregate::DEFAULT_REGION.to_string()
}
fn default_tier() -> String {
    aggregate::DEFAULT_TIER.to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    /// Outer position of the panel in physical pixels (`None` until it has been placed once).
    pub x: Option<i32>,
    pub y: Option<i32>,
    /// Create the rune page for the champion as soon as it is picked / hovered in champ select.
    #[serde(default = "yes")]
    pub auto_runes: bool,
    /// Set the summoner spells likewise (Flash stays on the key it is on).
    #[serde(default = "yes")]
    pub auto_spells: bool,
    /// Push the item set once the champion is locked (again if enemy locks change the path).
    #[serde(default = "yes")]
    pub auto_itemset: bool,
    /// op.gg region for the aggregate: global | na | euw | kr | ...
    #[serde(default = "default_region")]
    pub region: String,
    /// op.gg tier filter: emerald_plus | diamond_plus | master_plus | all | ...
    #[serde(default = "default_tier")]
    pub tier: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { x: None, y: None, auto_runes: true, auto_spells: true, auto_itemset: true, region: default_region(), tier: default_tier() }
    }
}

/// %LOCALAPPDATA%\Featherstorm (logs, Data Dragon + aggregate caches, settings)
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
