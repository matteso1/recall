//! Bounded candidate comparison. Scores are explicit policy values, not win probabilities.
use crate::answers::Answer;
use crate::coaching::{self, DecisionKind, Evidence, LearningTip};
use crate::ddragon::{normalize, Catalog, GrievousTrigger, Item, ShieldEffect};
use crate::engine::{
    self, Alternative, BuildPreference, Inputs, NextItem, PlanItem, PlannerPreferences,
};
use crate::live::{self, Me, Player};
use crate::nextprior;
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
/// Converts the fraction of incoming damage an item's armor or magic resist removes (see
/// `Defense::removed`) into a need score. A first resist item early (about 40 of either at 60 armor
/// and 40 MR against an even mix) removes about a tenth of incoming damage; x5 keeps that near the
/// 0.5 the earlier per-item formula gave, so the scale of other needs is unchanged.
const RESIST_SCALE: f64 = 5.0;
/// A defensive item moves ahead of an earlier planned one only when it adds clearly more effective
/// health per remaining gold (15%): near ties keep op.gg's order and the target does not flip
/// between polls as levels and gold change.
const ORDER_MARGIN: f64 = 0.15;
/// Boots are the one slot chosen mostly by the enemy's damage type (Plated Steelcaps against
/// attacks, Mercury's Treads against magic), so their defensive fit counts three times against the
/// pick-rate prior, and only boots a real share of players buy (5%) are considered.
const BOOTS_FIT_WEIGHT: f64 = 3.0;
const MIN_BOOTS_PICK: f64 = 0.05;
/// Score the boots already on the path keep over a challenger: the defensive fit moves with every
/// fight, and without a hold the Xayah game of 2026-09-28 switched between Berserker's and
/// Gluttonous Greaves four times in three minutes.
const BOOTS_HOLD: f64 = 0.5;
/// Before a game (champion select planning) there are no measured stats; the first finished items
/// land around this level, so base resistances and health are taken there.
const PLANNING_LEVEL: u32 = 9;
/// Each level an enemy is ahead of (or behind) you changes their threat by 10%, held inside
/// 0.6-1.5. A ratio of levels made level 3 against level 2 look 50% more dangerous, which swung the
/// defensive scores, the tags and the tail order on every early level-up (Lux game, 2026-09-26).
const LEVEL_STEP: f64 = 0.1;
/// A fed enemy (more kills than deaths on the scoreboard) counts up to about half again as
/// dangerous: 8% per kill of lead, counting at most six.
const FED_STEP: f64 = 0.08;
const FED_LEAD_CAP: f64 = 6.0;
/// Kill-feed memory: one of your deaths counts fully when it just happened and fades out over six
/// minutes. The killer counts 1, each assister 0.5, capped at 2 per enemy.
const HUNT_MEMORY_SECONDS: f64 = 360.0;
const HUNT_CAP: f64 = 2.0;
/// Each point of kill-feed evidence raises that enemy's threat by 40% and is evidence of dive:
/// an enemy who reached and killed you is diving you, assassin trait or not.
const HUNT_THREAT: f64 = 0.4;
const HUNT_DIVE: f64 = 0.5;
/// An answer promoted ahead of the core at `DETOUR_NEED` stays promoted until its need falls
/// below this, so it does not flip as the evidence slowly fades.
const PROMOTE_KEEP: f64 = 1.2;
/// A tail candidate replaces the previous plan's choice only when it scores this much higher, and
/// a tag appears at its threshold but disappears only this far below it.
const TAIL_MARGIN: f64 = 0.25;
const TAG_MARGIN: f64 = 0.15;
/// Engine v3 (see nextprior.rs): the Master+ distribution already reacts to the enemy composition
/// (measured lifts: healer, magic-heavy, tanky). The situational needs add what the composition alone
/// cannot see, the live state (your measured armor/MR against their damage, their visible armor and
/// healing items), as a nudge of `V3_NUDGE` nats per unit of need capped at `V3_NEED_CAP` (0.6 nats,
/// under 2x the odds): enough to decide close choices, never to overturn a clear one (Guardian Angel
/// second on Sivir: 0 of 469 Master+ players, a ~4.7-nat gap).
const V3_NUDGE: f64 = 0.3;
/// Chain hysteresis: a choice the previous plan made keeps its step unless another item scores this
/// many nats more, so level and gold ticks do not reorder the path.
const V3_TIE_MARGIN: f64 = 0.25;
const V3_NEED_CAP: f64 = 2.0;
/// Legendary slots the chain plans (one slot stays for boots).
const V3_LEGENDARIES: usize = 5;
/// In v3 the chain already carries the needs, so the target ranking's situational term is capped
/// below the gap between the first two planned items.
const V3_SITUATION_CAP: f64 = 1.0;
/// The kill feed is the confirmation a composition-only detour lacks: a resistance answer against
/// the damage type of the enemy it blames is promoted at this need instead of `DETOUR_NEED`.
const TYPED_ANSWER_NEED: f64 = 1.0;
/// When a resistance answer against the blamed enemy and a generic buffer (a shield or stasis
/// without that resistance) both qualify, the resistance answer goes first unless the buffer's need
/// is higher by this much: resistance cuts all of that enemy's damage, a buffer one burst.
const TYPED_MARGIN: f64 = 1.0;
/// A promoted answer stays promoted this long after it last met its bar, so it does not drop and
/// return as one death fades and the next renews the evidence (the Xayah game of 2026-09-26 traded
/// Bloodthirster and Mortal Reminder six times between 23:40 and 28:00).
const PROMOTE_HOLD_SECONDS: f64 = 180.0;
/// A promoted answer the player has started buying (see `started`) stays promoted until it is
/// finished, while its need holds at `PROMOTE_KEEP`, and gives way only to an answer whose need is
/// this much higher: switching strands the gold already spent. In the Xayah game of 2026-09-26 the
/// player bought Null-Magic Mantle toward Mercurial Scimitar after "Lillia (3/0) killed you", then
/// one solo death to Yasuo switched the answer to Guardian Angel, Mercurial left the path, and the
/// Mantle was sold. A second answer for another enemy follows the committed one instead.
const COMMIT_MARGIN: f64 = 1.0;
/// The target shown last poll stays the target unless a challenger's total is this much higher
/// (or it was bought, declined, blocked or left the candidates). Finishable targets stay regardless.
const TARGET_MARGIN: f64 = 0.75;

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

/// Which damage family an item feeds, from its Data Dragon stats and described penetration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Family {
    Physical,
    Magical,
    Mixed,
}

pub(crate) fn family(item: &Item) -> Family {
    let ad = item.stat("FlatPhysicalDamageMod").unwrap_or(0.0)
        + item.stat("PercentAttackSpeedMod").unwrap_or(0.0)
        + item.stat("FlatCritChanceMod").unwrap_or(0.0)
        + item.effects.flat_armor_pen.unwrap_or(0.0)
        + item.effects.percent_armor_pen.unwrap_or(0.0);
    let ap = item.stat("FlatMagicDamageMod").unwrap_or(0.0)
        + item.effects.flat_magic_pen.unwrap_or(0.0)
        + item.effects.percent_magic_pen.unwrap_or(0.0);
    match (ad > 0.0, ap > 0.0) {
        (true, false) => Family::Physical,
        (false, true) => Family::Magical,
        _ => Family::Mixed,
    }
}

/// Whether a finished item belongs with the chosen core line. A core of on-hit items (Kraken,
/// Blade of the Ruined King, Terminus on Katarina) is a physical build even on an AP champion:
/// its tail and boots must not be the AP crowd's Lich Bane, Shadowflame and Sorcerer's Shoes.
/// Mixed items (Nashor's, Gunblade, Guinsoo's, defensive items without damage) fit either.
pub(crate) fn coherent(inp: &Inputs, id: u32) -> bool {
    coherent_with(Archetype::from_build(inp), inp.catalog, id)
}

fn coherent_with(archetype: Archetype, cat: &Catalog, id: u32) -> bool {
    let Some(item) = cat.item(id) else {
        return true;
    };
    match family(item) {
        Family::Magical => archetype.physical < 0.75,
        Family::Physical => archetype.magical < 0.75,
        Family::Mixed => true,
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
    /// Strongest cleanse call among enemies (0-1) and who: the champion and, for a verified
    /// suppression, the ability.
    cleanse: f64,
    cleanse_threat: Option<(String, Option<String>)>,
    physical_share: f64,
    magic_share: f64,
    physical_name: String,
    magic_name: String,
    pressure: f64,
    dive: f64,
    poke: f64,
    ally_antiheal: Vec<String>,
    context: Vec<String>,
    /// The enemy with the strongest kill-feed evidence against you (recent deaths they landed or
    /// assisted), named in the reason when a defensive answer is moved ahead of the core.
    hunter: Option<Hunter>,
    /// Every enemy's damage type (share of magic, 0-1) and strength without the kill feed
    /// (equipment, levels, kill lead; 1.0 is even with you), for what a committed answer still
    /// answers.
    threats: Vec<(String, f64, f64)>,
    /// The player's durability before any candidate is added.
    defense: Defense,
    /// Owned item ids (one per unit), so planned-but-unbought items can be told apart.
    owned: Vec<u32>,
}

#[derive(Clone, Debug, Default)]
struct Hunter {
    champion: String,
    kills: u32,
    deaths: u32,
    /// Landed at least one of the remembered kills (not only assisted).
    landed: bool,
    /// Kill-feed evidence (`hunting` below) times how strong the enemy is (equipment, levels, kill lead): the enemy blamed is
    /// the one with the most, so a far-behind enemy landing a teamfight's last hit is not named
    /// over the fed one who assisted every death.
    blame: f64,
    /// The share of this enemy's damage that is magic (0-1), for the resistance that answers it.
    magic: f64,
}

/// How much of your recent dying one enemy did: the killer counts 1 and an assister 0.5 per death,
/// fading linearly over `HUNT_MEMORY_SECONDS`. Returns (score, landed a kill).
fn hunting(deaths: &[live::Death], now: f64, champion: &str) -> (f64, bool) {
    let mut score = 0.0;
    let mut landed = false;
    for death in deaths {
        let fade = (1.0 - (now - death.time) / HUNT_MEMORY_SECONDS).clamp(0.0, 1.0);
        if fade <= 0.0 {
            continue;
        }
        if normalize(&death.killer) == normalize(champion) {
            score += fade;
            landed = true;
        } else if death
            .assisters
            .iter()
            .any(|a| normalize(a) == normalize(champion))
        {
            score += 0.5 * fade;
        }
    }
    (score.min(HUNT_CAP), landed)
}

/// The player's armor, magic resist and health, for valuing more of them as damage actually
/// removed. In game these are the client's measured totals (owned items, levels and passives
/// included); before a game, or when the client does not report them, the champion's Data Dragon
/// base stats at the current (or planning) level plus owned items.
#[derive(Clone, Copy, Debug, Default)]
struct Defense {
    armor: f64,
    magic_resist: f64,
    health: f64,
    observed: bool,
}

fn health_of(item: &Item) -> f64 {
    item.stat("FlatHPPoolMod").unwrap_or(0.0)
}

impl Defense {
    fn of(inp: &Inputs, me: Option<&Me>) -> Self {
        let level = me
            .map(|m| m.player.level)
            .filter(|level| *level > 0)
            .unwrap_or(PLANNING_LEVEL);
        let champion = inp
            .catalog
            .champion_key(inp.champion)
            .and_then(|key| inp.catalog.champion(key));
        // A typical champion near the planning level, only when Data Dragon lacks the champion.
        let base = |stat: &str, typical: f64| {
            champion
                .and_then(|c| c.stat_at(stat, level))
                .unwrap_or(typical)
        };
        let owned = |value: fn(&Item) -> f64| {
            me.map(|m| item_sum(inp.catalog, &m.player, value))
                .unwrap_or(0.0)
        };
        let health = me
            .and_then(|m| m.stats.max_health)
            .unwrap_or_else(|| base("hp", 1500.0) + owned(health_of));
        match me.map(|m| (m.stats.armor, m.stats.magic_resist)) {
            Some((Some(armor), Some(magic_resist))) => Self {
                armor,
                magic_resist,
                health,
                observed: true,
            },
            _ => Self {
                armor: base("armor", 60.0) + owned(|i| i.effects.armor.unwrap_or(0.0)),
                magic_resist: base("spellblock", 40.0)
                    + owned(|i| i.effects.magic_resist.unwrap_or(0.0)),
                health,
                observed: false,
            },
        }
    }

    /// With these items' stats added (items planned but not bought yet).
    fn plus(&self, cat: &Catalog, ids: impl IntoIterator<Item = u32>) -> Self {
        let mut out = *self;
        for item in ids.into_iter().filter_map(|id| cat.item(id)) {
            out.armor += item.effects.armor.unwrap_or(0.0);
            out.magic_resist += item.effects.magic_resist.unwrap_or(0.0);
            out.health += health_of(item);
        }
        out
    }

    /// Damage taken per point of incoming damage against the enemy mix.
    fn taken(&self, n: &Needs, armor: f64, magic_resist: f64) -> f64 {
        n.physical_share * 100.0 / (100.0 + (self.armor + armor).max(0.0))
            + n.magic_share * 100.0 / (100.0 + (self.magic_resist + magic_resist).max(0.0))
    }

    /// Fraction of all incoming damage that extra armor and magic resist remove: each damage
    /// type's share after current mitigation times the part of it the new resistance stops.
    fn removed(&self, n: &Needs, armor: f64, magic_resist: f64) -> f64 {
        let before = self.taken(n, 0.0, 0.0);
        if before <= 0.0 {
            return 0.0;
        }
        ((before - self.taken(n, armor, magic_resist)) / before).max(0.0)
    }

    /// Relative effective-health gain against the enemy mix from extra health and resistances.
    fn ehp_gain(&self, n: &Needs, health: f64, armor: f64, magic_resist: f64) -> f64 {
        let before = self.taken(n, 0.0, 0.0);
        let after = self.taken(n, armor, magic_resist);
        if before <= 0.0 || after <= 0.0 || self.health <= 0.0 {
            return 0.0;
        }
        (self.health + health) / after / (self.health / before) - 1.0
    }
}

/// `already` minus owned items, one unit at a time: what is planned but not bought yet.
fn unbought(already: &[u32], owned: &[u32]) -> Vec<u32> {
    let mut owned = owned.to_vec();
    already
        .iter()
        .copied()
        .filter(|id| match owned.iter().position(|o| o == id) {
            Some(index) => {
                owned.swap_remove(index);
                false
            }
            None => true,
        })
        .collect()
}

/// Lane phase: the lane opponent deals most of the damage a laner takes until roams and
/// objectives take over. x3 through 10:00, falling linearly to x1 at 20:00. Planning before the
/// game counts as lane phase.
fn lane_boost(game_time: Option<f64>) -> f64 {
    let minutes = game_time.map_or(0.0, |seconds| seconds / 60.0);
    3.0 - 2.0 * ((minutes - 10.0) / 10.0).clamp(0.0, 1.0)
}

/// The enemies a laner faces in lane: the opponent in the same position, and for bot lane both of
/// them. A jungler has no lane, so no one is weighted up.
fn lane_opponents(inp: &Inputs, role: Option<crate::aggregate::Position>) -> Vec<String> {
    use crate::aggregate::Position;
    let lanes: &[Position] = match role {
        Some(Position::Top) => &[Position::Top],
        Some(Position::Mid) => &[Position::Mid],
        Some(Position::Adc) | Some(Position::Support) => &[Position::Adc, Position::Support],
        _ => &[],
    };
    lanes
        .iter()
        .filter_map(|position| engine::lane_opponent(inp, *position))
        .map(|name| normalize(&name))
        .collect()
}

/// An item bought for durability: armor, magic resist or health and no damage stats.
fn defensive(item: &Item) -> bool {
    let damage = item.stat("FlatPhysicalDamageMod").unwrap_or(0.0)
        + item.stat("FlatMagicDamageMod").unwrap_or(0.0)
        + item.stat("PercentAttackSpeedMod").unwrap_or(0.0)
        + item.stat("FlatCritChanceMod").unwrap_or(0.0);
    damage <= 0.0
        && (item.effects.armor.is_some_and(|v| v > 0.0)
            || item.effects.magic_resist.is_some_and(|v| v > 0.0)
            || health_of(item) > 0.0)
}

/// The why line for armor or magic resist. When the measured balance is what makes it worth
/// buying (the other resistance at least half again as high), the line gives the numbers.
fn resist_reason(short: &str, n: &Needs, magic: bool) -> String {
    let d = &n.defense;
    let (mine, other, name) = if magic {
        (d.magic_resist, d.armor, &n.magic_name)
    } else {
        (d.armor, d.magic_resist, &n.physical_name)
    };
    if d.observed && !name.is_empty() && other >= 1.5 * mine.max(1.0) {
        return format!(
            "{short}: {:.0} armor vs {:.0} MR; {name} deals {} damage",
            d.armor,
            d.magic_resist,
            if magic { "magic" } else { "physical" }
        );
    }
    if magic {
        format!("{short}: magic protection for {name}'s damage profile")
    } else {
        format!("{short}: armor for {name}'s damage profile")
    }
}

fn equipped_value(cat: &Catalog, p: &Player) -> f64 {
    p.items
        .iter()
        .filter(|i| i.slot < 6 || i.slot == crate::roleslot::ROLE_SLOT)
        .filter_map(|i| {
            cat.item(i.id)
                .map(|item| f64::from(item.total) * f64::from(i.count.min(6)))
        })
        .sum()
}

fn item_sum(cat: &Catalog, p: &Player, value: impl Fn(&Item) -> f64) -> f64 {
    p.items
        .iter()
        .filter(|i| i.slot < 6 || i.slot == crate::roleslot::ROLE_SLOT)
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
        let lane = lane_opponents(inp, own_role);
        let boost = lane_boost(inp.live.map(|l| l.game_time));
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
                (Some(p), Some(m)) if m.player.level > 0 && p.level > 0 => (1.0
                    + LEVEL_STEP * (f64::from(p.level) - f64::from(m.player.level)))
                .clamp(0.6, 1.5),
                _ => 1.0,
            };
            // The scoreboard and the kill feed: a fed enemy, and above all one who has been
            // killing you, is the one to itemize against.
            let fed = player.map_or(1.0, |p| {
                1.0 + FED_STEP * (f64::from(p.kills) - f64::from(p.deaths)).clamp(0.0, FED_LEAD_CAP)
            });
            let (hunt, landed) = inp
                .live
                .map_or((0.0, false), |l| hunting(&l.my_deaths, l.game_time, &name));
            // How strong this enemy is without the kill feed: what their evidence is weighed by.
            let strength = relative * levels * fed;
            // Kill-feed dive counts in proportion to strength: a far-behind enemy finishing a
            // teamfight is weaker evidence of being dived than a fed one reaching you.
            n.dive = (n.dive + HUNT_DIVE * hunt * strength.min(1.0)).min(1.0);
            // Pressure and dive are about how strong an enemy is; the damage split also weighs
            // who you face in lane, so the lane opponent names the need early.
            let threat = strength * (1.0 + HUNT_THREAT * hunt);
            strongest_pressure = strongest_pressure.max(threat);
            let weight = if lane.contains(&normalize(&name)) {
                threat * boost
            } else {
                threat
            };
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
            // A champion the hand-written traits do not cover yet (a new release) takes its split
            // from Data Dragon instead of counting as half physical, half magic.
            let prior_magic = match t.map(|t| t.damage.as_str()) {
                Some("ap") => 1.0,
                Some("ad") => 0.0,
                Some(_) => 0.5,
                None => inp
                    .catalog
                    .champion_key(&name)
                    .and_then(|key| inp.catalog.champion(key))
                    .map_or(0.5, |c| c.magic_share_prior()),
            };
            let magic = if equipment_ap + equipment_ad > 0.0 {
                // Item stats adjust, rather than replace, the champion's damage-type prior.
                0.6 * prior_magic + 0.4 * equipment_ap / (equipment_ap + equipment_ad)
            } else {
                prior_magic
            };
            let blame = hunt * strength;
            if blame > 0.0 && n.hunter.as_ref().is_none_or(|h| blame > h.blame) {
                n.hunter = Some(Hunter {
                    champion: name.clone(),
                    kills: player.map_or(0, |p| p.kills),
                    deaths: player.map_or(0, |p| p.deaths),
                    landed,
                    blame,
                    magic,
                });
            }
            n.threats.push((name.clone(), magic, strength));
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
            // How much Master+ players answer this champion with anti-heal (the healing trait only
            // for champions the corpus lacks).
            let heal_weight = crate::answers::weight(Answer::AntiHeal, &name).unwrap_or(
                if t.is_some_and(|t| t.healing) {
                    1.0
                } else {
                    0.0
                },
            );
            if heal_weight > 0.0 {
                let in_lane = player
                    .and_then(|p| crate::aggregate::Position::parse(&p.position))
                    .is_some_and(|r| {
                        Some(r) == own_role
                            || (own_role == Some(crate::aggregate::Position::Adc)
                                && r == crate::aggregate::Position::Support)
                    });
                let contribution = heal_weight * if in_lane { 0.8 } else { 0.55 };
                n.healing = (n.healing + contribution).min(1.0);
                if contribution > strongest_healing {
                    strongest_healing = contribution;
                    n.healing_name = name.clone();
                }
            }
            // Dive and cleanse, like anti-heal, by how Master+ players answer the champion.
            let dive_weight = crate::answers::weight(Answer::AntiBurst, &name).unwrap_or(
                if t.is_some_and(|t| t.assassin || t.burst) {
                    1.0
                } else {
                    0.0
                },
            );
            n.dive = (n.dive + 0.25 * threat * dive_weight).min(1.0);
            let suppression = t.and_then(|t| t.control.as_ref()).filter(|control| {
                control.kind == ControlKind::Suppression
                    && inp
                        .catalog
                        .version
                        .starts_with(&format!("{}.", control.verified_patch))
            });
            let cleanse_weight = crate::answers::weight(Answer::Cleanse, &name)
                .unwrap_or(if suppression.is_some() { 1.0 } else { 0.0 });
            // Mostly ultimates: counted from level 6, as the suppression tag always was.
            if cleanse_weight > n.cleanse && player.is_none_or(|p| p.level >= 6) {
                n.cleanse = cleanse_weight;
                n.cleanse_threat = Some((
                    name.clone(),
                    suppression.map(|control| control.ability.clone()),
                ));
            }
            if let Some(t) = t {
                if t.poke {
                    n.poke = (n.poke + 0.3).min(1.0);
                }
            }
        }
        let total = weighted_physical + weighted_magic;
        if total > 0.0 {
            n.physical_share = weighted_physical / total;
            n.magic_share = weighted_magic / total;
        }
        n.pressure = (strongest_pressure - 1.0).clamp(0.0, 1.0);
        n.owned = owned_ids(me);
        n.defense = Defense::of(inp, me);
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

/// The boots on the path: among the aggregate's boots of the build's family that a real share of
/// players buy, the pick-rate prior against three times the defensive fit, so Mercury's Treads
/// beats the more popular Plated Steelcaps when the enemy's damage is mostly magic.
fn choose_boots(
    inp: &Inputs,
    agg: &crate::aggregate::Aggregate,
    n: &Needs,
    archetype: Archetype,
    base: &PlanItem,
    planned: &[u32],
    preferences: &PlannerPreferences,
) -> PlanItem {
    let cat = inp.catalog;
    let mut best: Option<(f64, u32, String)> = None;
    for line in agg.boots_lines.iter().chain(agg.boots.iter()) {
        let Some(&id) = line.ids.first() else {
            continue;
        };
        let Some(item) = cat.item(id) else { continue };
        if (line.pick_rate < MIN_BOOTS_PICK && id != base.id)
            || !item.effects.boots
            || !coherent_with(archetype, cat, id)
        {
            continue;
        }
        let f = fit(item, inp, n, archetype, planned, preferences.mode);
        // Only the resistance question moves boots off the prior: Master+ ADCs buy Gluttonous Greaves
        // in 23% of games whatever the enemy's poke (0 to 3 poke champions), so sustain is not a boots
        // reason; with dive measured instead of tagged it had started to outscore Berserker's.
        let fit = if matches!(
            f.kind,
            DecisionKind::MagicDefense | DecisionKind::PhysicalDefense
        ) {
            f.score
        } else {
            0.0
        };
        let held = if preferences.last_path.contains(&id) {
            BOOTS_HOLD
        } else {
            0.0
        };
        let score = 2.0 * line.pick_rate.max(0.0).sqrt() + BOOTS_FIT_WEIGHT * fit + held;
        if best.as_ref().is_none_or(|(top, _, _)| score > *top) {
            best = Some((score, id, f.reason));
        }
    }
    match best {
        Some((_, id, reason)) if id != base.id => {
            engine::item_by_id(cat, inp.pack, id, Some(reason)).unwrap_or_else(|| base.clone())
        }
        _ => base.clone(),
    }
}

/// Planned defensive items in the order they add the most effective health per remaining gold
/// against the enemy mix, greedily (each pick's stats count for the next). Only positions held by
/// unowned defensive items are permuted: damage items, boots and the first core item (op.gg's
/// first-item choice, bought for laning as much as for durability) keep their places, and an item
/// moves ahead only by a clear margin (`ORDER_MARGIN`).
fn order_defense(
    path: &mut [PlanItem],
    inp: &Inputs,
    n: &Needs,
    first: Option<u32>,
    me: Option<&Me>,
    locked: bool,
    swiftplay: bool,
) {
    if n.physical_share + n.magic_share <= 0.0 {
        return;
    }
    let cat = inp.catalog;
    let slots: Vec<usize> = path
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            !p.owned
                && p.role != "boots"
                && Some(p.id) != first
                && cat
                    .item(p.id)
                    .is_some_and(|i| i.is_finished(cat) && defensive(i))
        })
        .map(|(index, _)| index)
        .collect();
    if slots.len() < 2 {
        return;
    }
    let mut defense = n.defense.plus(
        cat,
        path[..slots[0]].iter().filter(|p| !p.owned).map(|p| p.id),
    );
    let mut pool: Vec<PlanItem> = slots.iter().map(|&index| path[index].clone()).collect();
    let mut ordered = Vec::with_capacity(pool.len());
    while !pool.is_empty() {
        let values: Vec<f64> = pool
            .iter()
            .map(|p| {
                cat.item(p.id).map_or(0.0, |item| {
                    let cost = remaining(inp, p.id, me, locked, swiftplay)
                        .remaining_cost
                        .unwrap_or(item.total)
                        .max(1);
                    defense.ehp_gain(
                        n,
                        health_of(item),
                        item.effects.armor.unwrap_or(0.0),
                        item.effects.magic_resist.unwrap_or(0.0),
                    ) / f64::from(cost)
                })
            })
            .collect();
        let best =
            (0..values.len()).fold(0, |best, i| if values[i] > values[best] { i } else { best });
        let pick = if values[best] > values[0] * (1.0 + ORDER_MARGIN) {
            best
        } else {
            0
        };
        let item = pool.remove(pick);
        defense = defense.plus(cat, [item.id]);
        ordered.push(item);
    }
    for (slot, item) in slots.into_iter().zip(ordered) {
        path[slot] = item;
    }
}

/// Owned progress toward `answer` that nothing else still to buy explains: a component that builds
/// into it and into no other unowned path item. Null-Magic Mantle toward Mercurial Scimitar counts;
/// a Long Sword that also builds into the core line does not. Returns that component.
fn started(cat: &Catalog, path: &[PlanItem], owned: &[u32], answer: u32) -> Option<u32> {
    if owned.contains(&answer) {
        return None;
    }
    owned.iter().copied().find(|&component| {
        component != answer
            && builds_into(cat, component, answer)
            && !path
                .iter()
                .any(|p| !p.owned && p.id != answer && builds_into(cat, component, p.id))
    })
}

/// An enemy who is even with you or ahead (strength without the kill feed).
const COMMIT_THREAT: f64 = 1.0;

/// Whether some enemy still calls for `item`: its magic resist answers an enemy dealing mostly
/// magic damage, its armor one dealing mostly physical damage, who is even with you or ahead
/// (`COMMIT_THREAT`); a buffer without either answers any such enemy. This is the commitment
/// test instead of the item's need against the whole team, which falls as soon as the player buys
/// a component (Null-Magic Mantle's own magic resist took Mercurial Scimitar's need from 1.05 to
/// 0.91 in the Xayah game) and whenever another enemy's evidence weighs the mix, so a team-wide
/// threshold would release every commitment right after its first purchase.
fn still_answers(n: &Needs, item: &Item) -> bool {
    let magic_resist = item.effects.magic_resist.is_some_and(|v| v > 0.0);
    let armor = item.effects.armor.is_some_and(|v| v > 0.0);
    n.threats.iter().any(|(_, magic, strength)| {
        *strength >= COMMIT_THREAT
            && ((magic_resist && *magic >= 0.5)
                || (armor && *magic < 0.5)
                || (!magic_resist && !armor))
    })
}

/// A committed answer (see `started`) with no current kill-feed evidence: it stays at the front of
/// what is left to buy while `still_answers` holds, with its own reason and what the player
/// already owns of it. Returns it with the current game second, or None when released.
fn keep_commitment(
    path: &mut Vec<PlanItem>,
    inp: &Inputs,
    n: &Needs,
    archetype: Archetype,
    owned: &[u32],
    commitment: Option<(u32, u32)>,
    mode: BuildPreference,
) -> Option<(u32, u32)> {
    let (id, component) = commitment?;
    let cat = inp.catalog;
    let item = cat.item(id)?;
    if !still_answers(n, item) {
        return None;
    }
    let now = inp.live.map_or(0.0, |l| l.game_time.max(0.0));
    let f = fit(item, inp, n, archetype, owned, mode);
    let mut entry = match path.iter().position(|p| p.id == id && !p.owned) {
        Some(index) => path.remove(index),
        None => {
            if path.len() >= 6 {
                // Make room with the last flexible item the player has not started.
                let index = path.iter().rposition(|p| {
                    !p.owned && p.role != "boots" && started(cat, path, owned, p.id).is_none()
                })?;
                path.remove(index);
            }
            engine::item_by_id(cat, inp.pack, id, None)?
        }
    };
    entry.why = Some(format!(
        "{}; you already own {}",
        f.reason,
        cat.item(component)
            .map(|i| i.name.clone())
            .unwrap_or_default()
    ));
    entry.tag = Some("situational".into());
    let front = path.iter().position(|p| !p.owned).unwrap_or(path.len());
    path.insert(front, entry);
    Some((id, now as u32))
}

/// With kill-feed evidence (an enemy has been killing you), a defensive answer moves to the front
/// of what is left to buy, ahead of the next core item, once the first core item is finished. Two
/// kinds of answer qualify: resistance against the blamed enemy's damage type (magic resist against
/// a mage) at `TYPED_ANSWER_NEED`, because the kill feed confirms that need, and a buffer the kill
/// feed calls for (stasis, a spell shield, a shield) at `DETOUR_NEED`. Buffers come from the planned
/// path; a resistance answer may also come from the champion's candidate pool (`pool`), taking the
/// place of the weakest flexible path item (never a core item or boots), because the path can be
/// full of core items and boots when the kill feed calls for it. The resistance answer goes first
/// unless the buffer's need is `TYPED_MARGIN` higher. An answer already promoted stays while its
/// need is above `PROMOTE_KEEP` or for `PROMOTE_HOLD_SECONDS` after it last met its bar, and gives
/// way only to a resistance answer, so the front of the path does not flip as deaths fade and
/// renew. An answer the player has started buying (`started`) is committed: it stays in front until
/// finished while its need holds at `PROMOTE_KEEP`, unless another answer's need is `COMMIT_MARGIN`
/// higher, and a qualifying answer for another enemy goes right after it instead of replacing it.
/// Nothing with owned progress is ever the item a pool answer replaces. Returns the promoted item
/// and the game second it last met its bar.
#[allow(clippy::too_many_arguments)]
fn promote_answer(
    path: &mut Vec<PlanItem>,
    pool: &[u32],
    core: &[u32],
    inp: &Inputs,
    n: &Needs,
    archetype: Archetype,
    owned: &[u32],
    completed_core: usize,
    preferences: &PlannerPreferences,
) -> Option<(u32, u32)> {
    struct Candidate {
        id: u32,
        slot: Option<usize>,
        score: f64,
        typed: bool,
        met: bool,
        committed: bool,
        fit: Fit,
    }
    if completed_core == 0 {
        return None;
    }
    let cat = inp.catalog;
    // The promoted answer the player has started buying, and the component that shows it.
    let commitment = preferences
        .promoted
        .and_then(|id| started(cat, path, owned, id).map(|component| (id, component)));
    let committed_id = commitment.map(|(id, _)| id);
    let Some(hunter) = n.hunter.as_ref() else {
        // The kill-feed evidence has faded (six minutes after the last death), but an answer the
        // player has started buying stays in front while some enemy still calls for it.
        return keep_commitment(path, inp, n, archetype, owned, commitment, preferences.mode);
    };
    let now = inp.live.map_or(0.0, |l| l.game_time.max(0.0));
    let front = path.iter().position(|p| !p.owned)?;
    let magic_hunter = hunter.magic >= 0.5;
    let need = |id: u32| {
        cat.item(id).map_or(0.0, |item| {
            fit(item, inp, n, archetype, owned, preferences.mode).score
        })
    };
    // The flexible path item a pool answer would replace when the path is full: the unowned
    // non-core item (not boots) with the least need, never the committed answer or an item the
    // player has started (`started`: a component only that item explains, so dropping it would
    // strand the gold). A component shared with another item still to buy (a Long Sword toward
    // both Mortal Reminder and Mercurial Scimitar) strands nothing and does not protect it.
    let snapshot: Vec<PlanItem> = path.clone();
    let replaceable = |p: &PlanItem| {
        !p.owned
            && p.role != "boots"
            && !core.contains(&p.id)
            && Some(p.id) != committed_id
            && started(cat, &snapshot, owned, p.id).is_none()
    };
    let weakest = path
        .iter()
        .enumerate()
        .filter(|(_, p)| replaceable(p))
        .map(|(index, p)| (need(p.id), index))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, index)| index);
    let mut options: Vec<(u32, Option<usize>)> = path
        .iter()
        .enumerate()
        .skip(front)
        .filter(|(_, p)| !p.owned && p.role != "boots")
        .map(|(index, p)| (p.id, Some(index)))
        .collect();
    if path.len() < 6 || weakest.is_some() {
        options.extend(pool.iter().map(|&id| (id, None)));
    }
    let mut candidates = Vec::new();
    for (id, slot) in options {
        let Some(item) = cat.item(id) else { continue };
        if !item.is_finished(cat) {
            continue;
        }
        let f = fit(item, inp, n, archetype, owned, preferences.mode);
        let typed = if magic_hunter {
            item.effects.magic_resist.is_some_and(|v| v > 0.0)
        } else {
            item.effects.armor.is_some_and(|v| v > 0.0)
        };
        let buffer = f.evidence == Evidence::KillFeed && slot.is_some();
        // The answer already promoted stays a candidate through its hold even when a different
        // enemy now has the most evidence (a later kill by another enemy while the first one's
        // deaths fade must not drop Mercurial Scimitar for one poll and bring it back the next).
        let held = preferences.promoted == Some(id)
            && preferences
                .promoted_seen
                .is_some_and(|seen| now - f64::from(seen) <= PROMOTE_HOLD_SECONDS);
        let committed = committed_id == Some(id);
        if !typed && !buffer && !held && !committed {
            continue;
        }
        let met = (typed || buffer)
            && f.score
                >= if typed {
                    TYPED_ANSWER_NEED
                } else {
                    DETOUR_NEED
                }
            || preferences.promoted == Some(id) && f.score >= PROMOTE_KEEP;
        if met || held || committed {
            candidates.push(Candidate {
                id,
                slot,
                score: f.score,
                typed,
                met,
                committed,
                fit: f,
            });
        }
    }
    let best = |typed: bool| {
        candidates
            .iter()
            .filter(|c| c.typed == typed)
            .max_by(|a, b| a.score.total_cmp(&b.score))
    };
    let mut choice = match (best(true), best(false)) {
        (Some(typed), Some(buffer)) if buffer.score < typed.score + TYPED_MARGIN => typed,
        (_, Some(buffer)) => buffer,
        (Some(typed), None) => typed,
        (None, None) => return None,
    };
    if let Some(previous) = candidates
        .iter()
        .find(|c| preferences.promoted == Some(c.id))
    {
        if previous.typed || !choice.typed {
            choice = previous;
        }
    }
    // Commitment: the answer the player has started buying stays in front while it is still
    // needed, unless another answer is clearly more needed; that other answer, if it qualified on
    // its own, follows it.
    let mut second = None;
    if let Some(committed) = candidates.iter().find(|c| c.committed) {
        let outclassed =
            choice.id != committed.id && choice.score >= committed.score + COMMIT_MARGIN;
        let answers = cat
            .item(committed.id)
            .is_some_and(|item| still_answers(n, item));
        if answers && !outclassed {
            if choice.id != committed.id && choice.met {
                second = Some(choice);
            }
            choice = committed;
        }
    }
    let seen = if choice.met || choice.committed {
        now as u32
    } else {
        preferences.promoted_seen.unwrap_or(now as u32)
    };
    let short = cat
        .item(choice.id)
        .map(|item| engine::short_of(inp.pack, &item.name))
        .unwrap_or_default();
    // A buffer's own reason already names the evidence ("stasis stops the all-in"); a resistance
    // answer says what it cuts; an answer kept only by its hold keeps its own reason rather than
    // claiming to answer an enemy whose damage it does not resist, and a committed one says what
    // the player already owns of it.
    let reason_for = |candidate: &Candidate, short: &str| -> String {
        if candidate.fit.evidence == Evidence::KillFeed || !candidate.typed {
            let own = candidate.fit.reason.clone();
            match commitment {
                Some((id, component)) if id == candidate.id => format!(
                    "{own}; you already own {}",
                    cat.item(component)
                        .map(|i| i.name.clone())
                        .unwrap_or_default()
                ),
                _ => own,
            }
        } else {
            format!(
                "{short}: {} ({}/{}) {} you; its {} cuts that damage",
                hunter.champion,
                hunter.kills,
                hunter.deaths,
                if hunter.landed {
                    "killed"
                } else {
                    "helped kill"
                },
                if magic_hunter {
                    "magic resist"
                } else {
                    "armor"
                }
            )
        }
    };
    let reason = reason_for(choice, &short);
    let mut item = match choice.slot {
        Some(index) => path.remove(index),
        None => {
            let entry = engine::item_by_id(cat, inp.pack, choice.id, None)?;
            if path.len() >= 6 {
                path.remove(weakest?);
            }
            entry
        }
    };
    item.why = Some(reason);
    item.tag = Some("situational".into());
    let front = path.iter().position(|p| !p.owned).unwrap_or(path.len());
    path.insert(front, item);
    let promoted = path[front].id;
    // The answer for another enemy follows the committed one rather than replacing it.
    if let Some(second) = second {
        let short = cat
            .item(second.id)
            .map(|item| engine::short_of(inp.pack, &item.name))
            .unwrap_or_default();
        let reason = reason_for(second, &short);
        let entry = match path.iter().position(|p| p.id == second.id && !p.owned) {
            Some(index) => Some(path.remove(index)),
            None => {
                let room = path.len() < 6
                    || path
                        .iter()
                        .enumerate()
                        .filter(|(_, p)| replaceable(p) && p.id != promoted)
                        .map(|(index, p)| (need(p.id), index))
                        .min_by(|a, b| a.0.total_cmp(&b.0))
                        .map(|(_, index)| {
                            path.remove(index);
                        })
                        .is_some();
                if room {
                    engine::item_by_id(cat, inp.pack, second.id, None)
                } else {
                    None
                }
            }
        };
        if let Some(mut entry) = entry {
            entry.why = Some(reason);
            entry.tag = Some("situational".into());
            let after = path
                .iter()
                .position(|p| p.id == promoted)
                .map_or(path.len(), |index| index + 1);
            path.insert(after, entry);
        }
    }
    Some((promoted, seen))
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
        if let Some((champ, ability)) = &n.cleanse_threat {
            let reason = match ability {
                Some(ability) => format!("{short}: its active removes {champ}'s {ability} suppression"),
                None => format!(
                    "{short}: its active removes {champ}'s crowd control; Master+ players buy one {:.0}x as often against {champ}",
                    crate::answers::odds(Answer::Cleanse, champ).unwrap_or(1.0)
                ),
            };
            terms.push((
                3.2 * n.cleanse,
                DecisionKind::Cleanse,
                reason,
                Evidence::Composition,
            ));
        }
    }
    let defense_weight = if pref == BuildPreference::Survival {
        2.8
    } else {
        0.9 + 0.9 * n.pressure
    };
    // Resistances are valued as the share of incoming damage they remove on top of what the
    // player already has (measured in game) plus the planned items not bought yet, so a third
    // armor item on 259 armor and 50 MR is worth little and magic resist a lot.
    let defense = n.defense.plus(inp.catalog, unbought(already, &n.owned));
    // Quicksilver Sash's magic resistance is a side stat of a component bought for its cleanse, so
    // it is scored as a cleanse only, never sold as "magic protection". A finished cleanse item's
    // resistance is real: Mercurial Scimitar is the magic-resist item marksmen buy against mages
    // (Xayah's only one above the late-item floor), so it counts like any other.
    if let Some(mr) = e
        .magic_resist
        .filter(|v| *v > 0.0 && (e.cleanse.is_none() || item.is_finished(inp.catalog)))
    {
        terms.push((
            defense_weight * RESIST_SCALE * defense.removed(n, 0.0, mr),
            DecisionKind::MagicDefense,
            resist_reason(&short, n, true),
            Evidence::Composition,
        ));
    }
    if let Some(armor) = e.armor.filter(|v| *v > 0.0) {
        terms.push((
            defense_weight * RESIST_SCALE * defense.removed(n, armor, 0.0),
            DecisionKind::PhysicalDefense,
            resist_reason(&short, n, false),
            Evidence::Composition,
        ));
    }
    if e.shield.is_some() || e.spell_shield || e.stasis {
        let relevance = if e.shield == Some(ShieldEffect::Magic) {
            n.magic_share
        } else {
            1.0
        };
        // When the kill feed shows who has been killing you, the reason says so: that is the
        // evidence a player can check, and it is what the buffer is for.
        let (reason, evidence) = match &n.hunter {
            Some(h) => {
                let answer = if e.stasis {
                    "stasis stops the all-in"
                } else if e.spell_shield {
                    "its spell shield blocks the engage"
                } else {
                    "its shield absorbs the burst"
                };
                (
                    format!(
                        "{short}: {} ({}/{}) {} you; {answer}",
                        h.champion,
                        h.kills,
                        h.deaths,
                        if h.landed { "killed" } else { "helped kill" }
                    ),
                    Evidence::KillFeed,
                )
            }
            None => (
                format!("{short}: a defensive buffer against their burst threats"),
                Evidence::Composition,
            ),
        };
        terms.push((
            defense_weight * relevance * n.dive,
            DecisionKind::AntiBurst,
            reason,
            evidence,
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
    // Kill-feed evidence names the reason whenever it is a real part of the need, even when the
    // item's resistance term is larger: "Darius killed you" is why the item is worth it now.
    if let Some(index) = terms
        .iter()
        .position(|t| t.3 == Evidence::KillFeed && t.0 >= 0.3)
    {
        let evidenced = terms.remove(index);
        terms.insert(0, evidenced);
    }
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
/// the game. Consumables and trinkets are not an answer; neither is a component that builds only
/// into the detour. A component shared with the planned path (a Long Sword when both an
/// Executioner's Calling and a Black Cleaver want one) is an answer: it was bought for the path.
fn note_declined_detour(
    cat: &Catalog,
    ids: &[u32],
    pending: &[u32],
    preferences: &mut PlannerPreferences,
) {
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
            let progress = builds_into(cat, id, detour)
                && !pending.iter().any(|&planned| builds_into(cat, id, planned));
            !item
                .tags
                .iter()
                .any(|t| t == "Consumable" || t == "Trinket")
                && !progress
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
        .filter(|i| i.slot < 6 || i.slot == crate::roleslot::ROLE_SLOT)
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
            (i.slot < 6 || i.slot == crate::roleslot::ROLE_SLOT)
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
    // boots stay regardless of their share. Items from the other damage family (the AP crowd's
    // Zhonya's and Shadowflame behind Katarina's on-hit core line, whether they come from late
    // items, boots or another core line) are not candidates either: a build is one family.
    let archetype = Archetype::from_build(inp);
    let fits = |id: u32| a.core.ids.contains(&id) || coherent_with(archetype, inp.catalog, id);
    for line in a
        .late
        .iter()
        .filter(|line| line.pick_rate >= MIN_LATE_PICK)
        .chain(a.core_lines.iter())
        .chain(std::iter::once(&a.core))
        .chain(a.boots_lines.iter())
        .chain(a.boots.iter())
    {
        for &id in &line.ids {
            if !fits(id) {
                continue;
            }
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
/// Enemy champions known now: the live scoreboard in game, the lobby before it.
fn enemy_names(inp: &Inputs) -> Vec<String> {
    let mut names: Vec<String> = inp.enemies.to_vec();
    if let Some(live) = inp.live {
        for p in &live.enemies {
            if !names.iter().any(|n| normalize(n) == normalize(&p.champion)) {
                names.push(p.champion.clone());
            }
        }
    }
    names
}

/// The v3 backbone: greedily chain the Master+ next-legendary distribution from the owned legendary
/// items, each step scored as ln P(item | champion, role, owned + planned) plus a bounded need nudge,
/// among items compatible with what is owned and planned. None when the corpus does not cover the
/// champion and role. Returns (item, probability) per planned step.
#[allow(clippy::too_many_arguments)]
fn prior_chain(
    inp: &Inputs,
    key: u32,
    role: crate::aggregate::Position,
    owned_legendaries: &[u32],
    owned: &[u32],
    needs: &Needs,
    archetype: Archetype,
    mode: BuildPreference,
    compatible: &dyn Fn(u32, &[u32]) -> bool,
    comp: nextprior::Comp,
    last_path: &[u32],
) -> Option<Vec<(u32, f64)>> {
    let cat = inp.catalog;
    let mut legendaries = owned_legendaries.to_vec();
    let mut planned = owned.to_vec();
    let mut chain = Vec::new();
    while legendaries.len() < V3_LEGENDARIES {
        let dist = nextprior::distribution(key, role, &legendaries, Some(comp))?;
        let scored: Vec<(u32, f64, f64)> = dist
            .iter()
            .take(12)
            .filter(|(id, _)| !planned.contains(id) && compatible(*id, &planned))
            .filter_map(|&(id, p)| {
                let item = cat.item(id)?;
                let need = fit(item, inp, needs, archetype, &planned, mode).score;
                Some((id, p, p.ln() + V3_NUDGE * need.clamp(0.0, V3_NEED_CAP)))
            })
            .collect();
        let Some(top) = scored.iter().map(|s| s.2).max_by(f64::total_cmp) else {
            break;
        };
        // Among near-ties, the item the previous plan listed first keeps its place.
        let Some(&(id, p, _)) = scored
            .iter()
            .filter(|s| s.2 >= top - V3_TIE_MARGIN)
            .min_by_key(|s| {
                (
                    last_path
                        .iter()
                        .position(|x| *x == s.0)
                        .unwrap_or(usize::MAX),
                    if s.2 == top { 0 } else { 1 },
                    s.0,
                )
            })
        else {
            break;
        };
        chain.push((id, p));
        legendaries.push(id);
        planned.push(id);
    }
    Some(chain)
}

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
    let mut choices = pool(inp);
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
    // Engine v3: what Master+ players on this champion and role buy next with the items owned. Swiftplay
    // (another shop) and champions the corpus does not cover keep the op.gg build below.
    let owned_now: Vec<u32> = path.iter().map(|p| p.id).collect();
    let owned_legendaries: Vec<u32> = owned_now
        .iter()
        .copied()
        .filter(|&id| nextprior::is_legendary(cat, id))
        .collect();
    let v3_chain = (!swiftplay)
        .then(|| cat.champion_key(inp.champion))
        .flatten()
        .and_then(|key| {
            prior_chain(
                inp,
                key,
                engine::actual_position(agg),
                &owned_legendaries,
                &owned_now,
                &needs,
                archetype,
                preferences.mode,
                &compatible,
                nextprior::Comp::of(inp.traits, &enemy_names(inp)),
                &preferences.last_path,
            )
        })
        .filter(|chain| !chain.is_empty() || owned_legendaries.len() >= V3_LEGENDARIES);
    let v3 = v3_chain.is_some();
    // The learned path is itself a candidate source. An older provider response can
    // omit its next item; that must not make the purchase scorer skip the learned choice.
    if let Some(chain) = &v3_chain {
        for &(id, probability) in chain {
            choices.entry(id).or_insert(probability);
        }
    }
    let v3_core: Vec<u32>;
    let (core_ids, base) = match &v3_chain {
        Some(chain) => {
            v3_core = owned_legendaries
                .iter()
                .copied()
                .chain(chain.iter().map(|(id, _)| *id))
                .collect();
            let mut planned: Vec<PlanItem> = chain
                .iter()
                .filter_map(|&(id, p)| {
                    let short = engine::short_of(inp.pack, &cat.item(id)?.name);
                    engine::item_by_id(
                        cat,
                        inp.pack,
                        id,
                        Some(format!(
                            "{short}: {:.0}% of Master+ {} players buy it at this point",
                            100.0 * p,
                            inp.champion
                        )),
                    )
                })
                .collect();
            // Boots right after the first legendary: Master+ players finish tier-2 boots at a
            // median minute 13, just after their first item.
            let at = usize::from(owned_legendaries.is_empty()).min(planned.len());
            for (k, boots) in base.iter().filter(|b| b.role == "boots").enumerate() {
                planned.insert((at + k).min(planned.len()), boots.clone());
            }
            (&v3_core, planned)
        }
        None => (&agg.core.ids, base),
    };
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
        let planned: Vec<u32> = path.iter().map(|p| p.id).collect();
        let item = if item.role == "boots" {
            choose_boots(inp, agg, &needs, archetype, item, &planned, preferences)
        } else {
            item.clone()
        };
        if fulfilled(inp, item.id, me)
            || (Some(item.id) == first && alternative_first_owned)
            || !compatible(item.id, &ids)
            || !compatible(item.id, &planned)
        {
            continue;
        }
        path.push(item);
    }
    // An answer promoted because an enemy has been killing you keeps its place on the path while
    // it stays promoted (`promote_answer` decides that): a component bought for another item must
    // not push it out of the tail and hand its place to a weaker answer (the Xayah game: a B. F.
    // Sword pulled Bloodthirster in and Mercurial Scimitar out a minute after Orianna's kill).
    if let Some(id) = preferences.promoted {
        let planned: Vec<u32> = path.iter().map(|p| p.id).collect();
        let keeps_place = path.len() < 6
            && !planned.contains(&id)
            && choices.contains_key(&id)
            && !fulfilled(inp, id, me)
            && compatible(id, &ids)
            && compatible(id, &planned);
        if let Some(item) = cat
            .item(id)
            .filter(|item| keeps_place && item.is_finished(cat))
        {
            let f = fit(item, inp, &needs, archetype, &planned, preferences.mode);
            if let Some(entry) = engine::item_by_id(cat, inp.pack, id, Some(f.reason)) {
                path.push(entry);
            }
        }
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
        let Some(best) = ranked.first().map(|r| r.0) else {
            break;
        };
        // Hysteresis: among candidates within `TAIL_MARGIN` of the best, the one the previous plan
        // listed first keeps its place, so near-ties (Banshee's Veil and Zhonya's, Banshee's and
        // Deathcap in the Lux game) do not swap every time a level or a component ticks.
        let chosen = ranked
            .iter()
            .enumerate()
            .filter(|(_, r)| r.0 >= best - TAIL_MARGIN)
            .filter_map(|(i, r)| {
                preferences
                    .last_path
                    .iter()
                    .position(|p| *p == r.1)
                    .map(|previous| (previous, i))
            })
            .min()
            .map_or(0, |(_, i)| i);
        let (_, id, f) = ranked.remove(chosen);
        if let Some(mut item) = engine::item_by_id(cat, inp.pack, id, Some(f.reason)) {
            // A named need is worth a tag at any real score; the generic label needs a clear one,
            // so a Guardian Angel hovering around the threshold does not flicker between polls.
            // A tag shown last poll stays until the score falls `TAG_MARGIN` below the threshold.
            let named = matches!(
                f.kind,
                DecisionKind::AntiHeal
                    | DecisionKind::Cleanse
                    | DecisionKind::ArmorPen
                    | DecisionKind::MagicPen
            );
            let threshold = if named { 0.2 } else { 0.75 };
            let tagged_before = preferences.last_tags.iter().any(|(t, _)| *t == id);
            let tagged = if tagged_before {
                f.score > threshold - TAG_MARGIN
            } else if named {
                f.score > threshold
            } else {
                f.score >= threshold
            };
            if tagged {
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
    order_defense(&mut path, inp, &needs, first, me, boots_locked, swiftplay);
    // Finished items from the champion's candidate pool that are not on the path, can be bought
    // with this inventory and were not declined: where a promoted resistance answer may come from.
    let on_path: Vec<u32> = path.iter().map(|p| p.id).collect();
    let pool_answers: Vec<u32> = choices
        .keys()
        .copied()
        .filter(|&id| {
            !on_path.contains(&id)
                && !fulfilled(inp, id, me)
                && compatible(id, &ids)
                && compatible(id, &on_path)
                && !out.preferences.declined_detours.contains(&id)
                && cat
                    .item(id)
                    .is_some_and(|i| i.is_finished(cat) && !i.effects.boots)
        })
        .collect();
    // v3: the corpus shows no defensive reaction to deaths (Master+ defensive share -1.1 pp after
    // recent deaths), so kill-feed promotions are off where the backbone applies.
    let promoted = if v3 {
        None
    } else {
        promote_answer(
            &mut path,
            &pool_answers,
            core_ids,
            inp,
            &needs,
            archetype,
            &ids,
            completed_core,
            preferences,
        )
    };
    out.preferences.promoted = promoted.map(|(id, _)| id);
    out.preferences.promoted_seen = promoted.map(|(_, seen)| seen);
    let pending: Vec<_> = path
        .iter()
        .filter(|p| !p.owned && !(boots_locked && p.role == "boots"))
        .map(|p| p.id)
        .collect();
    let baseline = pending.first().copied();
    note_declined_detour(cat, &ids, &pending, &mut out.preferences);
    let baseline_quote = baseline.map(|id| remaining(inp, id, me, boots_locked, swiftplay));
    let baseline_cost = baseline_quote
        .as_ref()
        .and_then(|q| q.remaining_cost)
        .unwrap_or(1)
        .max(1);
    // Boots are not a reason to stall the first core item: while nothing is finished and the
    // core item's next component is buyable, affordable boots do not get the finish-now bonus
    // (Swiftplay's 1400-gold start otherwise opens with Sorcerer's Shoes over a Lost Chapter).
    let core_can_progress = baseline_quote
        .as_ref()
        .is_some_and(|q| q.buy_now.is_some() && q.blocked.is_none());
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
        // A detour the player answered with another purchase is never the target again this
        // game; it stays on the path and among the options.
        if !eligible || out.preferences.declined_detours.contains(&id) {
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
        let planned = Some(id) == baseline || pending.contains(&id) || f.score >= DETOUR_NEED;
        let boots_too_early =
            item.effects.boots && Some(id) != baseline && completed_core == 0 && core_can_progress;
        let completion = OWNED_CREDIT * credit
            + if q.affordable && planned && !boots_too_early {
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
        let situation = if v3 {
            (f.score * phase).min(V3_SITUATION_CAP)
        } else {
            f.score * phase
        };
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
    // A target that was already shown and can still be finished right now stays the target.
    // Without this, two finishable path items trade places every time gold crosses one of
    // their prices (Morellonomicon and Rylai's in the recorded Morgana game).
    if out.preferences.pinned_item.is_none() {
        if let Some(last) = preferences.last_target {
            if let Some(index) = ranked.iter().position(|r| r.0.id == last) {
                // Finishable right now and still planned, or within `TARGET_MARGIN` of the best:
                // near-ties (Bloodthirster and Mortal Reminder in the Xayah game) keep the target
                // the player has been shown; a clearly better target still takes over.
                // A promoted answer is evidence-driven, not a near-tie: it takes over unless the old
                // target can be finished right now.
                let finishable = ranked[index].2.affordable && pending.contains(&last);
                let promoted_first = out.preferences.promoted == Some(ranked[0].0.id);
                let close =
                    !promoted_first && ranked[0].0.total - ranked[index].0.total < TARGET_MARGIN;
                let keep = index > 0 && ranked[index].2.blocked.is_none() && (finishable || close);
                if keep {
                    let chosen = ranked.remove(index);
                    ranked.insert(0, chosen);
                }
            }
        }
        // Otherwise a near-tie goes to the answer the kill feed calls for: with Infinity Edge
        // just bought, Mortal Reminder edged the promoted Mercurial Scimitar by 0.04 in the Xayah
        // game and the target flipped until the next death.
        let kept_last = preferences.last_target.is_some()
            && ranked.first().map(|r| r.0.id) == preferences.last_target;
        if let Some(promoted) = out.preferences.promoted.filter(|_| !kept_last) {
            if let Some(index) = ranked.iter().position(|r| r.0.id == promoted) {
                if index > 0
                    && ranked[index].2.blocked.is_none()
                    && ranked[0].0.total - ranked[index].0.total < TARGET_MARGIN
                {
                    let chosen = ranked.remove(index);
                    ranked.insert(0, chosen);
                }
            }
        }
    }
    out.preferences.last_target = ranked.first().map(|r| r.0.id);
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
            } else if let Some(why) = out
                .preferences
                .promoted
                .filter(|id| *id == score.id)
                .and_then(|id| path.iter().find(|p| p.id == id && !p.owned))
                .and_then(|p| p.why.clone())
            {
                // The promoted answer says why it jumped ahead: who has been killing you.
                (f.kind, why, Evidence::KillFeed)
            } else if let Some(why) = (v3 && f.score < 1.0)
                .then(|| path.iter().find(|p| p.id == target.id && !p.owned))
                .flatten()
                .and_then(|p| p.why.clone())
                .filter(|why| why.contains("Master+"))
            {
                // v3: the data is the reason; a strong situational need still speaks for itself below.
                (DecisionKind::Core, why, Evidence::Aggregate)
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
            } else {
                // Becoming the target does not change what the item is for: it keeps the tag its
                // path entry carried, so the tag does not blink off and on with target changes.
                target.tag = path
                    .iter()
                    .find(|p| p.id == target.id && !p.owned)
                    .and_then(|p| p.tag.clone());
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
    // What the next poll compares against (tail order and tags, see `TAIL_MARGIN`).
    out.preferences.last_path = path.iter().map(|p| p.id).collect();
    out.preferences.last_tags = path
        .iter()
        .filter_map(|p| p.tag.clone().map(|tag| (p.id, tag)))
        .collect();
    out.path = path;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn needs(physical: f64, magic: f64) -> Needs {
        Needs {
            physical_share: physical,
            magic_share: magic,
            ..Default::default()
        }
    }

    #[test]
    fn a_resistance_is_worth_more_where_less_of_it_is_owned() {
        let d = Defense {
            armor: 259.0,
            magic_resist: 50.0,
            health: 2614.0,
            observed: true,
        };
        let n = needs(0.57, 0.43);
        let armor = d.removed(&n, 75.0, 0.0);
        let mr = d.removed(&n, 0.0, 80.0);
        assert!(mr > 2.0 * armor, "armor {armor}, mr {mr}");
        // Against an all-physical team magic resist removes nothing.
        assert_eq!(d.removed(&needs(1.0, 0.0), 0.0, 80.0), 0.0);
        // Effective health: 400 health and 80 MR outweigh 150 health and 75 armor here.
        assert!(d.ehp_gain(&n, 400.0, 0.0, 80.0) > 2.0 * d.ehp_gain(&n, 150.0, 75.0, 0.0));
        assert_eq!(d.ehp_gain(&needs(0.0, 0.0), 400.0, 0.0, 80.0), 0.0);
    }

    #[test]
    fn early_single_resist_items_keep_the_old_score_scale() {
        // About 40 of either at 60 armor and 40 MR against an even mix scored 0.5 before; the new
        // scale keeps a first resist item near that so other needs keep their relative weight.
        let d = Defense {
            armor: 60.0,
            magic_resist: 40.0,
            health: 1500.0,
            observed: false,
        };
        let n = needs(0.5, 0.5);
        for score in [
            RESIST_SCALE * d.removed(&n, 40.0, 0.0),
            RESIST_SCALE * d.removed(&n, 0.0, 40.0),
        ] {
            assert!((0.35..0.65).contains(&score), "{score}");
        }
    }

    #[test]
    fn the_lane_opponent_counts_triple_until_ten_minutes_then_fades_by_twenty() {
        assert_eq!(lane_boost(None), 3.0);
        assert_eq!(lane_boost(Some(0.0)), 3.0);
        assert_eq!(lane_boost(Some(600.0)), 3.0);
        assert!((lane_boost(Some(900.0)) - 2.0).abs() < 1e-9);
        assert_eq!(lane_boost(Some(1200.0)), 1.0);
        assert_eq!(lane_boost(Some(2400.0)), 1.0);
    }

    #[test]
    fn planned_items_are_the_ones_not_bought_yet_counting_units() {
        assert_eq!(
            unbought(&[1029, 1029, 3075, 3068], &[1029, 3068]),
            [1029, 3075]
        );
        assert_eq!(unbought(&[3068], &[3068, 3068]), Vec::<u32>::new());
        assert_eq!(unbought(&[], &[3068]), Vec::<u32>::new());
    }
}
