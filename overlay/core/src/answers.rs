//! How strongly each enemy champion calls for an answer item, measured rather than tagged.
//! `data/pack/answers.json` holds, per answer and champion, the odds ratio of Master+ players owning one of the
//! answer's items when that champion is on the enemy team: one logistic regression per answer over ~107k ranked
//! player-games, on the player's role and the five enemies, so co-occurring threats are separated (Kaggle
//! ranked-timeline, 16.13-16.18; `tools/priors/export_answers.py`). The weight maps the odds ratio to 0-1.
//!
//! The hand-written trait tags were wrong in both directions. Garen was a "healer" (anti-heal odds 0.8; the
//! 2026-09-28 Xayah game was offered Executioner's Calling "for Garen"); Lissandra, Fiddlesticks and Twisted
//! Fate had no cleanse tag (5.5x, 4.3x, 3.8x); AP burst mages counted as dive like Zed (about 1.0x defensive
//! items, Zed 1.75x). The tags remain the fallback for champions with too few games.
use crate::ddragon::normalize;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

const JSON: &str = include_str!("../../../data/pack/answers.json");

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Answer {
    /// Grievous Wounds items.
    AntiHeal,
    /// Quicksilver Sash, Mercurial Scimitar.
    Cleanse,
    /// Guardian Angel, Zhonya's, Seeker's Armguard, Immortal Shieldbow, Edge of Night.
    AntiBurst,
}

#[derive(Deserialize)]
struct Entry {
    odds: f64,
    weight: f64,
}

#[derive(Deserialize)]
struct Doc {
    antiheal: HashMap<String, Entry>,
    cleanse: HashMap<String, Entry>,
    anti_burst: HashMap<String, Entry>,
}

fn doc() -> &'static Doc {
    static DOC: OnceLock<Doc> = OnceLock::new();
    DOC.get_or_init(|| serde_json::from_str(JSON).expect("data/pack/answers.json"))
}

fn entry(answer: Answer, champion: &str) -> Option<&'static Entry> {
    let d = doc();
    let table = match answer {
        Answer::AntiHeal => &d.antiheal,
        Answer::Cleanse => &d.cleanse,
        Answer::AntiBurst => &d.anti_burst,
    };
    table.get(&normalize(champion))
}

/// 0-1 weight of `answer` against an enemy champion (display name), or None with too few games.
pub fn weight(answer: Answer, champion: &str) -> Option<f64> {
    entry(answer, champion).map(|e| e.weight)
}

/// How many times as often Master+ players own the answer with this champion on the enemy team.
pub fn odds(answer: Answer, champion: &str) -> Option<f64> {
    entry(answer, champion).map(|e| e.odds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_reactions_not_the_tags() {
        assert_eq!(weight(Answer::AntiHeal, "Soraka"), Some(1.0));
        assert!(weight(Answer::AntiHeal, "Garen").unwrap() < 0.05);
        assert!(weight(Answer::AntiHeal, "Zac").unwrap() > 0.5);
        assert!(weight(Answer::Cleanse, "Malzahar").unwrap() > 0.9);
        assert!(weight(Answer::Cleanse, "Lissandra").unwrap() > 0.5);
        assert!(weight(Answer::Cleanse, "Zed").unwrap() < 0.05);
        assert!(weight(Answer::AntiBurst, "Zed").unwrap() > 0.8);
        assert!(weight(Answer::AntiBurst, "Syndra").unwrap() < 0.2);
        assert!(odds(Answer::Cleanse, "Malzahar").unwrap() > 5.0);
        assert!(weight(Answer::AntiHeal, "Nobody").is_none());
    }
}
