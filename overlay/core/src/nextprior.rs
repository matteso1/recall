//! What Master+ players buy next (engine v3 backbone). `data/pack/next_items.json` holds weighted counts of
//! the next completed legendary item per champion and role, conditioned on the legendary items already
//! owned, from ~107k ranked Master+ player-games on patches 16.13-16.18 (Kaggle, MIT; the shop of 16.17-16.18
//! is identical to 16.19). Built by `tools/priors/export_next.py`. Held out by player, this owned-set model
//! names the next legendary 50.6% of the time (top-3 75.9%) against 47.4% (71.2%) for a static build order.
//!
//! Smoothing at lookup: owned set -> build step (nth item) -> the role's owned set -> the role's build step.
//! The enemy composition then scales each item by how Master+ players of the role react to it
//! (`lifts`: e.g. Mortal Reminder x2.1 against a healer, Lord Dominik's x1.4 against two tanks).
use crate::aggregate::Position;
use crate::ddragon::Catalog;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

const JSON: &str = include_str!("../../../data/pack/next_items.json");
/// Pseudo-counts: how many games of the backoff level one level of evidence is worth.
const A_SET: f64 = 30.0;
const A_NTH: f64 = 100.0;
const A_ROLE: f64 = 3000.0;
/// Items the corpus records under another id: transformed tear items and Stormrazor's pre-16.17 id.
const NORMALIZE: [(u32, u32); 5] = [
    (3097, 3095),
    (3042, 3004),
    (3040, 3003),
    (3121, 3119),
    (2530, 2526),
];
/// A legendary item as the corpus counts one: finished, at least this price, not boots or consumables.
const LEGENDARY_GOLD: u32 = 2200;

type Counts = HashMap<String, HashMap<String, f64>>;

#[derive(Deserialize)]
struct Level {
    #[serde(default)]
    sets: Counts,
    #[serde(default)]
    nth: Counts,
    /// feature -> "1"/"0" -> item -> P(item | role, feature value) / P(item | role).
    #[serde(default)]
    lifts: HashMap<String, Counts>,
}

/// The enemy composition as the corpus export defines it (tools/priors/export_next.py): any enemy with
/// meaningful healing, three or more magic-damage enemies, two or more tanks (champion traits file).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Comp {
    pub heal: bool,
    pub magic: bool,
    pub tank: bool,
}

impl Comp {
    pub fn of(traits: &crate::pack::Traits, enemies: &[String]) -> Self {
        let t: Vec<_> = enemies.iter().filter_map(|name| traits.get(name)).collect();
        Self {
            heal: t.iter().any(|t| t.healing),
            magic: t.iter().filter(|t| t.damage == "ap").count() >= 3,
            tank: t.iter().filter(|t| t.tank).count() >= 2,
        }
    }
}

#[derive(Deserialize)]
struct Champion {
    #[serde(default)]
    games: u32,
    #[serde(flatten)]
    level: Level,
}

#[derive(Deserialize)]
struct Doc {
    champions: HashMap<String, Champion>,
    roles: HashMap<String, Level>,
}

static DOC: OnceLock<Doc> = OnceLock::new();

/// Install training-only counts before any planner call in an offline evaluator.
#[cfg(feature = "evaluation")]
pub fn load_for_evaluation(json: &str) -> anyhow::Result<()> {
    let data = serde_json::from_str(json)?;
    DOC.set(data)
        .map_err(|_| anyhow::anyhow!("next-item data already initialized"))
}

fn doc() -> &'static Doc {
    DOC.get_or_init(|| serde_json::from_str(JSON).expect("data/pack/next_items.json"))
}

pub fn normalize(id: u32) -> u32 {
    NORMALIZE
        .iter()
        .find(|(from, _)| *from == id)
        .map_or(id, |(_, to)| *to)
}

/// Whether the corpus counts `id` as a legendary completion.
pub fn is_legendary(cat: &Catalog, id: u32) -> bool {
    cat.item(normalize(id)).is_some_and(|i| {
        i.is_finished(cat)
            && i.total >= LEGENDARY_GOLD
            && !i.effects.boots
            && !i.tags.iter().any(|t| t == "Consumable")
    })
}

/// Games behind the champion's own table, if the corpus covers this champion and role.
pub fn coverage(champion_key: u32, role: Position) -> Option<u32> {
    doc()
        .champions
        .get(&format!("{champion_key}|{}", role.label()))
        .map(|c| c.games)
}

fn smooth(
    counts: Option<&HashMap<String, f64>>,
    prior: &HashMap<u32, f64>,
    a: f64,
) -> HashMap<u32, f64> {
    let counts: HashMap<u32, f64> = counts
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| k.parse().ok().map(|id| (id, *v)))
        .collect();
    let n: f64 = counts.values().sum();
    let mut keys: Vec<u32> = counts.keys().chain(prior.keys()).copied().collect();
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter()
        .map(|id| {
            let c = counts.get(&id).copied().unwrap_or(0.0);
            let p = prior.get(&id).copied().unwrap_or(0.0);
            (id, (c + a * p) / (n + a).max(f64::MIN_POSITIVE))
        })
        .collect()
}

/// P(next legendary | champion, role, owned legendaries), highest first, or None when the corpus does
/// not cover this champion and role (the caller keeps the op.gg build then). Owned items are excluded.
pub fn distribution(
    champion_key: u32,
    role: Position,
    owned: &[u32],
    comp: Option<Comp>,
) -> Option<Vec<(u32, f64)>> {
    let d = doc();
    let champ = d
        .champions
        .get(&format!("{champion_key}|{}", role.label()))?;
    let mut owned: Vec<u32> = owned.iter().map(|&id| normalize(id)).collect();
    owned.sort_unstable();
    owned.dedup();
    let key = owned
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let nth = (owned.len() + 1).to_string();
    let role_level = d.roles.get(role.label());
    let p_role = smooth(
        role_level.and_then(|r| r.nth.get(&nth)),
        &HashMap::new(),
        0.0,
    );
    let p_role_set = smooth(role_level.and_then(|r| r.sets.get(&key)), &p_role, A_ROLE);
    let p_nth = smooth(champ.level.nth.get(&nth), &p_role_set, A_NTH);
    let mut p = smooth(champ.level.sets.get(&key), &p_nth, A_SET);
    if let (Some(comp), Some(level)) = (comp, role_level) {
        for (feature, on) in [
            ("heal", comp.heal),
            ("magic", comp.magic),
            ("tank", comp.tank),
        ] {
            let Some(lift) = level
                .lifts
                .get(feature)
                .and_then(|l| l.get(if on { "1" } else { "0" }))
            else {
                continue;
            };
            for (id, v) in p.iter_mut() {
                *v *= lift.get(&id.to_string()).copied().unwrap_or(1.0);
            }
        }
        let total: f64 = p.values().sum();
        if total > 0.0 {
            p.values_mut().for_each(|v| *v /= total);
        }
    }
    let mut ranked: Vec<(u32, f64)> = p
        .into_iter()
        .filter(|(id, v)| *v > 0.0 && !owned.contains(id))
        .collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    Some(ranked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xayah_after_yun_tal_goes_infinity_edge_or_navori() {
        let d = distribution(498, Position::Adc, &[3032], None).expect("Xayah ADC is covered");
        let top: Vec<u32> = d.iter().take(2).map(|(id, _)| *id).collect();
        assert!(top.contains(&3031) && top.contains(&6675), "{d:?}");
        let total: f64 = d.iter().map(|(_, p)| p).sum();
        assert!((0.9..=1.0001).contains(&total), "{total}");
        assert!(
            d.iter().all(|(id, _)| *id != 3032),
            "owned items are excluded"
        );
    }

    #[test]
    fn sivir_never_builds_guardian_angel_second() {
        let d = distribution(15, Position::Adc, &[3508], None).expect("Sivir ADC is covered");
        let ga = d.iter().position(|(id, _)| *id == 3026);
        assert!(ga.is_none_or(|rank| rank >= 5), "{d:?}");
        assert!(
            matches!(d.first(), Some((6675, _)) | Some((3031, _))),
            "{d:?}"
        );
    }

    #[test]
    fn a_healer_raises_mortal_reminder_and_two_tanks_raise_lord_dominiks() {
        let share = |comp: Comp, id: u32| {
            distribution(498, Position::Adc, &[3032, 6675, 3031], Some(comp))
                .unwrap()
                .iter()
                .find(|(i, _)| *i == id)
                .map_or(0.0, |(_, p)| *p)
        };
        let none = Comp::default();
        assert!(share(Comp { heal: true, ..none }, 3033) > 1.5 * share(none, 3033));
        assert!(share(Comp { tank: true, ..none }, 3036) > 1.2 * share(none, 3036));
    }

    #[test]
    fn uncovered_champion_roles_fall_back_to_the_caller() {
        assert!(distribution(99_999, Position::Top, &[], None).is_none());
        assert_eq!(normalize(3097), 3095);
    }
}
