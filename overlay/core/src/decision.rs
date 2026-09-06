//! Bounded candidate comparison. Scores are explicit policy values, not win probabilities.
use crate::coaching::{self, DecisionKind, Evidence, LearningTip};
use crate::ddragon::{normalize, Catalog, GrievousTrigger, Item, ShieldEffect};
use crate::engine::{
    self, Alternative, BuildPreference, Inputs, NextItem, PlanItem, PlannerPreferences,
};
use crate::live::{Me, Player};
use crate::pack::ControlKind;
use crate::shop;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct CandidateScore {
    pub id: u32,
    pub prior: f64,
    pub completion: f64,
    pub situation: f64,
    pub delay: f64,
    pub total: f64,
}

#[derive(Default)]
pub(crate) struct Selection {
    pub path: Vec<PlanItem>,
    pub options: Vec<PlanItem>,
    pub next: Option<NextItem>,
    pub learning: Option<LearningTip>,
    pub alternative: Option<Alternative>,
    pub preferences: PlannerPreferences,
    pub context: Vec<String>,
    pub warnings: Vec<String>,
    pub scores: Vec<CandidateScore>,
}

// These are inspectable policy weights, not fitted coefficients or disguised win probabilities.
// Completion and existing investment must be able to beat a modest situational preference.
const ORDER_PRIOR: f64 = 3.0;
const FINISH_NOW: f64 = 3.0;
const OWNED_CREDIT: f64 = 3.0;
const MAX_NEED_SCORE: f64 = 5.0;
/// Share of this champion's final builds a late item needs before it can be a candidate.
const MIN_LATE_PICK: f64 = 0.02;
/// Situational score at which an off-path item counts as a real detour (a verified cleanse scores
/// 3.2, anti-heal against a healer about 2.7; incidental stats stay well below 1).
const DETOUR_NEED: f64 = 1.5;

#[derive(Clone, Copy, Debug, Default)]
struct Archetype {
    physical: f64,
    magical: f64,
    frontline: f64,
}

impl Archetype {
    fn from_build(inp: &Inputs) -> Self {
        let mut ad = 0.0;
        let mut ap = 0.0;
        let mut durability = 0.0;
        for item in inp
            .aggregate
            .into_iter()
            .flat_map(|a| &a.core.ids)
            .filter_map(|id| inp.catalog.item(*id))
        {
            ad += item.stat("FlatPhysicalDamageMod").unwrap_or(0.0);
            ap += item.stat("FlatMagicDamageMod").unwrap_or(0.0);
            durability += item.stat("FlatHPPoolMod").unwrap_or(0.0) / 20.0
                + item.effects.armor.unwrap_or(0.0)
                + item.effects.magic_resist.unwrap_or(0.0);
        }
        // AP and AD are not interchangeable damage. This ratio only determines which
        // item-effect family is appropriate, never an estimate of a champion's DPS.
        let (physical, magical) = if ad + ap > 0.0 {
            (ad / (ad + ap), ap / (ad + ap))
        } else {
            match inp.traits.get(inp.champion).map(|t| t.damage.as_str()) {
                Some("ad") => (1.0, 0.0),
                Some("ap") => (0.0, 1.0),
                _ => (0.5, 0.5),
            }
        };
        Self {
            physical,
            magical,
            frontline: (durability / (durability + ad + ap + 1.0)).clamp(0.0, 1.0),
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Needs {
    armor: f64,
    armor_name: String,
    magic_resist: f64,
    mr_name: String,
    healing: f64,
    healing_name: String,
    healing_observed: bool,
    suppression: Option<(String, String)>,
    physical_share: f64,
    magic_share: f64,
    physical_name: String,
    magic_name: String,
    pressure: f64,
    dive: f64,
    poke: f64,
    ally_antiheal: Vec<String>,
    context: Vec<String>,
}

fn equipped_value(cat: &Catalog, p: &Player) -> f64 {
    p.items
        .iter()
        .filter(|i| i.slot < 6)
        .filter_map(|i| {
            cat.item(i.id)
                .map(|item| f64::from(item.total) * f64::from(i.count.min(6)))
        })
        .sum()
}

fn item_sum(cat: &Catalog, p: &Player, value: impl Fn(&Item) -> f64) -> f64 {
    p.items
        .iter()
        .filter(|i| i.slot < 6)
        .filter_map(|i| {
            cat.item(i.id)
                .map(|item| value(item) * f64::from(i.count.min(6)))
        })
        .sum()
}

impl Needs {
    fn from_state(inp: &Inputs) -> Self {
        let mut n = Self::default();
        let me = inp.live.and_then(|l| l.me.as_ref());
        let my_value = me
            .map(|m| equipped_value(inp.catalog, &m.player))
            .unwrap_or(0.0);
        let mut weighted_physical = 0.0;
        let mut weighted_magic = 0.0;
        let mut strongest_physical = 0.0;
        let mut strongest_magic = 0.0;
        let mut strongest_healing: f64 = 0.0;
        let mut strongest_pressure: f64 = 1.0;
        let own_role = inp.aggregate.map(engine::actual_position);
        let mut names: BTreeSet<String> = inp.enemies.iter().cloned().collect();
        if let Some(live) = inp.live {
            names.extend(live.enemies.iter().map(|p| p.champion.clone()));
        }
        for name in names {
            let t = inp.traits.get(&name);
            let player = inp.live.and_then(|l| {
                l.enemies
                    .iter()
                    .find(|p| normalize(&p.champion) == normalize(&name))
            });
            let value = player
                .map(|p| equipped_value(inp.catalog, p))
                .unwrap_or(0.0);
            // Scoreboard-visible equipment/levels, not inferred wallets or future cooldowns.
            let relative = if my_value > 0.0 {
                ((value + 1000.0) / (my_value + 1000.0)).clamp(0.5, 2.0)
            } else {
                1.0
            };
            let levels = match (player, me) {
                (Some(p), Some(m)) if m.player.level > 0 => {
                    (f64::from(p.level) / f64::from(m.player.level)).clamp(0.5, 1.5)
                }
                _ => 1.0,
            };
            let weight = relative * levels;
            strongest_pressure = strongest_pressure.max(weight);
            let equipment_ap = player
                .map(|p| {
                    item_sum(inp.catalog, p, |i| {
                        i.stat("FlatMagicDamageMod").unwrap_or(0.0)
                    })
                })
                .unwrap_or(0.0);
            let equipment_ad = player
                .map(|p| {
                    item_sum(inp.catalog, p, |i| {
                        i.stat("FlatPhysicalDamageMod").unwrap_or(0.0)
                    })
                })
                .unwrap_or(0.0);
            let prior_magic = match t.map(|t| t.damage.as_str()) {
                Some("ap") => 1.0,
                Some("ad") => 0.0,
                _ => 0.5,
            };
            let magic = if equipment_ap + equipment_ad > 0.0 {
                // Item stats adjust, rather than replace, the champion's damage-type prior.
                0.6 * prior_magic + 0.4 * equipment_ap / (equipment_ap + equipment_ad)
            } else {
                prior_magic
            };
            weighted_magic += weight * magic;
            weighted_physical += weight * (1.0 - magic);
            if weight * magic > strongest_magic {
                strongest_magic = weight * magic;
                n.magic_name = name.clone();
            }
            if weight * (1.0 - magic) > strongest_physical {
                strongest_physical = weight * (1.0 - magic);
                n.physical_name = name.clone();
            }
            if let Some(p) = player {
                let armor = item_sum(inp.catalog, p, |i| i.effects.armor.unwrap_or(0.0));
                let mr = item_sum(inp.catalog, p, |i| i.effects.magic_resist.unwrap_or(0.0));
                if armor > n.armor {
                    n.armor = armor;
                    n.armor_name = name.clone();
                }
                if mr > n.magic_resist {
                    n.magic_resist = mr;
                    n.mr_name = name.clone();
                }
                let sustain = item_sum(inp.catalog, p, |i| {
                    i.effects.life_steal.unwrap_or(0.0) + i.effects.omnivamp.unwrap_or(0.0)
                });
                if sustain > 0.0 {
                    n.healing = (n.healing + sustain * 2.0).min(1.0);
                    // The reason names the biggest healing source, not the first one seen.
                    if sustain * 2.0 > strongest_healing {
                        strongest_healing = sustain * 2.0;
                        n.healing_name = name.clone();
                    }
                    n.healing_observed = true;
                }
            }
            if let Some(t) = t {
                if t.healing {
                    let in_lane = player
                        .and_then(|p| crate::aggregate::Position::parse(&p.position))
                        .is_some_and(|r| {
                            Some(r) == own_role
                                || (own_role == Some(crate::aggregate::Position::Adc)
                                    && r == crate::aggregate::Position::Support)
                        });
                    let contribution = if in_lane { 0.8 } else { 0.55 };
                    n.healing = (n.healing + contribution).min(1.0);
                    if contribution > strongest_healing {
                        strongest_healing = contribution;
                        n.healing_name = name.clone();
                    }
                }
                if t.assassin || t.burst {
                    n.dive = (n.dive + 0.25 * weight).min(1.0);
                }
                if t.poke {
                    n.poke = (n.poke + 0.3).min(1.0);
                }
                if let Some(control) = &t.control {
                    if control.kind == ControlKind::Suppression
                        && inp
                            .catalog
                            .version
                            .starts_with(&format!("{}.", control.verified_patch))
                        && player.is_none_or(|p| p.level >= 6)
                    {
                        n.suppression = Some((name.clone(), control.ability.clone()));
                    }
                }
            }
        }
        let total = weighted_physical + weighted_magic;
        if total > 0.0 {
            n.physical_share = weighted_physical / total;
            n.magic_share = weighted_magic / total;
        }
        n.pressure = (strongest_pressure - 1.0).clamp(0.0, 1.0);
        if let Some(live) = inp.live {
            for ally in &live.allies {
                if ally.items.iter().any(|i| {
                    inp.catalog
                        .item(i.id)
                        .is_some_and(|i| i.effects.grievous_wounds.is_some())
                }) {
                    n.ally_antiheal.push(ally.champion.clone());
                }
            }
            if !n.ally_antiheal.is_empty() {
                // Coverage is uncertain: allies may hit another target. Never count each ally as
                // another multiplicative reduction or treat presence as guaranteed application.
                n.healing *= 0.5;
                n.context.push(format!(
                    "Ally anti-heal seen on {}; coverage on your target is not guaranteed",
                    n.ally_antiheal.join(", ")
                ));
            }
            if let (Some(m), Some(role)) = (me, own_role) {
                if let Some(opponent) = engine::lane_opponent(inp, role)
                    .and_then(|name| live.enemies.iter().find(|p| p.champion == name))
                {
                    let diff = equipped_value(inp.catalog, &m.player)
                        - equipped_value(inp.catalog, opponent);
                    n.context.push(format!(
                        "Visible equipment vs {}: {:+.0}g; not total earned gold",
                        opponent.champion, diff
                    ));
                }
            }
            if own_role == Some(crate::aggregate::Position::Adc) {
                let supports: Vec<_> = live
                    .allies
                    .iter()
                    .filter(|p| {
                        crate::aggregate::Position::parse(&p.position)
                            == Some(crate::aggregate::Position::Support)
                    })
                    .collect();
                if supports.len() == 1 {
                    n.context.push(format!(
                        "Lane partner: {}. Both bot-lane opponents affect item needs",
                        supports[0].champion
                    ));
                }
            }
        }
        n
    }
}

#[derive(Clone, Debug)]
struct Fit {
    score: f64,
    kind: DecisionKind,
    reason: String,
    evidence: Evidence,
}

fn covered(cat: &Catalog, ids: &[u32], effect: impl Fn(&Item) -> bool) -> bool {
    ids.iter().filter_map(|id| cat.item(*id)).any(effect)
}

fn fit(
    item: &Item,
    inp: &Inputs,
    n: &Needs,
    archetype: Archetype,
    already: &[u32],
    pref: BuildPreference,
) -> Fit {
    let e = &item.effects;
    let mut terms: Vec<(f64, DecisionKind, String, Evidence)> = Vec::new();
    let short = engine::short_of(inp.pack, &item.name);
    if let Some(pen) = e.percent_armor_pen {
        if !covered(inp.catalog, already, |i| {
            i.effects.percent_armor_pen.is_some()
        }) && n.armor > 0.0
        {
            terms.push((
                5.0 * archetype.physical * (pen / 0.35).min(1.3) * n.armor / (100.0 + n.armor),
                DecisionKind::ArmorPen,
                format!(
                    "{short}: {} has +{:.0} armor from visible items",
                    n.armor_name, n.armor
                ),
                Evidence::VisibleItems,
            ));
        }
    }
    if let Some(pen) = e.percent_magic_pen {
        if !covered(inp.catalog, already, |i| {
            i.effects.percent_magic_pen.is_some()
        }) && n.magic_resist > 0.0
        {
            terms.push((
                5.0 * archetype.magical * (pen / 0.40).min(1.3) * n.magic_resist
                    / (70.0 + n.magic_resist),
                DecisionKind::MagicPen,
                format!(
                    "{short}: {} has +{:.0} MR from visible items",
                    n.mr_name, n.magic_resist
                ),
                Evidence::VisibleItems,
            ));
        }
    }
    if e.grievous_wounds.is_some()
        && !covered(inp.catalog, already, |i| {
            i.effects.grievous_wounds.is_some()
        })
    {
        let application = match e.grievous_trigger {
            Some(GrievousTrigger::PhysicalDamage) => archetype.physical,
            Some(GrievousTrigger::MagicDamage) => archetype.magical,
            Some(GrievousTrigger::AnyDamage) => 1.0,
            Some(GrievousTrigger::WhenAttacked) => archetype.frontline * 0.65,
            None => 0.0,
        };
        let reason = if e.grievous_trigger == Some(GrievousTrigger::WhenAttacked) {
            format!(
                "{short}: reduces {}'s healing only when they attack you",
                n.healing_name
            )
        } else {
            format!(
                "{short}: anti-heal for {}{}",
                n.healing_name,
                if n.ally_antiheal.is_empty() {
                    ""
                } else {
                    "; ally coverage is uncertain"
                }
            )
        };
        terms.push((
            3.4 * n.healing * application,
            DecisionKind::AntiHeal,
            reason,
            if n.healing_observed {
                Evidence::VisibleItems
            } else {
                Evidence::Composition
            },
        ));
    }
    if e.cleanse.is_some() && !covered(inp.catalog, already, |i| i.effects.cleanse.is_some()) {
        if let Some((champ, ability)) = &n.suppression {
            terms.push((
                3.2,
                DecisionKind::Cleanse,
                format!("{short}: its active removes {champ}'s {ability} suppression"),
                Evidence::Composition,
            ));
        }
    }
    let defense_weight = if pref == BuildPreference::Survival {
        2.8
    } else {
        0.9 + 0.9 * n.pressure
    };
    // A cleanse item's magic resistance is a side stat; its reason to exist is the active. It is
    // scored above as a cleanse only, never sold as "magic protection".
    if let Some(mr) = e.magic_resist.filter(|v| *v > 0.0 && e.cleanse.is_none()) {
        let old: f64 = already
            .iter()
            .filter_map(|id| inp.catalog.item(*id))
            .map(|i| i.effects.magic_resist.unwrap_or(0.0))
            .sum();
        terms.push((
            defense_weight * n.magic_share * mr / (40.0 + old),
            DecisionKind::MagicDefense,
            format!(
                "{short}: magic protection for {}'s damage profile",
                n.magic_name
            ),
            Evidence::Composition,
        ));
    }
    if let Some(armor) = e.armor.filter(|v| *v > 0.0) {
        let old: f64 = already
            .iter()
            .filter_map(|id| inp.catalog.item(*id))
            .map(|i| i.effects.armor.unwrap_or(0.0))
            .sum();
        terms.push((
            defense_weight * n.physical_share * armor / (40.0 + old),
            DecisionKind::PhysicalDefense,
            format!("{short}: armor for {}'s damage profile", n.physical_name),
            Evidence::Composition,
        ));
    }
    if e.shield.is_some() || e.spell_shield || e.stasis {
        let relevance = if e.shield == Some(ShieldEffect::Magic) {
            n.magic_share
        } else {
            1.0
        };
        terms.push((
            defense_weight * relevance * n.dive,
            DecisionKind::AntiBurst,
            format!("{short}: a defensive buffer against their burst threats"),
            Evidence::Composition,
        ));
    }
    if let Some(sustain) = e.life_steal.or(e.omnivamp) {
        let old: f64 = already
            .iter()
            .filter_map(|id| inp.catalog.item(*id))
            .map(|i| i.effects.life_steal.or(i.effects.omnivamp).unwrap_or(0.0))
            .sum();
        terms.push((
            1.7 * n.poke * (sustain / (0.15 + old)).min(1.5) * (1.0 - 0.5 * n.dive),
            DecisionKind::Sustain,
            format!("{short}: sustain for repeated poke trades"),
            Evidence::Composition,
        ));
    }
    terms.retain(|(v, _, _, _)| v.is_finite() && *v > 0.01);
    terms.sort_by(|a, b| b.0.total_cmp(&a.0));
    let score = terms.iter().map(|t| t.0).sum::<f64>().min(MAX_NEED_SCORE);
    let (_, kind, reason, evidence) = terms.into_iter().next().unwrap_or((
        0.0,
        DecisionKind::Core,
        format!("{short}: next in the most-played build for this champion and role"),
        Evidence::Aggregate,
    ));
    Fit {
        score,
        kind,
        reason,
        evidence,
    }
}

/// `part` is `whole` itself or somewhere in its recipe tree.
fn builds_into(cat: &Catalog, part: u32, whole: u32) -> bool {
    part == whole
        || cat
            .item(whole)
            .is_some_and(|item| item.from.iter().any(|&child| builds_into(cat, part, child)))
}

/// An offered detour the player answered by buying something else is declined for the rest of
/// the game. Progress toward the detour, consumables and trinkets are not an answer.
fn note_declined_detour(cat: &Catalog, ids: &[u32], preferences: &mut PlannerPreferences) {
    let Some(detour) = preferences.offered_detour else {
        return;
    };
    let mut inventory = ids.to_vec();
    inventory.sort_unstable();
    if ids.contains(&detour) {
        preferences.offered_detour = None;
        preferences.offered_inventory.clear();
        return;
    }
    let mut before = preferences.offered_inventory.clone();
    let declined = inventory.iter().any(|&id| {
        if let Some(index) = before.iter().position(|&b| b == id) {
            before.swap_remove(index);
            return false;
        }
        cat.item(id).is_some_and(|item| {
            !item
                .tags
                .iter()
                .any(|t| t == "Consumable" || t == "Trinket")
                && !builds_into(cat, id, detour)
        })
    });
    if declined {
        if !preferences.declined_detours.contains(&detour) {
            preferences.declined_detours.push(detour);
        }
        preferences.offered_detour = None;
        preferences.offered_inventory.clear();
    }
}

fn owned_ids(me: Option<&Me>) -> Vec<u32> {
    me.into_iter()
        .flat_map(|m| &m.player.items)
        .filter(|i| i.slot < 6)
        .flat_map(|i| std::iter::repeat_n(i.id, i.count.min(6) as usize))
        .collect()
}

fn fulfilled(inp: &Inputs, id: u32, me: Option<&Me>) -> bool {
    me.is_some_and(|m| {
        m.player.has_item(id)
            || shop::quote(inp.catalog, id, &m.player.items, 0.0, false)
                .blocked
                .as_deref()
                == Some("Target is already owned")
    })
}

fn commitment(inp: &Inputs, me: Option<&Me>) -> Vec<PlanItem> {
    let mut items = me.map(|m| m.player.items.clone()).unwrap_or_default();
    items.sort_by_key(|i| i.slot);
    items
        .into_iter()
        .filter(|i| {
            i.slot < 6
                && inp
                    .catalog
                    .item(i.id)
                    .is_none_or(|d| d.is_owned_commitment(inp.catalog))
        })
        .map(|i| {
            let mut p = engine::item_by_id(
                inp.catalog,
                inp.pack,
                i.id,
                Some("Already owned; no automatic sale".into()),
            )
            .unwrap_or(PlanItem {
                id: i.id,
                name: i.name.clone(),
                short: i.name,
                role: "owned".into(),
                ..Default::default()
            });
            p.owned = true;
            p
        })
        .take(6)
        .collect()
}

fn pool(inp: &Inputs) -> BTreeMap<u32, f64> {
    let mut result = BTreeMap::new();
    let Some(a) = inp.aggregate else {
        return result;
    };
    // Late items that almost nobody playing this champion buys (Randuin's Omen on Xayah at
    // 0.3%) are not candidates: a situational score must not resurrect them. Core lines and
    // boots stay regardless of their share.
    for line in a
        .late
        .iter()
        .filter(|line| line.pick_rate >= MIN_LATE_PICK)
        .chain(a.core_lines.iter())
        .chain(std::iter::once(&a.core))
        .chain(a.boots.iter())
    {
        for &id in &line.ids {
            result
                .entry(id)
                .and_modify(|v: &mut f64| *v = v.max(line.pick_rate))
                .or_insert(line.pick_rate);
        }
    }
    // A situational component is a factual recipe detour, not a hand-picked champion build.
    let mut stack: Vec<_> = result.keys().copied().collect();
    let mut visited = BTreeSet::new();
    while let Some(id) = stack.pop() {
        if !visited.insert(id) || visited.len() > 512 {
            continue;
        }
        if let Some(item) = inp.catalog.item(id) {
            for &part in &item.from {
                result.entry(part).or_insert(0.0);
                stack.push(part);
            }
        }
    }
    result
}

fn remaining(
    inp: &Inputs,
    id: u32,
    me: Option<&Me>,
    locked: bool,
    swiftplay: bool,
) -> shop::ShopQuote {
    let context = shop::ShopContext {
        champion: Some(inp.champion),
        spell_ids: me
            .filter(|m| !m.spell_ids.is_empty())
            .map(|m| m.spell_ids.as_slice()),
        boots_locked: locked,
        swiftplay,
    };
    shop::quote_with_context(
        inp.catalog,
        id,
        me.map(|m| m.player.items.as_slice()).unwrap_or(&[]),
        me.map(|m| m.gold).unwrap_or(0.0),
        &context,
    )
}

/// `pregame_spells` is the planned pair before a game (the live loadout wins once observed);
/// `swiftplay` switches the shop rules (Doran's disabled, Guardian's sold).
pub(crate) fn select(
    inp: &Inputs,
    base: Vec<PlanItem>,
    preferences: &PlannerPreferences,
    boots_locked: bool,
    pregame_spells: &[u32],
    swiftplay: bool,
) -> Selection {
    let me = inp.live.and_then(|l| l.me.as_ref());
    let cat = inp.catalog;
    let ids = owned_ids(me);
    let archetype = Archetype::from_build(inp);
    let needs = Needs::from_state(inp);
    let mut out = Selection {
        preferences: preferences.clone(),
        context: needs.context.clone(),
        ..Default::default()
    };
    let Some(agg) = inp.aggregate else { return out };
    note_declined_detour(cat, &ids, &mut out.preferences);
    let choices = pool(inp);
    let mut path = commitment(inp, me);
    let full_committed = path.len() == 6;
    let committed_ids: Vec<_> = path.iter().map(|p| p.id).collect();
    let consuming_upgrade = |quote: &shop::ShopQuote| {
        !full_committed
            || (quote.blocked.is_none()
                && quote
                    .components
                    .iter()
                    .any(|c| c.owned && committed_ids.contains(&c.id)))
    };
    let context = shop::ShopContext {
        champion: Some(inp.champion),
        spell_ids: me
            .filter(|m| !m.spell_ids.is_empty())
            .map(|m| m.spell_ids.as_slice())
            .or_else(|| (me.is_none() && !pregame_spells.is_empty()).then_some(pregame_spells)),
        boots_locked,
        swiftplay,
    };
    let compatible = |id, owned: &[u32]| shop::compatible_with_context(cat, id, owned, &context);
    let core_ids = &agg.core.ids;
    let first = core_ids.first().copied();
    let alternative_first_owned = agg
        .core_alternatives
        .iter()
        .any(|id| cat.item(*id).is_some_and(|i| i.is_finished(cat)) && fulfilled(inp, *id, me));
    let completed_core = path
        .iter()
        .filter(|p| {
            cat.item(p.id).is_some_and(|i| i.is_finished(cat))
                && (core_ids.contains(&p.id) || agg.core_alternatives.contains(&p.id))
        })
        .count();
    // Core order is a prior. Bought items are immutable; an alternative first item
    // fills that commitment rather than forcing a second first-item purchase.
    for item in base
        .iter()
        .filter(|i| core_ids.contains(&i.id) || i.role == "boots")
    {
        if path.len() >= 6 {
            break;
        }
        if fulfilled(inp, item.id, me)
            || (Some(item.id) == first && alternative_first_owned)
            || !compatible(item.id, &ids)
            || !compatible(item.id, &path.iter().map(|p| p.id).collect::<Vec<_>>())
        {
            continue;
        }
        path.push(item.clone());
    }
    // Greedy marginal coverage for the small flexible tail. Effects already present
    // are discounted; mutually exclusive items are filtered before scoring.
    while path.len() < 6 {
        let planned: Vec<u32> = path.iter().map(|p| p.id).collect();
        let mut ranked = Vec::new();
        for (&id, &pick) in &choices {
            let Some(item) = cat.item(id) else { continue };
            if !item.is_finished(cat)
                || item.effects.boots
                || planned.contains(&id)
                || fulfilled(inp, id, me)
                || agg.core_alternatives.contains(&id)
                || !compatible(id, &ids)
                || !compatible(id, &planned)
            {
                continue;
            }
            let q = remaining(inp, id, me, boots_locked, swiftplay);
            let credit = q
                .remaining_cost
                .map(|cost| 1.0 - f64::from(cost) / f64::from(item.total.max(1)))
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            let f = fit(item, inp, &needs, archetype, &planned, preferences.mode);
            let score = 2.0 * pick.max(0.0).sqrt() + f.score + OWNED_CREDIT * credit;
            ranked.push((score, id, f));
        }
        ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        let Some((_, id, f)) = ranked.into_iter().next() else {
            break;
        };
        if let Some(mut item) = engine::item_by_id(cat, inp.pack, id, Some(f.reason)) {
            // A named need is worth a tag at any real score; the generic label needs a clear one,
            // so a Guardian Angel hovering around the threshold does not flicker between polls.
            let named = matches!(
                f.kind,
                DecisionKind::AntiHeal
                    | DecisionKind::Cleanse
                    | DecisionKind::ArmorPen
                    | DecisionKind::MagicPen
            );
            if f.score > 0.2 && named || f.score >= 0.75 {
                item.tag = Some(
                    match f.kind {
                        DecisionKind::AntiHeal => "anti-heal",
                        DecisionKind::Cleanse => "cleanse",
                        DecisionKind::ArmorPen => "armor",
                        DecisionKind::MagicPen => "MR",
                        _ => "situational",
                    }
                    .into(),
                );
            }
            path.push(item);
        }
    }
    let pending: Vec<_> = path
        .iter()
        .filter(|p| !p.owned && !(boots_locked && p.role == "boots"))
        .map(|p| p.id)
        .collect();
    let baseline = pending.first().copied();
    let baseline_cost = baseline
        .and_then(|id| remaining(inp, id, me, boots_locked, swiftplay).remaining_cost)
        .unwrap_or(1)
        .max(1);
    let mut ranked = Vec::new();
    for (&id, &pick) in &choices {
        let Some(item) = cat.item(id) else { continue };
        if fulfilled(inp, id, me) || (boots_locked && item.effects.boots) || !compatible(id, &ids) {
            continue;
        }
        let q = remaining(inp, id, me, boots_locked, swiftplay);
        if q.remaining_cost.is_none() || !consuming_upgrade(&q) {
            continue;
        }
        let cost = q.remaining_cost.unwrap();
        let credit = (1.0 - f64::from(cost) / f64::from(item.total.max(1))).clamp(0.0, 1.0);
        let ordinal = pending.iter().position(|p| *p == id);
        let f = fit(item, inp, &needs, archetype, &ids, preferences.mode);
        let situational_component = !item.is_finished(cat)
            && !item.effects.boots
            && (item.effects.cleanse.is_some() || item.effects.grievous_wounds.is_some())
            && f.score > 0.0;
        let eligible = if me.is_none() {
            Some(id) == baseline
        } else {
            Some(id) == baseline
                || ordinal.is_some_and(|o| o <= 1)
                || credit > 0.0 && item.is_finished(cat)
                || completed_core >= 1 && situational_component
                || completed_core >= 2 && item.is_finished(cat) && f.score > 0.25
        };
        if !eligible {
            continue;
        }
        // No progression can currently fit in a full bag: keep it as a future target,
        // but do not let that blocked action beat a legal alternative solely on price.
        let blocked = q.blocked.is_some();
        let prior = ordinal
            .map(|o| ORDER_PRIOR / (1.0 + o as f64))
            .unwrap_or(0.2 * pick.sqrt());
        // "Affordable right now" is a reason to buy something you were building anyway (the next
        // planned item) or a detour with a real, verified need (anti-heal against a healer, a
        // cleanse against suppression). An off-path item that merely happens to be affordable
        // (Stormrazor sharing IE's components) must not pull the player off the core item.
        let planned = (Some(id) == baseline || pending.contains(&id) || f.score >= DETOUR_NEED)
            && !out.preferences.declined_detours.contains(&id);
        let completion = OWNED_CREDIT * credit
            + if q.affordable && planned {
                FINISH_NOW
            } else {
                0.0
            };
        let phase = if Some(id) == baseline || completed_core >= 2 {
            1.0
        } else if completed_core >= 1 {
            0.85
        } else {
            0.0
        };
        let situation = f.score * phase;
        let delay = 0.4 * (f64::from(cost) / f64::from(baseline_cost) - 1.0).max(0.0)
            + if blocked { 4.0 } else { 0.0 };
        let total = prior + completion + situation - delay;
        ranked.push((
            CandidateScore {
                id,
                prior,
                completion,
                situation,
                delay,
                total,
            },
            f,
            q,
            credit,
        ));
    }
    // Feasibility is a constraint, not another soft policy score. A blocked target
    // cannot win over a legal purchase just because its situational score is large.
    ranked.sort_by(|a, b| {
        a.2.blocked
            .is_some()
            .cmp(&b.2.blocked.is_some())
            .then_with(|| b.0.total.total_cmp(&a.0.total))
            .then_with(|| a.0.id.cmp(&b.0.id))
    });
    if let Some(id) = preferences.pinned_item {
        if fulfilled(inp, id, me) {
            out.preferences.pinned_item = None;
            out.context
                .push("Pinned target completed; back to automatic recommendations".into());
        } else if choices.contains_key(&id)
            && compatible(id, &ids)
            && remaining(inp, id, me, boots_locked, swiftplay)
                .remaining_cost
                .is_some()
            && consuming_upgrade(&remaining(inp, id, me, boots_locked, swiftplay))
        {
            if let Some(index) = ranked.iter().position(|r| r.0.id == id) {
                let chosen = ranked.remove(index);
                ranked.insert(0, chosen);
            } else if let Some(item) = cat.item(id) {
                ranked.insert(
                    0,
                    (
                        CandidateScore {
                            id,
                            ..Default::default()
                        },
                        fit(item, inp, &needs, archetype, &ids, preferences.mode),
                        remaining(inp, id, me, boots_locked, swiftplay),
                        0.0,
                    ),
                );
            }
        } else {
            out.preferences.pinned_item = None;
            out.warnings.push("Pinned target is unavailable or incompatible with your inventory; returned to Auto".into());
        }
    }
    out.scores = ranked.iter().map(|r| r.0.clone()).collect();
    if let Some((score, _, _, _)) = ranked.first() {
        // Remember an affordable detour together with the bag it was offered against.
        if Some(score.id) != baseline
            && !pending.contains(&score.id)
            && out.preferences.offered_detour != Some(score.id)
        {
            out.preferences.offered_detour = Some(score.id);
            let mut inventory = ids.clone();
            inventory.sort_unstable();
            out.preferences.offered_inventory = inventory;
        }
    }
    if let Some((score, f, q, credit)) = ranked.first() {
        if let Some(mut target) = engine::item_by_id(cat, inp.pack, score.id, None) {
            let pinned = out.preferences.pinned_item == Some(score.id);
            let (kind, reason, evidence) = if pinned {
                (
                    DecisionKind::Pinned,
                    format!(
                        "{}: your pinned target; existing components are counted",
                        target.short
                    ),
                    Evidence::PlayerChoice,
                )
            } else if q.affordable
                && *credit > 0.0
                && !cat.item(score.id).is_some_and(|i| i.effects.boots)
            {
                (
                    DecisionKind::Completion,
                    format!(
                        "{}: only {}g left with your components",
                        target.short,
                        q.remaining_cost.unwrap_or(0)
                    ),
                    Evidence::Inventory,
                )
            } else if f.score > 0.35 && score.situation > 0.0 {
                (f.kind, f.reason.clone(), f.evidence)
            } else if target.role == "boots" {
                (
                    DecisionKind::Boots,
                    format!(
                        "{}: movement and the common boot upgrade for this role",
                        target.short
                    ),
                    Evidence::Aggregate,
                )
            } else {
                (
                    DecisionKind::Core,
                    format!(
                        "{}: next in the most-played build for {} {}",
                        target.short,
                        inp.champion,
                        agg.position.label()
                    ),
                    Evidence::Aggregate,
                )
            };
            target.why = Some(reason.clone());
            if pinned {
                target.tag = Some("pinned".into());
            }
            out.next = Some(engine::next_for_target(
                cat,
                &target,
                me,
                boots_locked,
                swiftplay,
            ));
            out.learning = Some(coaching::explain(kind, reason, evidence));
            // Reorder only unowned commitments. A component detour stays outside the
            // six-item horizon, leaving the main build ready to resume afterwards.
            if full_committed {
                out.context.push(
                    "Upgrade an owned item; no extra inventory slot or automatic sale".into(),
                );
            } else if cat.item(target.id).is_some_and(|i| i.is_finished(cat)) {
                path.retain(|p| p.id != target.id || p.owned);
                if !path.iter().any(|p| p.id == target.id && p.owned) {
                    path.retain(|p| p.owned || compatible(p.id, &[target.id]));
                    let index = path.iter().take_while(|p| p.owned).count();
                    path.insert(index, target.clone());
                    path.truncate(6);
                }
            } else {
                out.context.push(
                    "Temporary detour; the main item path resumes after this purchase".into(),
                );
            }
            if let Some((other, other_fit, quote, _)) =
                ranked.iter().skip(1).find(|r| r.2.blocked.is_none())
            {
                if let Some(item) = engine::item_by_id(cat, inp.pack, other.id, None) {
                    out.alternative = Some(Alternative {
                        item,
                        remaining_cost: quote.remaining_cost,
                        reason: other_fit.reason.clone(),
                    });
                }
            }
        }
    } else if let Some(id) = baseline {
        if let Some(target) = engine::item_by_id(cat, inp.pack, id, None) {
            out.next = Some(engine::next_for_target(
                cat,
                &target,
                me,
                boots_locked,
                swiftplay,
            ));
            out.learning = Some(coaching::explain(
                DecisionKind::Core,
                format!("{}: next in the current build", target.short),
                Evidence::Aggregate,
            ));
        }
    } else if full_committed {
        out.preferences.pinned_item = None;
        out.context
            .push("Full build; no automatic sales or seventh-item purchases".into());
    }
    out.options = choices
        .iter()
        .filter(|(id, _)| {
            !path.iter().any(|p| p.id == **id)
                && !fulfilled(inp, **id, me)
                && compatible(**id, &ids)
        })
        .filter(|(id, _)| {
            cat.item(**id).is_some_and(|i| {
                i.is_finished(cat)
                    || i.effects.cleanse.is_some()
                    || i.effects.grievous_wounds.is_some()
            })
        })
        .filter_map(|(&id, _)| engine::item_by_id(cat, inp.pack, id, None))
        .collect();
    out.options.sort_by(|a, b| {
        choices[&b.id]
            .total_cmp(&choices[&a.id])
            .then_with(|| a.id.cmp(&b.id))
    });
    out.options.truncate(16);
    out.path = path;
    out
}
