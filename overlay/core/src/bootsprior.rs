//! First tier-two boots observed in Master+ timelines, conditioned on champion, role and
//! enemy magic-damage count. Exported by tools/priors/export_boots.py; absent coverage
//! leaves the caller's provider fallback intact. This is purchase frequency, not win uplift.
use crate::aggregate::Position;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

const JSON: &str = include_str!("../../../data/pack/boots.json");

#[derive(Deserialize)]
struct Champion {
    by_magic_enemies: HashMap<String, HashMap<String, f64>>,
}

#[derive(Deserialize)]
struct Doc {
    champions: HashMap<String, Champion>,
}

static DOC: OnceLock<Doc> = OnceLock::new();

#[cfg(feature = "evaluation")]
pub fn load_for_evaluation(json: &str) -> anyhow::Result<()> {
    let data = serde_json::from_str(json)?;
    DOC.set(data)
        .map_err(|_| anyhow::anyhow!("boots data already initialized"))
}

pub fn distribution(
    champion_key: u32,
    role: Position,
    magic_enemies: usize,
) -> Option<Vec<(u32, f64)>> {
    let doc = DOC.get_or_init(|| serde_json::from_str(JSON).expect("data/pack/boots.json"));
    let table = doc
        .champions
        .get(&format!("{champion_key}|{}", role.label()))?
        .by_magic_enemies
        .get(&magic_enemies.min(4).to_string())?;
    let mut ranked: Vec<_> = table
        .iter()
        .filter_map(|(id, &p)| {
            (p.is_finite() && p > 0.0)
                .then(|| id.parse::<u32>().ok().map(|id| (id, p)))
                .flatten()
        })
        .collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    (!ranked.is_empty()).then_some(ranked)
}
