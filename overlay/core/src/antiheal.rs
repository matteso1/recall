//! How strongly each enemy champion calls for anti-heal, measured rather than assumed. `data/pack/antiheal.json`
//! holds, per champion, the odds ratio of Master+ players owning a Grievous Wounds item when that champion is on
//! the enemy team, from a logistic regression over ~107k ranked player-games that separates co-occurring
//! healers (Kaggle ranked-timeline, 16.13-16.18; `tools/priors/export_antiheal.py`). The weight maps the odds
//! ratio to 0-1: Soraka, Warwick and Aatrox 1, Zac 0.78, Sylas 0.28, Garen and Cho'Gath 0. The binary healing
//! trait used to count Garen's regeneration like Soraka's heals (the Xayah game of 2026-09-28 was offered
//! Executioner's Calling "for Garen", which 3% of Master+ ADCs buy against him).
use crate::ddragon::normalize;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

const JSON: &str = include_str!("../../../data/pack/antiheal.json");

#[derive(Deserialize)]
struct Entry {
    weight: f64,
}

#[derive(Deserialize)]
struct Doc {
    champions: HashMap<String, Entry>,
}

fn doc() -> &'static Doc {
    static DOC: OnceLock<Doc> = OnceLock::new();
    DOC.get_or_init(|| serde_json::from_str(JSON).expect("data/pack/antiheal.json"))
}

/// 0-1 anti-heal weight for an enemy champion (display name), or None when the corpus has too few games.
pub fn weight(champion: &str) -> Option<f64> {
    doc().champions.get(&normalize(champion)).map(|e| e.weight)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_reactions_not_the_trait() {
        assert_eq!(weight("Soraka"), Some(1.0));
        assert!(
            weight("Garen").unwrap() < 0.05,
            "regeneration draws no anti-heal"
        );
        assert!(weight("Cho'Gath").unwrap() < 0.05);
        assert!(weight("Zac").unwrap() > 0.5);
        assert!(weight("Dr. Mundo").unwrap() > 0.5);
        assert!(weight("Nobody").is_none());
    }
}
