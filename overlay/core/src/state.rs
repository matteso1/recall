//! What the panel renders. Serialized to the webview on every change.
use crate::engine::Plan;
use serde::Serialize;

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct LobbyView {
    pub allies: Vec<String>,
    pub enemies: Vec<String>,
    pub my_position: String,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct LiveView {
    pub game_time: f64,
    pub gold: f64,
    pub level: u32,
    pub kda: String,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Flash {
    pub skill: char,
    pub until_ms: u64,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Imports {
    pub itemset: String,
    pub runes: String,
    pub spells: String,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct PanelState {
    /// noclient | idle | champselect | loading | ingame
    pub phase: String,
    pub gameflow: String,
    pub summoner: Option<String>,
    pub champion: Option<String>,
    /// The pack knows this champion (M1: Xayah only)
    pub supported: bool,
    pub lobby: Option<LobbyView>,
    pub plan: Option<Plan>,
    pub live: Option<LiveView>,
    pub flash: Option<Flash>,
    pub imports: Imports,
    pub message: Option<String>,
    pub collapsed: bool,
    pub ddragon: Option<String>,
    pub version: String,
}
