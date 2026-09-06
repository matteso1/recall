//! Deterministic, role-aware purchase planning. Champion loadouts come from their
//! own aggregate; shared item effects and actual inventory drive adaptations.
//! No I/O, enemy wallets, cooldowns, or model-generated decisions live here.
use crate::aggregate::{Aggregate, Position, RunePageIds};
use crate::coaching::{self, DecisionKind, Evidence, LearningTip};
use crate::ddragon::{normalize, Catalog};
use crate::decision::{self, CandidateScore};
use crate::live::{LiveSnapshot, Me};
use crate::pack::{ChampionPack, SkillOrder, Traits};
use crate::{runes, shop, statistics};
use serde::{Deserialize, Serialize};

pub const ENGINE_VERSION: &str = "2.0";
const PATH_LEN: usize = 6;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct PlanItem {
    pub id: u32,
    pub name: String,
    pub short: String,
    pub cost: u32,
    pub owned: bool,
    pub role: String,
    pub why: Option<String>,
    pub tag: Option<String>,
}

pub type Component = shop::ShopComponent;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct NextItem {
    pub id: u32,
    pub name: String,
    pub cost: u32,
    pub remaining_cost: u32,
    pub price_known: bool,
    pub components: Vec<Component>,
    pub buy_now: Option<Component>,
    pub buy_now_affordable: bool,
    pub basket: Vec<Component>,
    pub basket_cost: u32,
    pub save_gap: Option<u32>,
    pub blocked: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct SkillPlan {
    pub next: Option<char>,
    pub point_available: bool,
    pub label: String,
    pub levels: [u8; 4],
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct EnemyProfile {
    pub names: Vec<String>,
    pub healers: Vec<String>,
    pub tanks: Vec<String>,
    pub lockdown: Vec<String>,
    pub assassins: Vec<String>,
    pub poke: Vec<String>,
    /// Display-only summaries. These votes never select a defensive item.
    pub ap: u32,
    pub ad: u32,
    pub unknown: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BuildPreference {
    #[default]
    Balanced,
    Survival,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct PlannerPreferences {
    pub mode: BuildPreference,
    pub pinned_item: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Alternative {
    pub item: PlanItem,
    pub remaining_cost: Option<u32>,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct CoreEvidence {
    pub games: u32,
    pub wins: u32,
    pub win_rate: f64,
    pub interval: Option<statistics::Interval>,
    pub comparison_games: Option<u32>,
    /// Most-played alternative minus baseline; not an estimated treatment effect.
    pub difference: Option<statistics::Interval>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Plan {
    pub champion: String,
    pub position: Option<String>,
    pub source: Option<String>,
    pub note: Option<String>,
    pub start: Vec<PlanItem>,
    pub path: Vec<PlanItem>,
    pub options: Vec<PlanItem>,
    pub next: Option<NextItem>,
    pub skill: SkillPlan,
    pub why: Vec<String>,
    pub matchup: Option<String>,
    pub matchup_champion: Option<String>,
    pub runes: Option<RunePageIds>,
    pub runes_summary: String,
    pub spells: Vec<String>,
    pub spell_ids: Vec<u32>,
    pub enemy: EnemyProfile,
    pub learning: Option<LearningTip>,
    pub alternative: Option<Alternative>,
    pub preferences: PlannerPreferences,
    pub context: Vec<String>,
    pub warnings: Vec<String>,
    pub core_evidence: Option<CoreEvidence>,
    /// Auditable policy scores, not probabilities. Kept out of the compact UI.
    pub score_trace: Vec<CandidateScore>,
}

#[derive(Clone, Copy)]
pub struct Inputs<'a> {
    pub champion: &'a str,
    pub pack: Option<&'a ChampionPack>,
    pub aggregate: Option<&'a Aggregate>,
    pub traits: &'a Traits,
    pub catalog: &'a Catalog,
    pub enemies: &'a [String],
    pub live: Option<&'a LiveSnapshot>,
}
const SHORTS: &[(&str, &str)] = &[
    ("Infinity Edge", "IE"),
    ("Essence Reaver", "ER"),
    ("Lord Dominik's Regards", "LDR"),
    ("Guardian Angel", "GA"),
    ("Bloodthirster", "BT"),
    ("Phantom Dancer", "PD"),
    ("Rapid Firecannon", "RFC"),
    ("Statikk Shiv", "Shiv"),
    ("Kraken Slayer", "Kraken"),
    ("The Collector", "Collector"),
    ("Navori Flickerblade", "Navori"),
    ("Yun Tal Wildarrows", "Yun Tal"),
    ("Berserker's Greaves", "Greaves"),
    ("Mortal Reminder", "Mortal"),
    ("Mercurial Scimitar", "Merc"),
    ("Maw of Malmortius", "Maw"),
    ("Immortal Shieldbow", "Shieldbow"),
    ("Runaan's Hurricane", "Hurricane"),
    ("Blade of The Ruined King", "BotRK"),
    ("Plated Steelcaps", "Steelcaps"),
    ("Mercury's Treads", "Mercs"),
    ("Boots of Swiftness", "Swifties"),
    ("Sorcerer's Shoes", "Sorcs"),
    ("Ionian Boots of Lucidity", "Lucidity"),
    ("Rabadon's Deathcap", "Deathcap"),
    ("Zhonya's Hourglass", "Zhonya's"),
    ("Banshee's Veil", "Banshee's"),
    ("Void Staff", "Void"),
    ("Luden's Companion", "Luden's"),
    ("Liandry's Torment", "Liandry's"),
    ("Rylai's Crystal Scepter", "Rylai's"),
    ("Morellonomicon", "Morello"),
    ("Sterak's Gage", "Sterak's"),
    ("Death's Dance", "DD"),
    ("Black Cleaver", "Cleaver"),
    ("Serylda's Grudge", "Serylda's"),
    ("Youmuu's Ghostblade", "Youmuu's"),
    ("Edge of Night", "EoN"),
    ("Trinity Force", "Triforce"),
    ("Spear of Shojin", "Shojin"),
    ("Sunfire Aegis", "Sunfire"),
    ("Randuin's Omen", "Randuin's"),
    ("Force of Nature", "FoN"),
    ("Kaenic Rookern", "Rookern"),
    ("Jak'Sho, The Protean", "Jak'Sho"),
    ("Doran's Blade", "Doran's"),
    ("Doran's Bow", "Doran's Bow"),
    ("Doran's Ring", "Doran's"),
    ("Doran's Shield", "Doran's"),
    ("Health Potion", "Potion"),
    ("Stealth Ward", "Ward"),
];

pub(crate) fn short_of(pack: Option<&ChampionPack>, name: &str) -> String {
    if let Some(s) = pack.and_then(|p| p.short_opt(name)) {
        return s;
    }
    let key = normalize(name);
    if let Some((_, s)) = SHORTS.iter().find(|(n, _)| normalize(n) == key) {
        return s.to_string();
    }
    if name.chars().count() <= 12 {
        name.to_string()
    } else {
        name.split_whitespace().next().unwrap_or(name).to_string()
    }
}

pub fn profile(traits: &Traits, catalog: &Catalog, enemies: &[String]) -> EnemyProfile {
    let mut p = EnemyProfile {
        names: enemies.to_vec(),
        ..Default::default()
    };
    for name in enemies {
        if let Some(t) = traits.get(name) {
            if t.healing {
                p.healers.push(name.clone());
            }
            if t.tank {
                p.tanks.push(name.clone());
            }
            if t.control.is_some() {
                p.lockdown.push(name.clone());
            }
            if t.assassin {
                p.assassins.push(name.clone());
            }
            if t.poke {
                p.poke.push(name.clone());
            }
            match t.damage.as_str() {
                "ap" => p.ap += 1,
                "ad" => p.ad += 1,
                "mixed" => {
                    p.ap += 1;
                    p.ad += 1;
                }
                _ => {}
            }
        } else {
            p.unknown.push(name.clone());
            if let Some(c) = catalog
                .champion_key(name)
                .and_then(|key| catalog.champion(key))
            {
                if c.tags.iter().any(|t| t == "Tank") {
                    p.tanks.push(name.clone());
                }
                if c.tags.iter().any(|t| t == "Assassin") {
                    p.assassins.push(name.clone());
                }
            }
        }
    }
    p
}

pub(crate) fn item_by_id(
    cat: &Catalog,
    pack: Option<&ChampionPack>,
    id: u32,
    why: Option<String>,
) -> Option<PlanItem> {
    let i = cat.item(id)?;
    let role = if i.effects.boots {
        "boots"
    } else if i.effects.percent_armor_pen.is_some() {
        "armor_pen"
    } else if i.effects.percent_magic_pen.is_some() {
        "magic_pen"
    } else if i.effects.cleanse.is_some()
        || i.effects.shield.is_some()
        || i.effects.stasis
        || i.effects.spell_shield
        || i.effects.armor.is_some()
        || i.effects.magic_resist.is_some()
    {
        "defensive"
    } else if i.total < 1800 && i.is_owned_commitment(cat) {
        "utility"
    } else {
        "damage"
    };
    Some(PlanItem {
        id,
        name: i.name.clone(),
        short: short_of(pack, &i.name),
        cost: i.total,
        role: role.into(),
        why,
        ..Default::default()
    })
}

fn baseline_path(inp: &Inputs, a: &Aggregate) -> Vec<PlanItem> {
    let cat = inp.catalog;
    let mut path = Vec::new();
    let core = if a.core_most_picked.ids.is_empty() {
        &a.core
    } else {
        &a.core_most_picked
    };
    for &id in &core.ids {
        if !cat.item(id).is_some_and(|i| i.is_finished(cat)) {
            continue;
        }
        if path.iter().any(|p: &PlanItem| p.id == id) {
            continue;
        }
        if let Some(i) = item_by_id(
            cat,
            inp.pack,
            id,
            Some(format!("Most-played core line ({} games)", core.games)),
        ) {
            path.push(i);
        }
    }
    if !path.iter().any(|p| p.role == "boots") {
        if let Some(id) = a.boots.as_ref().and_then(|b| b.ids.first()).copied() {
            if let Some(i) = item_by_id(
                cat,
                inp.pack,
                id,
                Some("Most-played boots for this champion and role".into()),
            ) {
                path.insert(1.min(path.len()), i);
            }
        }
    }
    for line in &a.late {
        if path.len() >= PATH_LEN {
            break;
        }
        let Some(&id) = line.ids.first() else {
            continue;
        };
        if a.core_alternatives.contains(&id)
            || path.iter().any(|p| p.id == id)
            || !cat
                .item(id)
                .is_some_and(|i| i.is_finished(cat) && !i.effects.boots)
            || !shop::compatible(cat, id, &path.iter().map(|i| i.id).collect::<Vec<_>>())
        {
            continue;
        }
        if let Some(i) = item_by_id(
            cat,
            inp.pack,
            id,
            Some(format!(
                "Seen in {} final builds; timing unknown",
                line.games
            )),
        ) {
            path.push(i);
        }
    }
    path
}

/// A visible position wins. Trait guesses are used only when exactly one opponent fits.
pub(crate) fn lane_opponent(inp: &Inputs, position: Position) -> Option<String> {
    if let Some(live) = inp.live {
        let candidates: Vec<_> = live
            .enemies
            .iter()
            .filter(|p| Position::parse(&p.position) == Some(position))
            .collect();
        if candidates.len() == 1 {
            return Some(candidates[0].champion.clone());
        }
        if candidates.len() > 1 {
            return None;
        }
    }
    let candidates: Vec<_> =
        inp.enemies
            .iter()
            .filter(|name| {
                let known_other_role = inp.live.is_some_and(|live| {
                    live.enemies.iter().any(|p| {
                        normalize(&p.champion) == normalize(name)
                            && Position::parse(&p.position).is_some()
                    })
                });
                !known_other_role
                    && inp.traits.get(name).is_some_and(|t| {
                        t.roles.iter().any(|r| Position::parse(r) == Some(position))
                    })
            })
            .collect();
    (candidates.len() == 1).then(|| candidates[0].clone())
}

fn counter_line(inp: &Inputs, a: &Aggregate, opponent: &str) -> Option<String> {
    let key = inp.catalog.champion_key(opponent)?;
    let (_, n, w) = a.counters.iter().find(|c| c.0 == key)?;
    if *n < 50 || w > n {
        return None;
    }
    let ci = statistics::wilson_interval(*w, *n)?;
    Some(format!("vs {opponent}: {:.0}% game win rate ({} games; {:.0}–{:.0}% interval), not a lane prediction",
        f64::from(*w) / f64::from(*n) * 100.0, n, ci.lower * 100.0, ci.upper * 100.0))
}

fn ability_index(c: char) -> usize {
    match c {
        'Q' => 0,
        'W' => 1,
        'E' => 2,
        _ => 3,
    }
}

/// Ability per level for 18 levels: the given opening, then the max order; R at 6 / 11 / 16.
pub fn sequence(first: &[char], max: &[char]) -> Vec<char> {
    let mut counts = [0u8; 4];
    let mut seq = Vec::with_capacity(18);
    for level in 1..=18usize {
        let ability = if matches!(level, 6 | 11 | 16) {
            'R'
        } else if let Some(&c) = first
            .get(level - 1)
            .filter(|&&c| c != 'R' && counts[ability_index(c)] < 5)
        {
            c
        } else {
            max.iter()
                .copied()
                .find(|&c| c != 'R' && counts[ability_index(c)] < 5)
                .unwrap_or('Q')
        };
        counts[ability_index(ability)] += 1;
        seq.push(ability);
    }
    seq
}

fn chars(v: &[String]) -> Vec<char> {
    v.iter()
        .filter_map(|s| s.chars().next())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

pub fn skill_sequence(order: &SkillOrder) -> Vec<char> {
    sequence(&chars(&order.first), &chars(&order.max))
}

pub fn skill_label(max: &[char]) -> String {
    if max.is_empty() {
        String::new()
    } else {
        format!(
            "max {}",
            max.iter()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(" > ")
        )
    }
}

pub fn skill_plan(seq: &[char], label: &str, me: Option<&Me>) -> SkillPlan {
    match me {
        None => SkillPlan {
            next: seq.first().copied(),
            point_available: false,
            label: label.to_string(),
            levels: [0; 4],
        },
        Some(m) => {
            let spent = m.abilities.total() as usize;
            let levels = [m.abilities.q, m.abilities.w, m.abilities.e, m.abilities.r];
            // Reconcile the requested opening with the ranks actually learned. A different
            // level-one choice must not turn the aggregate's level-two choice into an illegal rank.
            let at_level = if spent < m.player.level as usize {
                m.player.level
            } else {
                (m.player.level + 1).min(18)
            };
            let legal = |c: char| match c {
                'Q' | 'W' | 'E' => {
                    m.abilities.get(c) < 5 && u32::from(m.abilities.get(c)) < at_level.div_ceil(2)
                }
                'R' => {
                    m.abilities.r < 3 && at_level >= [6, 11, 16][usize::from(m.abilities.r.min(2))]
                }
                _ => false,
            };
            let mut desired = [0u8; 4];
            for &c in seq
                .iter()
                .take(at_level as usize)
                .filter(|c| ['Q', 'W', 'E', 'R'].contains(c))
            {
                desired[ability_index(c)] += 1;
            }
            let next = if at_level >= 6 && desired[3] > levels[3] && legal('R') {
                Some('R')
            } else {
                seq.iter()
                    .copied()
                    .find(|&c| legal(c) && desired[ability_index(c)] > levels[ability_index(c)])
                    .or_else(|| seq.iter().skip(spent).copied().find(|&c| legal(c)))
            };
            SkillPlan {
                next,
                point_available: (spent as u32) < m.player.level,
                label: label.to_string(),
                levels,
            }
        }
    }
}

/// Project a target into legal shop actions. An unknown price never becomes a free item.
pub(crate) fn next_for_target(
    cat: &Catalog,
    target: &PlanItem,
    me: Option<&Me>,
    boots_locked: bool,
) -> NextItem {
    let inventory = me.map(|m| m.player.items.as_slice()).unwrap_or(&[]);
    let gold = me.map(|m| m.gold).unwrap_or(0.0);
    let context = shop::ShopContext {
        champion: me.map(|m| m.player.champion.as_str()),
        spell_ids: me
            .filter(|m| !m.spell_ids.is_empty())
            .map(|m| m.spell_ids.as_slice()),
        boots_locked,
    };
    let q = shop::quote_with_context(cat, target.id, inventory, gold, &context);
    let affordable = q
        .buy_now
        .as_ref()
        .is_some_and(|b| f64::from(b.cost) <= gold)
        && q.blocked.is_none();
    let buy_now = q.buy_now.or(q.save_for).or_else(|| {
        if q.blocked.is_some() {
            return None;
        }
        // This fallback is a saving target only, never claimed to be affordable.
        q.components
            .iter()
            .filter(|c| !c.owned)
            .min_by_key(|c| (c.cost, c.id))
            .cloned()
            .or_else(|| {
                q.remaining_cost.map(|cost| Component {
                    id: target.id,
                    name: target.name.clone(),
                    cost,
                    owned: false,
                })
            })
    });
    let save_gap = buy_now
        .as_ref()
        .filter(|_| gold.is_finite())
        .and_then(|b| (f64::from(b.cost) > gold).then(|| (f64::from(b.cost) - gold).ceil() as u32));
    NextItem {
        id: target.id,
        name: target.name.clone(),
        cost: target.cost,
        remaining_cost: q.remaining_cost.unwrap_or(0),
        price_known: q.remaining_cost.is_some(),
        components: q.components,
        buy_now,
        buy_now_affordable: affordable,
        basket: q.basket,
        basket_cost: q.basket_cost,
        save_gap,
        blocked: q.blocked,
    }
}

pub fn next_item(
    cat: &Catalog,
    _pack: Option<&ChampionPack>,
    path: &[PlanItem],
    me: Option<&Me>,
    boots_locked: bool,
) -> Option<NextItem> {
    path.iter()
        .find(|p| !p.owned && !(boots_locked && p.role == "boots"))
        .map(|target| next_for_target(cat, target, me, boots_locked))
}

fn standard_skill_system(champion: &str, a: &Aggregate) -> bool {
    // These champions spend points on transformations, four basic skills, or stats.
    // This is a mechanics boundary, not an item preference or champion whitelist.
    !["aphelios", "udyr", "jayce", "elise", "nidalee", "karma"]
        .contains(&normalize(champion).as_str())
        && a.skill_order
            .iter()
            .all(|c| ['Q', 'W', 'E', 'R'].contains(c))
        && a.skill_max.len() == 3
}

pub fn plan(inp: &Inputs) -> Plan {
    plan_with_preferences(inp, &PlannerPreferences::default())
}

pub fn plan_with_preferences(inp: &Inputs, preferences: &PlannerPreferences) -> Plan {
    let mut p = Plan {
        champion: inp.champion.into(),
        preferences: preferences.clone(),
        enemy: profile(inp.traits, inp.catalog, inp.enemies),
        ..Default::default()
    };
    let cat = inp.catalog;
    if let Some(live) = inp.live {
        if !["classic", "swiftplay", "practicetool"].contains(&normalize(&live.mode).as_str()) {
            p.note = Some(format!(
                "{} needs its own mode data; purchase advice is paused",
                if live.mode.is_empty() {
                    "This mode"
                } else {
                    &live.mode
                }
            ));
            return p;
        }
    }
    let Some(a) = inp
        .aggregate
        .filter(|a| cat.champion_key(inp.champion) == Some(a.champion_key))
    else {
        p.note = Some(format!(
            "Waiting for {}'s build data. No invented fallback build.",
            inp.champion
        ));
        return p;
    };
    if a.requested_position.is_some_and(|r| r != a.position) {
        p.note = Some(
            "No build data for your assigned role; a different role's loadout will not be imported"
                .into(),
        );
        return p;
    }
    let actual_position = inp
        .live
        .and_then(|l| l.me.as_ref())
        .and_then(|m| Position::parse(&m.player.position));
    if actual_position.is_some_and(|role| role != a.position) {
        p.note = Some("Loading build data for your actual role; purchase advice is paused".into());
        return p;
    }
    p.position = Some(a.position.label().into());
    p.source = Some(format!("{}, patch {}", a.describe(), a.patch));
    if let Some(warning) = &a.provenance.warning {
        p.warnings.push(warning.clone());
    }
    if !cat.version.starts_with(&format!("{}.", a.patch)) && cat.version != a.patch {
        p.warnings.push(format!(
            "Build sample is patch {}; item prices are patch {}",
            a.patch, cat.version
        ));
    }
    if a.games < aggregate_sample_floor() {
        p.warnings.push(format!(
            "Limited role data: {} games; treat this as a low-confidence baseline",
            a.games
        ));
    }
    let me = inp.live.and_then(|l| l.me.as_ref());
    let mut starter_ids = a.starters.ids.clone();
    // Some support aggregates report only the consumables. The catalog's unique
    // purchasable base quest is a role mechanic, not a preferred champion build.
    let mut derived_support_starter = None;
    if a.position == Position::Support
        && !starter_ids.iter().any(|id| {
            cat.item(*id)
                .is_some_and(|i| i.exclusive_groups().contains(&"SupportQuest"))
        })
    {
        let candidates: Vec<_> = cat
            .items
            .values()
            .filter(|i| {
                i.purchasable
                    && i.in_store
                    && i.on_sr
                    && i.from.is_empty()
                    && i.special_recipe.is_none()
                    && i.exclusive_groups().contains(&"SupportQuest")
            })
            .collect();
        if candidates.len() == 1 {
            derived_support_starter = Some(candidates[0].id);
            starter_ids.insert(0, candidates[0].id);
        }
    }
    let mut starter_counts = std::collections::HashMap::new();
    p.start = starter_ids
        .iter()
        .filter_map(|&id| item_by_id(cat, inp.pack, id, None))
        .map(|mut item| {
            item.role = "start".into();
            let count = starter_counts.entry(item.id).or_insert(0);
            *count += 1;
            item.owned = me.is_some_and(|m| m.player.item_count(item.id) >= *count);
            item
        })
        .collect();
    p.runes = a.runes.clone();
    p.runes_summary = p
        .runes
        .as_ref()
        .and_then(|r| {
            r.perks
                .first()
                .map(|&id| format!("{} / {}", cat.rune_name(id), cat.style_name(r.sub_style)))
        })
        .unwrap_or_default();
    p.spell_ids = me
        .filter(|m| m.spell_ids.len() == 2)
        .map(|m| m.spell_ids.clone())
        .unwrap_or_else(|| a.spells.ids.clone());
    let opponent = lane_opponent(inp, a.position);
    p.matchup_champion = opponent.clone();
    p.matchup = opponent.as_deref().and_then(|o| {
        inp.pack
            .and_then(|pack| pack.matchup(o))
            .map(|m| m.line.clone())
            .or_else(|| counter_line(inp, a, o))
    });

    // Only a verified, removable lane stun can trigger this spell adaptation.
    // Smite/Teleport and non-ADC roles are never rewritten by a marksman matchup.
    if inp.live.is_none()
        && a.position == Position::Adc
        && p.spell_ids.len() == 2
        && !p.spell_ids.contains(&1)
        && !p.spell_ids.contains(&11)
    {
        if let Some(o) = opponent.as_deref() {
            if inp
                .traits
                .get(o)
                .and_then(|t| t.control.as_ref())
                .is_some_and(|c| {
                    c.summoner_cleanse_removes()
                        && cat.version.starts_with(&format!("{}.", c.verified_patch))
                })
            {
                if let Some(index) = p.spell_ids.iter().position(|id| *id != 4) {
                    p.spell_ids[index] = 1;
                    p.context.push(format!(
                        "Cleanse can remove {o}'s lane stun; it does not prevent the damage"
                    ));
                }
            }
        }
    }
    p.spells = p
        .spell_ids
        .iter()
        .map(|&id| {
            runes::spell_name(id)
                .map(str::to_string)
                .unwrap_or_else(|| format!("spell {id}"))
        })
        .collect();

    let footwear = cat.rune_id("Magical Footwear").unwrap_or(8304);
    let has_boots = me.is_some_and(|m| {
        m.player
            .items
            .iter()
            .any(|i| cat.item(i.id).is_some_and(|i| i.effects.boots))
    });
    // The recommended rune page is not proof of the rune actually taken.
    let boots_locked = me
        .is_some_and(|m| m.rune_ids.as_ref().is_some_and(|r| r.contains(&footwear)))
        && !has_boots;
    if me.is_some_and(|m| m.rune_ids.is_none()) {
        p.warnings
            .push("Actual runes unavailable; no rune-based purchase lock is assumed".into());
    }
    if standard_skill_system(inp.champion, a) && !a.skill_order.is_empty() {
        p.skill = skill_plan(
            &sequence(&a.skill_order, &a.skill_max),
            &skill_label(&a.skill_max),
            me,
        );
    } else if !a.skill_order.is_empty() {
        p.warnings.push(
            "This champion has a nonstandard skill system; use its in-game rank rules".into(),
        );
    }
    let mut base = baseline_path(inp, a);
    if boots_locked {
        for boots in base.iter_mut().filter(|p| p.role == "boots") {
            boots.tag = Some("free boots".into());
            boots.why = Some(
                "Your actual Magical Footwear rune prevents buying boots before delivery".into(),
            );
        }
        p.context
            .push("Your Magical Footwear rune locks boots until the free pair arrives".into());
    }
    let selected = decision::select(inp, base, preferences, boots_locked);
    p.path = selected.path;
    p.options = selected.options;
    p.next = selected.next;
    p.learning = selected.learning;
    p.alternative = selected.alternative;
    p.preferences = selected.preferences;
    p.context.extend(selected.context);
    p.warnings.extend(selected.warnings);
    p.score_trace = selected.scores;

    // Opening purchases use their actual multiplicities. Never restart the starter kit mid-game.
    if let (Some(live), Some(m)) = (inp.live, me) {
        // A different starter or early component is a deliberate investment. Do not
        // add the aggregate's kit on top of it; resume the inventory-aware main path.
        let manual_opening = m.player.items.iter().any(|i| {
            i.slot < 6
                && !starter_ids.contains(&i.id)
                && cat
                    .item(i.id)
                    .is_none_or(|item| !item.tags.iter().any(|t| t == "Consumable"))
        });
        if live.game_time < 90.0
            && m.player.level == 1
            && preferences.pinned_item.is_none()
            && !manual_opening
        {
            let mut seen = std::collections::HashMap::new();
            if let Some(id) = starter_ids.iter().copied().find(|id| {
                let count = seen.entry(*id).or_insert(0);
                *count += 1;
                m.player.item_count(*id) < *count
                    && cat
                        .item(*id)
                        .is_some_and(|i| i.purchasable && i.in_store && i.on_sr)
            }) {
                if let Some(target) = item_by_id(cat, inp.pack, id, None) {
                    p.next = Some(next_for_target(cat, &target, me, boots_locked));
                    let (reason, evidence) = if Some(id) == derived_support_starter {
                        (
                            format!(
                                "{}: your support quest provides income and wards",
                                target.short
                            ),
                            Evidence::Composition,
                        )
                    } else {
                        (
                            format!(
                                "{}: the common starting purchase for {} {}",
                                target.short,
                                inp.champion,
                                a.position.label()
                            ),
                            Evidence::Aggregate,
                        )
                    };
                    p.learning = Some(coaching::explain(DecisionKind::Start, reason, evidence));
                    p.alternative = None;
                }
            }
        }
    }
    if inp.live.is_some() && me.is_none() {
        p.next = None;
        p.learning = None;
        p.warnings
            .push("Your live identity is unavailable; waiting for a fresh observation".into());
    }
    if let Some(tip) = &p.learning {
        p.why.push(tip.reason.clone());
    }
    p.why.extend(p.context.iter().take(3).cloned());
    if p.path.is_empty() {
        p.warnings
            .push("No compatible build items are available in this patch's catalog".into());
    }
    p.note = p.warnings.first().cloned();
    let core = if a.core_most_picked.ids.is_empty() {
        &a.core
    } else {
        &a.core_most_picked
    };
    let mut core_bag = core.ids.clone();
    core_bag.sort_unstable();
    let alternative = a.core_lines.iter().filter(|l| l.ids != core.ids).find(|l| {
        let mut bag = l.ids.clone();
        bag.sort_unstable();
        bag == core_bag
    });
    p.core_evidence = Some(CoreEvidence {
        games: core.games,
        wins: core.wins,
        win_rate: Aggregate::win_rate_of(core),
        interval: statistics::wilson_interval(core.wins, core.games),
        comparison_games: alternative.map(|l| l.games),
        difference: alternative.and_then(|l| {
            statistics::independent_difference_interval(l.wins, l.games, core.wins, core.games)
        }),
    });
    p
}

fn aggregate_sample_floor() -> u32 {
    crate::aggregate::MIN_GAMES
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ddragon::test_support::catalog;
    use crate::pack::{load_traits, load_xayah};

    #[test]
    fn popular_core_is_not_promoted_by_a_noisy_win_rate_and_pack_defaults_are_ignored() {
        let cat = catalog();
        let mut pack = load_xayah().unwrap();
        pack.core.clear();
        let traits = load_traits().unwrap();
        let raw = serde_json::from_str(include_str!(
            "../../../m0/tests/fixtures/opgg_xayah_adc.json"
        ))
        .unwrap();
        let a =
            crate::aggregate::decode(&raw, 498, Position::Adc, "global", "emerald_plus").unwrap();
        let p = plan(&Inputs {
            champion: "Xayah",
            pack: Some(&pack),
            aggregate: Some(&a),
            traits: &traits,
            catalog: &cat,
            enemies: &[],
            live: None,
        });
        assert_eq!(
            p.path.iter().take(4).map(|i| i.id).collect::<Vec<_>>(),
            [3032, 3006, 6675, 3031]
        );
        assert!(p.why.first().is_some_and(|s| !s.is_empty()));
        assert!(p
            .core_evidence
            .unwrap()
            .difference
            .is_some_and(|d| d.lower < 0.0 && d.upper > 0.0));
    }

    #[test]
    fn sequence_and_short_labels_remain_available_to_importers() {
        let pack = load_xayah().unwrap();
        assert_eq!(
            skill_sequence(&pack.skill_order)
                .into_iter()
                .collect::<String>(),
            "QEWEEREEWWRWWQQRQQ"
        );
        assert_eq!(short_of(None, "Yun Tal Wildarrows"), "Yun Tal");
    }
}
