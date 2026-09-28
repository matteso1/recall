//! Master+ creep-score benchmarks for the post-game recap: median CS at 10 minutes and median CS per minute
//! per champion and role, from ~107k ranked Master+ player-games (Kaggle ranked-timeline, 16.13-16.18; lane
//! minions plus jungle monsters, as the Live Client's creepScore counts them). Built by
//! `tools/priors/export_cs.py`; champion-roles under 100 games use the role's value.
use crate::aggregate::Position;
use crate::ddragon::normalize;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

const JSON: &str = include_str!("../../../data/pack/cs_bench.json");

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct CsBenchmark {
    /// "Master+ Xayah ADC", or "Master+ ADC" for the role's value.
    #[serde(default)]
    pub label: String,
    pub games: u32,
    pub cs_at_10: f64,
    pub per_minute: f64,
}

#[derive(Deserialize)]
struct Doc {
    champions: HashMap<String, CsBenchmark>,
    roles: HashMap<String, CsBenchmark>,
}

fn doc() -> &'static Doc {
    static DOC: OnceLock<Doc> = OnceLock::new();
    DOC.get_or_init(|| serde_json::from_str(JSON).expect("data/pack/cs_bench.json"))
}

/// The benchmark for a champion (display name) in a role (a position label such as "ADC").
pub fn lookup(champion: &str, role: &str) -> Option<CsBenchmark> {
    let role = Position::parse(role)?;
    let d = doc();
    if let Some(b) = d
        .champions
        .get(&format!("{}|{}", normalize(champion), role.label()))
    {
        return Some(CsBenchmark {
            label: format!("Master+ {champion} {}", role.label()),
            ..b.clone()
        });
    }
    d.roles.get(role.label()).map(|b| CsBenchmark {
        label: format!("Master+ {}", role.label()),
        ..b.clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn champion_then_role_benchmarks() {
        let xayah = lookup("Xayah", "ADC").unwrap();
        assert_eq!(xayah.label, "Master+ Xayah ADC");
        assert!((70.0..90.0).contains(&xayah.cs_at_10), "{xayah:?}");
        assert!((7.0..10.0).contains(&xayah.per_minute), "{xayah:?}");
        let support = lookup("Lux", "Support").unwrap();
        assert!(support.cs_at_10 < 30.0, "supports do not farm lanes");
        assert_eq!(lookup("Nobody", "Top").unwrap().label, "Master+ Top");
        assert!(lookup("Xayah", "").is_none());
    }
}
