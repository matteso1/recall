//! What the panel renders. Serialized to the webview on every change.
use crate::engine::Plan;
use crate::journal::Recap;
use serde::Serialize;

/// Receipt age of the source currently shown. An unknown timestamp is never fresh.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SourceStatus {
    pub observed_at_ms: Option<u64>,
    pub age_ms: Option<u64>,
    pub stale: bool,
    pub identity_known: bool,
}

impl Default for SourceStatus {
    fn default() -> Self {
        Self {
            observed_at_ms: None,
            age_ms: None,
            stale: true,
            identity_known: false,
        }
    }
}

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
pub struct SwiftplaySlotView {
    pub index: usize,
    pub champion: String,
    pub position: String,
    pub plan: Option<Plan>,
    pub imports: Imports,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct SwiftplayView {
    pub slots: Vec<SwiftplaySlotView>,
    pub observed_at_ms: Option<u64>,
    pub ready: bool,
    pub preparing: bool,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct PanelState {
    /// Explicit, read-only staged preview. Never populated by the live poller.
    pub demo: bool,
    /// noclient | idle | swiftplay | champselect | loading | ingame
    pub phase: String,
    pub gameflow: String,
    pub summoner: Option<String>,
    pub champion: Option<String>,
    /// Role-specific aggregate data produced a nonempty compatible build path.
    pub supported: bool,
    pub lobby: Option<LobbyView>,
    pub swiftplay: Option<SwiftplayView>,
    pub plan: Option<Plan>,
    pub live: Option<LiveView>,
    #[serde(default)]
    pub live_source: Option<SourceStatus>,
    #[serde(default)]
    pub aggregate_source: Option<SourceStatus>,
    /// Most recent finished match; retained while idle or playing the next match.
    #[serde(default)]
    pub recap: Option<Recap>,
    /// Local journal persistence problems are visible without pausing live observations.
    #[serde(default)]
    pub journal_error: Option<String>,
    pub flash: Option<Flash>,
    pub imports: Imports,
    pub message: Option<String>,
    pub collapsed: bool,
    pub ddragon: Option<String>,
    pub version: String,
}

#[cfg(test)]
mod tests {
    use super::SourceStatus;

    #[test]
    fn source_without_an_observation_is_not_fresh() {
        let source = SourceStatus::default();
        assert!(source.stale);
        assert_eq!(source.observed_at_ms, None);
        assert_eq!(source.age_ms, None);
        assert!(!source.identity_known);
    }
}
