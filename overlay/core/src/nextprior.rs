//! What Master+ players buy next (engine v3 backbone). `data/pack/next_items.json` holds weighted counts of
//! the next completed legendary item per champion and role, conditioned on the legendary items already
//! owned, from ~107k ranked Master+ player-games on patches 16.13-16.18 (Kaggle, MIT; the shop of 16.17-16.18
//! is identical to 16.19). Built by `tools/priors/export_next.py`.
//!
//! Smoothing at lookup, for the owned legendary set S (k = |S| + 1 is the build step):
//! 1. The champion's own counts for exactly S (`A_SET` pseudo-games of the prior below).
//! 2. The prior mixes two views of the champion (`SUBSET_SHARE`), from two owned items on:
//!    - what the champion buys at step k, whatever is owned (`nth`), and
//!    - the same question asked of every set one item smaller, without the items now owned. This is the
//!      view that knows a skipped core item is still due: Yasuo with Stormrazor, Shieldbow and Death's
//!      Dance but no Infinity Edge is a rare set, the step-4 table says Guardian Angel (players at step
//!      4 usually own Infinity Edge already), and the smaller sets say Infinity Edge.
//! 3. Below the champion, the role's tables, **restricted to items this champion's Master+ players
//!    complete at some step** (its support). Unrestricted, the mid-lane table is mostly mages and put
//!    Zhonya's Hourglass on Yasuo's path and Guardian Angel on Lux's (the Yasuo game of 2026-10-02,
//!    0 of 395 Master+ Yasuo players): 11% of the probability sat on items the champion never builds.
//!
//! 4. In the build-step table, an item nobody built together with anything now owned counts
//!    `NOT_BUILT_WITH` as much: one champion can have two families of builds (on-hit and AP
//!    Katarina), and the step table alone is mostly the bigger family.
//!
//! Held out by player (24,013 decisions the tuning did not see; `tools/priors/eval_next.py`): next
//! legendary top-1 53.4% -> 55.0%, top-3 79.1% -> 81.5%, log loss 1.71 -> 1.52; on owned sets the
//! champion's table lacks, top-1 21% -> 31%; fourth items 35% -> 42%, fifth items 17% -> 27%.
//! The enemy composition then scales each item by how Master+ players of the role react to it
//! (`lifts`: e.g. Mortal Reminder x2.1 against a healer, Lord Dominik's x1.4 against two tanks).
use crate::aggregate::Position;
use crate::ddragon::Catalog;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

const JSON: &str = include_str!("../../../data/pack/next_items.json");
/// Pseudo-counts: how many games of the backoff level one level of evidence is worth.
const A_SET: f64 = 30.0;
const A_NTH: f64 = 50.0;
const A_ROLE: f64 = 3000.0;
/// Share of the smaller-set view in an owned set's prior (the build-step table has the rest).
/// Chosen on a tuning split of held-out players, with `A_NTH` (0.5 and 50 against 0.3-1.0 and 30-100).
const SUBSET_SHARE: f64 = 0.5;
/// Weight, in the build-step table, of an item never seen in a build with any owned item.
const NOT_BUILT_WITH: f64 = 0.3;
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

/// What a champion's tables say about items, apart from the counts.
#[derive(Default)]
struct Items {
    /// The legendary items its Master+ players complete at any step of the build.
    support: HashSet<u32>,
    /// For each item, the items seen with it in one build (an owned set and what was bought next).
    built_with: HashMap<u32, HashSet<u32>>,
}

impl Items {
    fn built_with_any(&self, id: u32, owned: &[u32]) -> bool {
        owned.iter().any(|a| {
            self.built_with
                .get(a)
                .is_some_and(|with| with.contains(&id))
        })
    }
}

fn items(name: &str, champ: &Champion) -> &'static Items {
    static CACHE: OnceLock<std::sync::Mutex<HashMap<String, &'static Items>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap();
    if let Some(items) = cache.get(name) {
        return items;
    }
    let ids = |keys: &mut dyn Iterator<Item = &str>| -> Vec<u32> {
        keys.filter_map(|id| id.parse().ok()).collect()
    };
    let mut items = Items::default();
    for (key, next) in &champ.level.sets {
        let owned = ids(&mut key.split(','));
        let next = ids(&mut next.keys().map(String::as_str));
        items.support.extend(owned.iter().chain(&next));
        for &a in &owned {
            let with = items.built_with.entry(a).or_default();
            with.extend(owned.iter().chain(&next));
        }
        for &b in &next {
            items.built_with.entry(b).or_default().extend(&owned);
        }
    }
    // Build-step keys are step numbers, not items.
    for next in champ.level.nth.values() {
        items
            .support
            .extend(ids(&mut next.keys().map(String::as_str)));
    }
    // One small table per covered champion-role, kept for the process lifetime.
    let items: &'static Items = Box::leak(Box::new(items));
    cache.insert(name.to_string(), items);
    items
}

/// Whether Master+ players of this champion and role complete `id` at some step of their build.
/// None when the corpus does not cover the champion and role.
pub fn builds(champion_key: u32, role: Position, id: u32) -> Option<bool> {
    let name = format!("{champion_key}|{}", role.label());
    let champ = doc().champions.get(&name)?;
    Some(items(&name, champ).support.contains(&normalize(id)))
}

/// Whether they have built `id` in one build with any of `owned` (true with nothing owned).
/// None when the corpus does not cover the champion and role.
pub fn built_with(champion_key: u32, role: Position, id: u32, owned: &[u32]) -> Option<bool> {
    let name = format!("{champion_key}|{}", role.label());
    let champ = doc().champions.get(&name)?;
    let owned: Vec<u32> = owned.iter().map(|&id| normalize(id)).collect();
    Some(owned.is_empty() || items(&name, champ).built_with_any(normalize(id), &owned))
}

fn normalized(p: impl Iterator<Item = (u32, f64)>) -> HashMap<u32, f64> {
    let mut p: HashMap<u32, f64> = p.filter(|(_, v)| *v > 0.0).collect();
    let total: f64 = p.values().sum();
    if total > 0.0 {
        p.values_mut().for_each(|v| *v /= total);
    }
    p
}

struct Lookup<'a> {
    champ: &'a Champion,
    role: Option<&'a Level>,
    items: &'a Items,
    memo: HashMap<Vec<u32>, HashMap<u32, f64>>,
}

impl Lookup<'_> {
    /// The smoothed next-item distribution for a sorted owned set (owned items not yet removed).
    fn owned_set(&mut self, owned: &[u32]) -> HashMap<u32, f64> {
        if let Some(p) = self.memo.get(owned) {
            return p.clone();
        }
        let key = owned
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let nth = (owned.len() + 1).to_string();
        let p_role = smooth(
            self.role.and_then(|r| r.nth.get(&nth)),
            &HashMap::new(),
            0.0,
        );
        let p_role_set = smooth(self.role.and_then(|r| r.sets.get(&key)), &p_role, A_ROLE);
        // The role's players are a prior only among items this champion builds.
        let items = self.items;
        let p_role_set = normalized(
            p_role_set
                .into_iter()
                .filter(|(id, _)| items.support.contains(id)),
        );
        let mut p_nth = smooth(self.champ.level.nth.get(&nth), &p_role_set, A_NTH);
        if !owned.is_empty() {
            for (id, v) in p_nth.iter_mut() {
                if !items.built_with_any(*id, owned) {
                    *v *= NOT_BUILT_WITH;
                }
            }
        }
        let prior = if owned.len() >= 2 {
            let unowned = |p: HashMap<u32, f64>| {
                normalized(p.into_iter().filter(|(id, _)| !owned.contains(id)))
            };
            let mut smaller: HashMap<u32, f64> = HashMap::new();
            let mut sets = 0.0;
            for skip in 0..owned.len() {
                let subset: Vec<u32> = owned
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| *index != skip)
                    .map(|(_, id)| *id)
                    .collect();
                let p = unowned(self.owned_set(&subset));
                if !p.is_empty() {
                    sets += 1.0;
                    for (id, v) in p {
                        *smaller.entry(id).or_default() += v;
                    }
                }
            }
            if sets > 0.0 {
                let mut prior: HashMap<u32, f64> = unowned(p_nth)
                    .into_iter()
                    .map(|(id, v)| (id, (1.0 - SUBSET_SHARE) * v))
                    .collect();
                for (id, v) in smaller {
                    *prior.entry(id).or_default() += SUBSET_SHARE * v / sets;
                }
                prior
            } else {
                p_nth
            }
        } else {
            p_nth
        };
        let p = smooth(self.champ.level.sets.get(&key), &prior, A_SET);
        self.memo.insert(owned.to_vec(), p.clone());
        p
    }
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
    let name = format!("{champion_key}|{}", role.label());
    let champ = d.champions.get(&name)?;
    let mut owned: Vec<u32> = owned.iter().map(|&id| normalize(id)).collect();
    owned.sort_unstable();
    owned.dedup();
    let role_level = d.roles.get(role.label());
    let mut p = Lookup {
        champ,
        role: role_level,
        items: items(&name, champ),
        memo: HashMap::new(),
    }
    .owned_set(&owned);
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

    const YASUO: u32 = 157;
    const LUX: u32 = 99;
    const ZHONYAS: u32 = 3157;
    const GUARDIAN_ANGEL: u32 = 3026;
    const INFINITY_EDGE: u32 = 3031;

    #[test]
    fn the_role_backoff_never_adds_an_item_the_champion_does_not_build() {
        // The Yasuo game of 2026-10-02: the fifth item came 90% from the mid-lane table, which is
        // mostly mages, so the path ended in Zhonya's Hourglass (0 of 395 Master+ Yasuo players) and
        // a defensive promotion later moved it to second. Lux's path ended in Guardian Angel.
        assert_eq!(builds(YASUO, Position::Mid, ZHONYAS), Some(false));
        assert_eq!(builds(YASUO, Position::Mid, INFINITY_EDGE), Some(true));
        assert_eq!(builds(LUX, Position::Mid, ZHONYAS), Some(true));
        assert_eq!(builds(99_999, Position::Mid, ZHONYAS), None);
        for (champion, never) in [(YASUO, ZHONYAS), (LUX, GUARDIAN_ANGEL)] {
            let mut owned: Vec<u32> = Vec::new();
            for step in 0..6 {
                let d = distribution(champion, Position::Mid, &owned, None).unwrap();
                assert!(
                    d.iter().all(|(id, _)| *id != never),
                    "champion {champion} step {step}: {d:?}"
                );
                for (id, _) in &d {
                    assert_eq!(builds(champion, Position::Mid, *id), Some(true), "{id}");
                }
                let Some((next, _)) = d.first() else { break };
                owned.push(*next);
            }
        }
    }

    #[test]
    fn a_skipped_core_item_is_still_due_in_an_unusual_set() {
        // Stormrazor, Immortal Shieldbow and Death's Dance without Infinity Edge: rare as a set,
        // and the step-4 table alone says Guardian Angel because step-4 players already own
        // Infinity Edge. The sets one item smaller know it is still missing.
        let d = distribution(YASUO, Position::Mid, &[2526, 6673, 6333], None).unwrap();
        assert_eq!(d.first().map(|(id, _)| *id), Some(INFINITY_EDGE), "{d:?}");
    }

    #[test]
    fn uncovered_champion_roles_fall_back_to_the_caller() {
        assert!(distribution(99_999, Position::Top, &[], None).is_none());
        assert_eq!(normalize(3097), 3095);
    }
}
