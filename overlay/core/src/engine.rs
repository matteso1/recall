//! The build brain. The base build is what players run on this patch (the aggregate, per champion
//! and position: start, core items, boots, skill order, rune page, summoner spells). The hand-curated
//! pack, when there is one for the champion, adds the late slots, the lane matchup lines, the item
//! alternatives and the explicit rules that adapt the path to the enemy comp and the live game.
//! Every change carries one line of why. Without a pack the rules that only need item roles still run.
use crate::aggregate::{Aggregate, Position, RunePageIds};
use crate::ddragon::{normalize, Catalog};
use crate::live::{LiveSnapshot, Me, Player};
use crate::pack::{ChampionPack, SkillOrder, Traits};
use crate::runes;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct PlanItem {
    pub id: u32,
    pub name: String,
    pub short: String,
    pub cost: u32,
    pub owned: bool,
    /// damage | boots | armor_pen | defensive | start
    pub role: String,
    pub why: Option<String>,
    /// Set when a rule changed this slot, e.g. "Soraka"
    pub tag: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Component {
    pub id: u32,
    pub name: String,
    pub cost: u32,
    pub owned: bool,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct NextItem {
    pub id: u32,
    pub name: String,
    pub cost: u32,
    /// Cost still to pay given the components already owned
    pub remaining_cost: u32,
    pub components: Vec<Component>,
    /// What to click right now (a component, or the finished item when it completes)
    pub buy_now: Option<Component>,
    pub buy_now_affordable: bool,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct SkillPlan {
    pub next: Option<char>,
    pub point_available: bool,
    pub label: String,
    /// Q W E R
    pub levels: [u8; 4],
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct EnemyProfile {
    pub names: Vec<String>,
    pub healers: Vec<String>,
    pub tanks: Vec<String>,
    pub lockdown: Vec<String>,
    pub assassins: Vec<String>,
    pub poke: Vec<String>,
    pub ap: u32,
    pub ad: u32,
    pub unknown: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Plan {
    pub champion: String,
    /// "ADC", when the base build comes from the aggregate
    pub position: Option<String>,
    /// Where the base build comes from, e.g. "op.gg emerald+ global, 88k games, patch 16.17"
    pub source: Option<String>,
    /// e.g. "Xayah is rarely played Support: showing the ADC build"
    pub note: Option<String>,
    pub start: Vec<PlanItem>,
    pub path: Vec<PlanItem>,
    /// Other popular finished items that did not make the path
    pub options: Vec<PlanItem>,
    pub next: Option<NextItem>,
    pub skill: SkillPlan,
    /// One line per change, most important first
    pub why: Vec<String>,
    pub matchup: Option<String>,
    pub matchup_champion: Option<String>,
    /// The rune page to import (ids the client takes as-is)
    pub runes: Option<RunePageIds>,
    pub runes_summary: String,
    pub spells: Vec<String>,
    pub spell_ids: Vec<u32>,
    pub enemy: EnemyProfile,
}

pub struct Inputs<'a> {
    /// Display name of our champion
    pub champion: &'a str,
    /// Hand-curated rules for this champion, if the pack knows it
    pub pack: Option<&'a ChampionPack>,
    /// What players run on this patch, if it could be fetched
    pub aggregate: Option<&'a Aggregate>,
    pub traits: &'a Traits,
    pub catalog: &'a Catalog,
    /// Enemy champion display names
    pub enemies: &'a [String],
    pub live: Option<&'a LiveSnapshot>,
}

/// Item ids that mean "this enemy is stacking armor".
const ARMOR_ITEM_MIN_COST: u32 = 900;
const PATH_LEN: usize = 6;
/// Magical Footwear (Inspiration): no boots can be bought until the free Slightly Magical Footwear
/// arrives (12:00, 45 s earlier per takedown); those then upgrade into the real boots.
const MAGICAL_FOOTWEAR_PERK: u32 = 8304;
const SLIGHTLY_MAGICAL_FOOTWEAR: u32 = 2422;
const BOOTS: u32 = 1001;
const FOOTWEAR_TAG: &str = "free @12";

/// Short labels for the path line; a pack's own shorts win.
const SHORTS: &[(&str, &str)] = &[
    ("Infinity Edge", "IE"), ("Essence Reaver", "ER"), ("Lord Dominik's Regards", "LDR"), ("Guardian Angel", "GA"),
    ("Bloodthirster", "BT"), ("Phantom Dancer", "PD"), ("Rapid Firecannon", "RFC"), ("Statikk Shiv", "Shiv"),
    ("Kraken Slayer", "Kraken"), ("The Collector", "Collector"), ("Navori Flickerblade", "Navori"),
    ("Yun Tal Wildarrows", "Yun Tal"), ("Berserker's Greaves", "Greaves"), ("Mortal Reminder", "Mortal"),
    ("Mercurial Scimitar", "Merc"), ("Maw of Malmortius", "Maw"), ("Immortal Shieldbow", "Shieldbow"),
    ("Runaan's Hurricane", "Hurricane"), ("Blade of The Ruined King", "BotRK"), ("Plated Steelcaps", "Steelcaps"),
    ("Mercury's Treads", "Mercs"), ("Boots of Swiftness", "Swifties"), ("Sorcerer's Shoes", "Sorcs"),
    ("Ionian Boots of Lucidity", "Lucidity"), ("Rabadon's Deathcap", "Deathcap"), ("Zhonya's Hourglass", "Zhonya's"),
    ("Banshee's Veil", "Banshee's"), ("Void Staff", "Void"), ("Luden's Companion", "Luden's"),
    ("Liandry's Torment", "Liandry's"), ("Rylai's Crystal Scepter", "Rylai's"), ("Morellonomicon", "Morello"),
    ("Sterak's Gage", "Sterak's"), ("Death's Dance", "DD"), ("Black Cleaver", "Cleaver"), ("Serylda's Grudge", "Serylda's"),
    ("Youmuu's Ghostblade", "Youmuu's"), ("Edge of Night", "EoN"), ("Trinity Force", "Triforce"),
    ("Spear of Shojin", "Shojin"), ("Sunfire Aegis", "Sunfire"), ("Randuin's Omen", "Randuin's"),
    ("Force of Nature", "FoN"), ("Kaenic Rookern", "Rookern"), ("Jak'Sho, The Protean", "Jak'Sho"),
    ("Doran's Blade", "Doran's"), ("Doran's Bow", "Doran's Bow"), ("Doran's Ring", "Doran's"),
    ("Doran's Shield", "Doran's"), ("Health Potion", "Potion"), ("Stealth Ward", "Ward"),
];

fn short_of(pack: Option<&ChampionPack>, name: &str) -> String {
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
    let mut p = EnemyProfile { names: enemies.to_vec(), ..Default::default() };
    for name in enemies {
        match traits.get(name) {
            Some(t) => {
                if t.healing {
                    p.healers.push(name.clone());
                }
                if t.tank {
                    p.tanks.push(name.clone());
                }
                if t.lockdown_ult {
                    p.lockdown.push(name.clone());
                }
                if t.assassin {
                    p.assassins.push(name.clone());
                }
                if t.poke {
                    p.poke.push(name.clone());
                }
                let is_carry = !t.tank && !(t.roles.len() == 1 && t.roles[0] == "utility");
                if is_carry {
                    match t.damage.as_str() {
                        "ap" => p.ap += 1,
                        "ad" => p.ad += 1,
                        "mixed" => {
                            p.ap += 1;
                            p.ad += 1;
                        }
                        _ => {}
                    }
                }
            }
            None => {
                // Fall back to Data Dragon class tags for champions the pack does not know.
                p.unknown.push(name.clone());
                if let Some(c) = catalog.champion_key(name).and_then(|k| catalog.champion(k)) {
                    let has = |t: &str| c.tags.iter().any(|x| x == t);
                    if has("Tank") {
                        p.tanks.push(name.clone());
                    }
                    if has("Assassin") {
                        p.assassins.push(name.clone());
                    }
                    if !has("Tank") && !has("Support") {
                        if has("Mage") {
                            p.ap += 1;
                        }
                        if has("Marksman") || has("Fighter") {
                            p.ad += 1;
                        }
                    }
                }
            }
        }
    }
    p
}

fn join(names: &[String]) -> String {
    match names.len() {
        0 => String::new(),
        1 => names[0].clone(),
        2 => format!("{} and {}", names[0], names[1]),
        _ => format!("{} and {}", names[..names.len() - 1].join(", "), names[names.len() - 1]),
    }
}

fn item_by_id(cat: &Catalog, pack: Option<&ChampionPack>, id: u32, role: &str, why: Option<String>) -> Option<PlanItem> {
    let item = cat.item(id)?;
    Some(PlanItem {
        id,
        name: item.name.clone(),
        short: short_of(pack, &item.name),
        cost: item.total,
        owned: false,
        role: role.to_string(),
        why,
        tag: None,
    })
}

fn make_item(cat: &Catalog, pack: Option<&ChampionPack>, name: &str, role: &str, why: Option<String>) -> Option<PlanItem> {
    item_by_id(cat, pack, cat.item_id(name)?, role, why)
}

fn position_of(path: &[PlanItem], name: &str, cat: &Catalog) -> Option<usize> {
    let id = cat.item_id(name)?;
    path.iter().position(|p| p.id == id)
}

fn replace(path: &mut [PlanItem], idx: usize, cat: &Catalog, pack: Option<&ChampionPack>, name: &str, tag: &str, why: &str) -> bool {
    let role = path[idx].role.clone();
    match make_item(cat, pack, name, &role, Some(why.to_string())) {
        Some(mut item) => {
            item.tag = Some(tag.to_string());
            path[idx] = item;
            true
        }
        None => false,
    }
}

/// Move the item at `idx` one slot earlier (never before the first item).
fn move_up(path: &mut [PlanItem], idx: usize) -> bool {
    if idx == 0 || idx >= path.len() {
        return false;
    }
    path.swap(idx - 1, idx);
    true
}

fn has_tag(cat: &Catalog, id: u32, tag: &str) -> bool {
    cat.item(id).map(|i| i.tags.iter().any(|t| t == tag)).unwrap_or(false)
}

/// A legendary you keep: not a component, not boots, not a consumable.
fn is_finished(cat: &Catalog, id: u32) -> bool {
    cat.item(id)
        .map(|i| i.into.is_empty() && i.total >= 2000 && !i.tags.iter().any(|t| t == "Boots" || t == "Consumable" || t == "Trinket"))
        .unwrap_or(false)
}

/// Role of an item we know nothing else about, from its Data Dragon tags.
fn role_by_tags(cat: &Catalog, id: u32) -> &'static str {
    if has_tag(cat, id, "Boots") {
        "boots"
    } else if has_tag(cat, id, "ArmorPenetration") || has_tag(cat, id, "MagicPenetration") {
        "armor_pen"
    } else if has_tag(cat, id, "Armor") || has_tag(cat, id, "SpellBlock") {
        "defensive"
    } else {
        "damage"
    }
}

/// The path before any rule: aggregate core + boots, then the pack's late slots (armor pen,
/// defensive), then the most popular finished items until six. Pack-only when there is no aggregate.
fn base_path(cat: &Catalog, pack: Option<&ChampionPack>, agg: Option<&Aggregate>) -> Vec<PlanItem> {
    let mut path: Vec<PlanItem> = Vec::new();
    let Some(a) = agg.filter(|a| !a.core.ids.is_empty()) else {
        if let Some(p) = pack {
            path = p
                .core
                .iter()
                .filter_map(|c| make_item(cat, pack, &c.item, c.role.as_deref().unwrap_or("damage"), c.why.clone()))
                .collect();
        }
        return path;
    };
    let core_why = Some(format!("core line in {:.0}% of games", a.core.pick_rate * 100.0));
    for &id in &a.core.ids {
        if let Some(item) = item_by_id(cat, pack, id, role_by_tags(cat, id), core_why.clone()) {
            path.push(item);
        }
    }
    if !path.iter().any(|p| p.role == "boots") {
        if let Some(b) = a.boots.as_ref().and_then(|b| b.ids.first().copied()) {
            if let Some(item) = item_by_id(cat, pack, b, "boots", Some("the boots most players take".into())) {
                let at = 1.min(path.len());
                path.insert(at, item);
            }
        }
    }
    if let Some(p) = pack {
        for c in &p.core {
            if path.len() >= PATH_LEN {
                break;
            }
            let role = c.role.as_deref().unwrap_or("damage");
            if role == "damage" || role == "boots" || position_of(&path, &c.item, cat).is_some() {
                continue;
            }
            if let Some(item) = make_item(cat, pack, &c.item, role, c.why.clone()) {
                path.push(item);
            }
        }
    }
    for l in &a.late {
        if path.len() >= PATH_LEN {
            break;
        }
        let Some(&id) = l.ids.first() else { continue };
        if path.iter().any(|p| p.id == id) || a.core_alternatives.contains(&id) || !is_finished(cat, id) {
            continue;
        }
        let role = role_by_tags(cat, id);
        if role == "armor_pen" && path.iter().any(|p| p.role == "armor_pen") {
            continue;
        }
        let why = Some(format!("in {:.0}% of finished builds", l.pick_rate * 100.0));
        if let Some(item) = item_by_id(cat, pack, id, role, why) {
            path.push(item);
        }
    }
    path
}

/// Popular finished items that are not in the path (for the item set and the tooltip).
fn options(cat: &Catalog, pack: Option<&ChampionPack>, agg: Option<&Aggregate>, path: &[PlanItem]) -> Vec<PlanItem> {
    let Some(a) = agg else { return Vec::new() };
    a.late
        .iter()
        .filter_map(|l| l.ids.first().copied().map(|id| (id, l.pick_rate)))
        .filter(|(id, _)| is_finished(cat, *id) && !path.iter().any(|p| p.id == *id))
        .take(6)
        .filter_map(|(id, rate)| item_by_id(cat, pack, id, role_by_tags(cat, id), Some(format!("in {:.0}% of finished builds", rate * 100.0))))
        .collect()
}

/// The enemy in our lane: a champion the traits place in our role, else a Marksman for bot.
fn lane_opponent(inp: &Inputs, role: Option<&str>) -> Option<String> {
    let role = role?;
    for name in inp.enemies {
        if let Some(t) = inp.traits.get(name) {
            if t.roles.iter().any(|r| r == role) {
                return Some(name.clone());
            }
        }
    }
    if role == "bottom" {
        for name in inp.enemies {
            if let Some(c) = inp.catalog.champion_key(name).and_then(|k| inp.catalog.champion(k)) {
                if c.tags.iter().any(|t| t == "Marksman") {
                    return Some(name.clone());
                }
            }
        }
    }
    None
}

/// "vs Jinx: Xayah wins 50% of these lanes at this rank (607 games)", from the aggregate's counters,
/// for champions without a hand-written matchup line. Says nothing below 50 games.
fn counter_line(inp: &Inputs, agg: Option<&Aggregate>, opponent: Option<&str>) -> Option<String> {
    let (a, o) = (agg?, opponent?);
    let key = inp.catalog.champion_key(o)?;
    let (_, games, wins) = a.counters.iter().find(|c| c.0 == key).copied()?;
    if games < 50 {
        return None;
    }
    Some(format!(
        "vs {o}: {} wins {:.0}% of these lanes at this rank ({games} games)",
        inp.champion,
        wins as f64 / games as f64 * 100.0
    ))
}

fn role_name(position: Position) -> &'static str {
    match position {
        Position::Top => "top",
        Position::Jungle => "jungle",
        Position::Mid => "middle",
        Position::Adc => "bottom",
        Position::Support => "utility",
    }
}

fn is_armor_item(cat: &Catalog, id: u32) -> bool {
    cat.item(id)
        .map(|i| i.tags.iter().any(|t| t == "Armor") && i.total >= ARMOR_ITEM_MIN_COST)
        .unwrap_or(false)
}

fn armor_stacker(cat: &Catalog, enemies: &[Player]) -> Option<String> {
    enemies
        .iter()
        .find(|p| p.items.iter().filter(|i| is_armor_item(cat, i.id)).count() >= 2)
        .map(|p| p.champion.clone())
}

fn fed_assassin(profile: &EnemyProfile, enemies: &[Player]) -> Option<String> {
    enemies
        .iter()
        .find(|p| profile.assassins.iter().any(|a| normalize(a) == normalize(&p.champion)) && p.kills >= 3 && p.kills > p.deaths)
        .map(|p| p.champion.clone())
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
        } else if let Some(&c) = first.get(level - 1).filter(|&&c| c != 'R' && counts[ability_index(c)] < 5) {
            c
        } else {
            max.iter().copied().find(|&c| c != 'R' && counts[ability_index(c)] < 5).unwrap_or('Q')
        };
        counts[ability_index(ability)] += 1;
        seq.push(ability);
    }
    seq
}

fn chars(v: &[String]) -> Vec<char> {
    v.iter().filter_map(|s| s.chars().next()).map(|c| c.to_ascii_uppercase()).collect()
}

pub fn skill_sequence(order: &SkillOrder) -> Vec<char> {
    sequence(&chars(&order.first), &chars(&order.max))
}

pub fn skill_label(max: &[char]) -> String {
    if max.is_empty() {
        String::new()
    } else {
        format!("max {}", max.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(" > "))
    }
}

pub fn skill_plan(seq: &[char], label: &str, me: Option<&Me>) -> SkillPlan {
    match me {
        None => SkillPlan { next: seq.first().copied(), point_available: false, label: label.to_string(), levels: [0; 4] },
        Some(m) => {
            let spent = m.abilities.total() as usize;
            SkillPlan {
                next: seq.get(spent).copied(),
                point_available: (spent as u32) < m.player.level,
                label: label.to_string(),
                levels: [m.abilities.q, m.abilities.w, m.abilities.e, m.abilities.r],
            }
        }
    }
}

fn component_ids(cat: &Catalog, pack: Option<&ChampionPack>, item: &PlanItem) -> Vec<u32> {
    let from_pack = pack
        .and_then(|p| p.core.iter().find(|c| normalize(&c.item) == normalize(&item.name)))
        .and_then(|c| c.components.as_ref())
        .map(|names| names.iter().filter_map(|n| cat.item_id(n)).collect::<Vec<u32>>());
    match from_pack {
        Some(ids) if !ids.is_empty() => ids,
        _ => cat.components(item.id),
    }
}

/// The first item still to buy; boots are skipped while Magical Footwear has them locked.
pub fn next_item(cat: &Catalog, pack: Option<&ChampionPack>, path: &[PlanItem], me: Option<&Me>, boots_locked: bool) -> Option<NextItem> {
    let target = path.iter().find(|p| !p.owned && !(boots_locked && p.role == "boots"))?;
    let mut inventory: HashMap<u32, u32> = HashMap::new();
    if let Some(m) = me {
        for i in &m.player.items {
            // The free footwear stands in for Boots in every boots recipe.
            let id = if i.id == SLIGHTLY_MAGICAL_FOOTWEAR { BOOTS } else { i.id };
            *inventory.entry(id).or_insert(0) += i.count;
        }
    }
    let mut components = Vec::new();
    let mut owned_cost = 0u32;
    for id in component_ids(cat, pack, target) {
        let owned = match inventory.get_mut(&id) {
            Some(n) if *n > 0 => {
                *n -= 1;
                true
            }
            _ => false,
        };
        let cost = cat.item_cost(id);
        if owned {
            owned_cost += cost;
        }
        components.push(Component { id, name: cat.item_name(id), cost, owned });
    }
    let remaining_cost = target.cost.saturating_sub(owned_cost);
    let gold = me.map(|m| m.gold).unwrap_or(0.0);
    let whole_affordable = gold >= remaining_cost as f64;
    let (buy_now, affordable) = if whole_affordable && me.is_some() {
        (Some(Component { id: target.id, name: target.name.clone(), cost: remaining_cost, owned: false }), true)
    } else if let Some(c) = components.iter().find(|c| !c.owned && gold >= c.cost as f64) {
        (Some(c.clone()), true)
    } else if let Some(c) = components.iter().find(|c| !c.owned) {
        (Some(c.clone()), false)
    } else {
        (Some(Component { id: target.id, name: target.name.clone(), cost: remaining_cost, owned: false }), whole_affordable)
    };
    Some(NextItem {
        id: target.id,
        name: target.name.clone(),
        cost: target.cost,
        remaining_cost,
        components,
        buy_now,
        buy_now_affordable: affordable,
    })
}

fn spell_ids_of(names: &[String]) -> Vec<u32> {
    names.iter().filter_map(|n| runes::spell_id(n).map(|id| id as u32)).collect()
}

pub fn plan(inp: &Inputs) -> Plan {
    let (cat, pack, agg) = (inp.catalog, inp.pack, inp.aggregate);
    let enemy = profile(inp.traits, cat, inp.enemies);
    let mut why: Vec<String> = Vec::new();
    let mut path = base_path(cat, pack, agg);

    // Start, spells and runes: the aggregate's picks, else the pack's.
    let mut start_ids: Vec<u32> = agg
        .map(|a| a.starters.ids.clone())
        .filter(|v| !v.is_empty())
        .or_else(|| pack.map(|p| p.start.iter().filter_map(|n| cat.item_id(n)).collect()))
        .unwrap_or_default();
    let mut spell_ids: Vec<u32> = agg
        .map(|a| a.spells.ids.clone())
        .filter(|v| v.len() == 2)
        .or_else(|| pack.map(|p| spell_ids_of(&p.spells)))
        .unwrap_or_default();
    let runes_page: Option<RunePageIds> = agg
        .and_then(|a| a.runes.clone())
        .or_else(|| pack.and_then(|p| runes::page_ids(&p.runes, cat).ok()));

    // Lane matchup: line, optional first-item / start / spell overrides (pack only).
    let role = pack.map(|p| p.role.clone()).or_else(|| agg.map(|a| role_name(a.position).to_string()));
    let opponent = lane_opponent(inp, role.as_deref());
    let matchup = opponent.as_deref().and_then(|o| pack.and_then(|p| p.matchup(o)));
    if let (Some(m), Some(o)) = (matchup, opponent.as_deref()) {
        if let Some(fi) = &m.first_item {
            if let Some(idx) = path.iter().position(|p| p.role == "damage") {
                if position_of(&path, fi, cat).is_none() {
                    let reason = format!("{fi} first vs {o}");
                    if replace(&mut path, idx, cat, pack, fi, o, &reason) {
                        why.push(reason);
                    }
                }
            }
        }
        if let Some(s) = &m.spells {
            let ids = spell_ids_of(s);
            if ids.len() == 2 && ids != spell_ids {
                why.push(format!("{} vs {o}", s.join(" + ")));
                spell_ids = ids;
            }
        }
        if let Some(s) = &m.start {
            let ids: Vec<u32> = s.iter().filter_map(|n| cat.item_id(n)).collect();
            if !ids.is_empty() {
                start_ids = ids;
            }
        }
    }

    if let Some(p) = pack {
        let alt = &p.alternatives;
        // R1 - anti-heal: swap the armor-pen slot to the anti-heal item when they have healing.
        if !enemy.healers.is_empty() {
            if let Some(idx) = position_of(&path, &alt.armor_pen, cat) {
                let reason = format!("{} over {}: {} heal{}", alt.anti_heal, p.short(&alt.armor_pen), join(&enemy.healers),
                                     if enemy.healers.len() == 1 { "s" } else { "" });
                if replace(&mut path, idx, cat, pack, &alt.anti_heal, &enemy.healers[0], &reason) {
                    why.push(reason);
                }
            }
        } else if let Some(idx) = position_of(&path, &alt.anti_heal, cat) {
            let reason = format!("{} over {}: no healing on their team", p.short(&alt.armor_pen), p.short(&alt.anti_heal));
            if replace(&mut path, idx, cat, pack, &alt.armor_pen, "no healing", &reason) {
                why.push(reason);
            }
        }
    }

    // R2 - two or more tanks: armor pen one slot earlier.
    let mut pen_moved = false;
    if enemy.tanks.len() >= 2 {
        if let Some(idx) = path.iter().position(|p| p.role == "armor_pen") {
            if move_up(&mut path, idx) {
                pen_moved = true;
                let reason = format!("{} both build armor: armor pen earlier", join(&enemy.tanks[..2]));
                if path[idx - 1].tag.is_none() {
                    path[idx - 1].tag = Some(enemy.tanks[0].clone());
                }
                why.push(reason);
            }
        }
    }

    let mut defensive_replaced = false;
    if let Some(p) = pack {
        let alt = &p.alternatives;
        // R3 - lockdown ult: the defensive slot becomes the cleanse item.
        if let Some(champ) = enemy.lockdown.first() {
            let idx = path.iter().position(|p| p.role == "defensive").unwrap_or(path.len().saturating_sub(1));
            if !path.is_empty() {
                let reason = format!("{} cleanses {}'s ult", p.short(&alt.cleanse), champ);
                if replace(&mut path, idx, cat, pack, &alt.cleanse, champ, &reason) {
                    path[idx].role = "defensive".to_string();
                    defensive_replaced = true;
                    why.push(reason);
                }
            }
        }
        // R5 - mostly magic damage: Maw instead of GA (unless the slot is already the cleanse item).
        if !defensive_replaced && enemy.ap > enemy.ad {
            if let Some(idx) = position_of(&path, &alt.defensive_ad, cat) {
                let reason = format!("{} over {}: mostly magic damage", p.short(&alt.defensive_ap), p.short(&alt.defensive_ad));
                if replace(&mut path, idx, cat, pack, &alt.defensive_ap, "AP comp", &reason) {
                    defensive_replaced = true;
                    why.push(reason);
                }
            }
        }
        // R6 - poke lane without assassins: sustain instead of GA.
        if !defensive_replaced && enemy.poke.len() >= 2 && enemy.assassins.is_empty() {
            if let Some(idx) = position_of(&path, &alt.defensive_ad, cat) {
                let reason = format!("{} for sustain: {} poke", p.short(&alt.sustain), join(&enemy.poke));
                if replace(&mut path, idx, cat, pack, &alt.sustain, &enemy.poke[0], &reason) {
                    why.push(reason);
                }
            }
        }
    }

    // Live-only rules.
    let me = inp.live.and_then(|l| l.me.as_ref());
    let mut dive_threat: Option<String> = if enemy.assassins.len() >= 2 { Some(join(&enemy.assassins)) } else { None };
    if let Some(live) = inp.live {
        for p in path.iter_mut() {
            p.owned = me.map(|m| m.player.has_item(p.id)).unwrap_or(false);
        }
        if dive_threat.is_none() {
            dive_threat = fed_assassin(&enemy, &live.enemies).map(|c| format!("{c} is fed"));
        }
        // R8 - an enemy stacking armor: armor pen earlier (if R2 did not already).
        if !pen_moved {
            if let Some(champ) = armor_stacker(cat, &live.enemies) {
                if let Some(idx) = path.iter().position(|p| p.role == "armor_pen" && !p.owned) {
                    if idx > 0 && !path[idx - 1].owned && move_up(&mut path, idx) {
                        path[idx - 1].tag = Some(champ.clone());
                        why.push(format!("{champ} is stacking armor: armor pen earlier"));
                    }
                }
            }
        }
        // R7 - behind: cheaper spike first (Navori before IE) among items not yet owned (pack rule).
        if let (Some(m), Some(_)) = (me, pack) {
            if m.player.deaths >= 3 && m.player.kills <= 1 {
                let ie = position_of(&path, "Infinity Edge", cat);
                let navori = position_of(&path, "Navori Flickerblade", cat);
                if let (Some(a), Some(b)) = (ie, navori) {
                    if a < b && !path[a].owned && !path[b].owned {
                        path.swap(a, b);
                        why.push(format!("behind ({}): Navori before IE for a cheaper spike", m.player.kda()));
                    }
                }
            }
        }
    }

    // R4 - assassins (two of them, or one that is fed): defensive item at slot 4.
    if let Some(threat) = dive_threat {
        if let Some(idx) = path.iter().position(|p| p.role == "defensive") {
            let target = 3.min(path.len().saturating_sub(1));
            if idx > target && !path[idx].owned {
                let item = path.remove(idx);
                path.insert(target, item);
                why.push(format!("{threat}: defensive item earlier"));
            }
        }
    }

    // Why this core order, when it is not simply the most-picked one.
    if let Some(a) = agg {
        if !a.core_most_picked.ids.is_empty() && a.core.ids != a.core_most_picked.ids {
            let order: Vec<String> = a.core.ids.iter().map(|&id| short_of(pack, &cat.item_name(id))).collect();
            why.push(format!(
                "{}: {:.0}% win rate vs {:.0}% for the most-picked order ({} games)",
                order.join(" > "),
                Aggregate::win_rate_of(&a.core) * 100.0,
                Aggregate::win_rate_of(&a.core_most_picked) * 100.0,
                a.core.games
            ));
        }
    }

    // The trinket is free; the start block should still show it.
    if let Some(ward) = cat.item_id("Stealth Ward") {
        if !start_ids.contains(&ward) && !start_ids.is_empty() {
            start_ids.push(ward);
        }
    }
    let start: Vec<PlanItem> = start_ids
        .iter()
        .filter_map(|&id| item_by_id(cat, pack, id, "start", None))
        .map(|mut i| {
            i.owned = me.map(|m| m.player.has_item(i.id)).unwrap_or(false);
            i
        })
        .collect();
    // Magical Footwear: say so on the boots slot, and do not point at boots while they are locked.
    let footwear = runes_page.as_ref().map(|r| r.perks.contains(&MAGICAL_FOOTWEAR_PERK)).unwrap_or(false);
    let mut boots_locked = false;
    if footwear {
        if let Some(b) = path.iter_mut().find(|p| p.role == "boots") {
            b.tag = Some(FOOTWEAR_TAG.to_string());
            b.why = Some("Magical Footwear: free boots at 12:00 (45 s sooner per takedown), then upgrade them".to_string());
        }
        if let Some(m) = me {
            let has_boots = m.player.items.iter().any(|i| i.id == SLIGHTLY_MAGICAL_FOOTWEAR || has_tag(cat, i.id, "Boots"));
            let boots_pending = path.iter().any(|p| p.role == "boots" && !p.owned);
            if !has_boots && boots_pending {
                boots_locked = true;
                why.insert(0, "Boots are locked until Magical Footwear delivers them (12:00, sooner with takedowns)".to_string());
            }
        }
    }
    let options = options(cat, pack, agg, &path);
    let next = next_item(cat, pack, &path, me, boots_locked);
    let (seq, label) = match (agg.filter(|a| !a.skill_order.is_empty()), pack) {
        (Some(a), _) => (sequence(&a.skill_order, &a.skill_max), skill_label(&a.skill_max)),
        (None, Some(p)) => (skill_sequence(&p.skill_order), p.skill_order.label.clone()),
        (None, None) => (Vec::new(), String::new()),
    };
    let skill = skill_plan(&seq, &label, me);
    let spells: Vec<String> = spell_ids
        .iter()
        .map(|&id| runes::spell_name(id).map(str::to_string).unwrap_or_else(|| format!("spell {id}")))
        .collect();
    let runes_summary = runes_page
        .as_ref()
        .and_then(|r| r.perks.first().map(|&k| format!("{} / {}", cat.rune_name(k), cat.style_name(r.sub_style))))
        .unwrap_or_default();

    Plan {
        champion: inp.champion.to_string(),
        position: agg.map(|a| a.position.label().to_string()),
        source: agg.map(|a| format!("{}, patch {}", a.describe(), a.patch)),
        note: agg.and_then(|a| {
            a.requested_position
                .map(|r| format!("{} is rarely played {}: showing the {} build", inp.champion, r.label(), a.position.label()))
        }),
        start,
        path,
        options,
        next,
        skill,
        why,
        matchup: matchup.map(|m| m.line.clone()).or_else(|| counter_line(inp, agg, opponent.as_deref())),
        matchup_champion: opponent,
        runes: runes_page,
        runes_summary,
        spells,
        spell_ids,
        enemy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::decode;
    use crate::ddragon::test_support::catalog;
    use crate::live::summarize;
    use crate::pack::{load_traits, load_xayah};

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn short_path(plan: &Plan) -> Vec<String> {
        plan.path.iter().map(|p| p.short.clone()).collect()
    }

    fn xayah_aggregate() -> Aggregate {
        let v: serde_json::Value = serde_json::from_str(include_str!("../../../m0/tests/fixtures/opgg_xayah_adc.json")).unwrap();
        decode(&v, 498, Position::Adc, "global", "emerald_plus").unwrap()
    }

    #[test]
    fn pack_only_path_without_enemies() {
        let (cat, pack, traits) = (catalog(), load_xayah().unwrap(), load_traits().unwrap());
        let plan = plan(&Inputs { champion: "Xayah", pack: Some(&pack), aggregate: None, traits: &traits, catalog: &cat, enemies: &[], live: None });
        assert_eq!(short_path(&plan), vec!["ER", "Greaves", "IE", "Navori", "LDR", "GA"]);
        assert!(plan.why.is_empty());
        let next = plan.next.unwrap();
        assert_eq!(next.name, "Essence Reaver");
        assert_eq!(next.components.iter().map(|c| c.id).collect::<Vec<_>>(), vec![3057, 3133, 1018]);
        assert_eq!(plan.skill.next, Some('Q'));
        assert_eq!(plan.start.iter().map(|s| s.id).collect::<Vec<_>>(), vec![1055, 2003, 3340]);
        assert_eq!(plan.spells, vec!["Flash", "Barrier"]);
        assert!(plan.source.is_none());
    }

    #[test]
    fn aggregate_is_the_base_and_the_pack_fills_the_late_slots() {
        let (cat, pack, traits, agg) = (catalog(), load_xayah().unwrap(), load_traits().unwrap(), xayah_aggregate());
        let plan = plan(&Inputs { champion: "Xayah", pack: Some(&pack), aggregate: Some(&agg), traits: &traits, catalog: &cat, enemies: &[], live: None });
        // Core in the order that wins more (IE before Navori), boots second, the pack's LDR + GA after.
        assert_eq!(short_path(&plan), vec!["Yun Tal", "Greaves", "IE", "Navori", "LDR", "GA"]);
        assert_eq!(plan.start.iter().map(|s| s.id).collect::<Vec<_>>(), vec![1086, 2003, 2003, 3340], "Doran's Bow start + trinket");
        assert_eq!(plan.spells, vec!["Flash", "Barrier"]);
        assert_eq!(plan.spell_ids, vec![4, 21]);
        assert_eq!(plan.runes.as_ref().unwrap().perks[0], 8008);
        assert_eq!(plan.skill.next, Some('Q'));
        assert_eq!(plan.skill.label, "max E > W > Q");
        assert_eq!(plan.position.as_deref(), Some("ADC"));
        assert!(plan.source.as_deref().unwrap().starts_with("op.gg emerald+ global"), "{:?}", plan.source);
        assert!(plan.note.is_none());
        assert_eq!(plan.why.len(), 1, "{:?}", plan.why);
        assert!(plan.why[0].starts_with("Yun Tal > IE > Navori: 60% win rate vs 57%"), "{:?}", plan.why);
        assert_eq!(plan.next.unwrap().name, "Yun Tal Wildarrows");
        assert!(plan.options.iter().all(|o| !plan.path.iter().any(|p| p.id == o.id)));
    }

    #[test]
    fn rules_still_apply_on_top_of_the_aggregate() {
        let (cat, pack, traits, agg) = (catalog(), load_xayah().unwrap(), load_traits().unwrap(), xayah_aggregate());
        let enemies = names(&["Tristana", "Soraka", "Malphite", "Ornn", "Thresh"]);
        let plan = plan(&Inputs { champion: "Xayah", pack: Some(&pack), aggregate: Some(&agg), traits: &traits, catalog: &cat, enemies: &enemies, live: None });
        assert_eq!(short_path(&plan), vec!["Yun Tal", "Greaves", "IE", "Mortal", "Navori", "GA"]);
        assert!(plan.why.iter().any(|w| w.contains("Soraka heals")), "{:?}", plan.why);
        assert!(plan.why.iter().any(|w| w.contains("both build armor")), "{:?}", plan.why);
        assert_eq!(plan.matchup_champion.as_deref(), Some("Tristana"));
        assert!(plan.matchup.as_deref().unwrap().starts_with("vs Tristana: loses"), "the pack's line wins over the counter stats");
    }

    #[test]
    fn counters_give_a_matchup_line_without_a_pack() {
        let (cat, traits, agg) = (catalog(), load_traits().unwrap(), xayah_aggregate());
        let enemies = names(&["Tristana", "Thresh"]);
        let plan = plan(&Inputs { champion: "Xayah", pack: None, aggregate: Some(&agg), traits: &traits, catalog: &cat, enemies: &enemies, live: None });
        assert_eq!(plan.matchup_champion.as_deref(), Some("Tristana"));
        assert_eq!(plan.matchup.as_deref(), Some("vs Tristana: Xayah wins 51% of these lanes at this rank (343 games)"));
    }

    #[test]
    fn aggregate_without_a_pack_fills_from_popular_items() {
        let (cat, traits, agg) = (catalog(), load_traits().unwrap(), xayah_aggregate());
        let plan = plan(&Inputs { champion: "Tristana", pack: None, aggregate: Some(&agg), traits: &traits, catalog: &cat, enemies: &[], live: None });
        // core + boots, then finished items by pick rate: LDR (armor pen), then no second armor-pen item.
        assert_eq!(short_path(&plan)[..4], ["Yun Tal", "Greaves", "IE", "Navori"]);
        assert_eq!(plan.path.len(), 6);
        assert_eq!(plan.path.iter().filter(|p| p.role == "armor_pen").count(), 1);
        assert!(!plan.path.iter().any(|p| p.id == 3508), "Essence Reaver is a core alternative, not a late item");
        assert!(!plan.path.iter().any(|p| p.id == 1038), "components never enter the path");
        assert_eq!(plan.spells, vec!["Flash", "Barrier"]);
        assert_eq!(plan.champion, "Tristana");
        assert!(plan.matchup.is_none());
    }

    #[test]
    fn healer_swaps_ldr_for_mortal_reminder() {
        let (cat, pack, traits) = (catalog(), load_xayah().unwrap(), load_traits().unwrap());
        let enemies = names(&["Tristana", "Soraka", "Malphite", "Ornn", "Thresh"]);
        let plan = plan(&Inputs { champion: "Xayah", pack: Some(&pack), aggregate: None, traits: &traits, catalog: &cat, enemies: &enemies, live: None });
        // Soraka -> Mortal Reminder; Malphite + Ornn -> armor pen moves up one slot
        assert_eq!(short_path(&plan), vec!["ER", "Greaves", "IE", "Mortal", "Navori", "GA"]);
        assert!(plan.why.iter().any(|w| w.contains("Soraka heals")), "{:?}", plan.why);
        assert!(plan.why.iter().any(|w| w.contains("both build armor")), "{:?}", plan.why);
        assert_eq!(plan.path[3].tag.as_deref(), Some("Soraka"));
        assert_eq!(plan.matchup_champion.as_deref(), Some("Tristana"));
        assert!(plan.matchup.unwrap().contains("Tristana"));
    }

    #[test]
    fn lockdown_ult_puts_mercurial_in_the_defensive_slot() {
        let (cat, pack, traits) = (catalog(), load_xayah().unwrap(), load_traits().unwrap());
        let enemies = names(&["Malzahar", "Ezreal"]);
        let plan = plan(&Inputs { champion: "Xayah", pack: Some(&pack), aggregate: None, traits: &traits, catalog: &cat, enemies: &enemies, live: None });
        assert_eq!(plan.path.last().unwrap().short, "Merc");
        assert!(plan.why.iter().any(|w| w.contains("Malzahar")));
    }

    #[test]
    fn ashe_matchup_switches_to_cleanse_with_a_reason() {
        let (cat, pack, traits, agg) = (catalog(), load_xayah().unwrap(), load_traits().unwrap(), xayah_aggregate());
        let enemies = names(&["Ashe", "Thresh"]);
        let plan = plan(&Inputs { champion: "Xayah", pack: Some(&pack), aggregate: Some(&agg), traits: &traits, catalog: &cat, enemies: &enemies, live: None });
        assert_eq!(plan.spells, vec!["Flash", "Cleanse"]);
        assert!(plan.why.iter().any(|w| w.contains("Cleanse") && w.contains("Ashe")), "{:?}", plan.why);
    }

    #[test]
    fn live_state_marks_owned_and_computes_next() {
        let (cat, pack, traits) = (catalog(), load_xayah().unwrap(), load_traits().unwrap());
        let mut data: serde_json::Value = serde_json::from_str(include_str!("../../../m0/tests/fixtures/allgamedata.json")).unwrap();
        // Give Xayah a finished Essence Reaver, boots, a B. F. Sword and 900 gold.
        let me = data["allPlayers"].as_array_mut().unwrap().iter_mut().find(|p| p["riotId"] == "matteso#NA1").unwrap();
        me["items"] = serde_json::json!([
            {"itemID": 3508, "displayName": "Essence Reaver", "count": 1, "slot": 0},
            {"itemID": 3006, "displayName": "Berserker's Greaves", "count": 1, "slot": 1},
            {"itemID": 1038, "displayName": "B. F. Sword", "count": 1, "slot": 2}
        ]);
        me["level"] = serde_json::json!(7);
        data["activePlayer"]["currentGold"] = serde_json::json!(900.0);
        data["activePlayer"]["level"] = serde_json::json!(7);
        for (k, lvl) in [("Q", 1), ("W", 1), ("E", 3), ("R", 1)] {
            data["activePlayer"]["abilities"][k]["abilityLevel"] = serde_json::json!(lvl);
        }
        let live = summarize(&data);
        let enemies: Vec<String> = live.enemies.iter().map(|p| p.champion.clone()).collect();
        let plan = plan(&Inputs { champion: "Xayah", pack: Some(&pack), aggregate: None, traits: &traits, catalog: &cat, enemies: &enemies, live: Some(&live) });
        assert!(plan.path[0].owned && plan.path[1].owned && !plan.path[2].owned);
        let next = plan.next.unwrap();
        assert_eq!(next.name, "Infinity Edge");
        assert_eq!(next.remaining_cost, 3500 - 1300);
        let owned: Vec<bool> = next.components.iter().map(|c| c.owned).collect();
        assert_eq!(owned, vec![true, false, false]);
        assert_eq!(next.buy_now.unwrap().name, "Pickaxe"); // 875 <= 900
        assert!(next.buy_now_affordable);
        // 6 points spent at level 7 -> a point is available; Q E W E R E -> next is E
        assert!(plan.skill.point_available);
        assert_eq!(plan.skill.next, Some('E'));
    }

    #[test]
    fn magical_footwear_locks_boots_until_they_arrive() {
        let (cat, pack, traits, agg) = (catalog(), load_xayah().unwrap(), load_traits().unwrap(), xayah_aggregate());
        let mut data: serde_json::Value = serde_json::from_str(include_str!("../../../m0/tests/fixtures/allgamedata.json")).unwrap();
        let me = data["allPlayers"].as_array_mut().unwrap().iter_mut().find(|p| p["riotId"] == "matteso#NA1").unwrap();
        me["items"] = serde_json::json!([{"itemID": 3032, "displayName": "Yun Tal Wildarrows", "count": 1, "slot": 0}]);
        data["activePlayer"]["currentGold"] = serde_json::json!(900.0);
        let live = summarize(&data);
        let p = plan(&Inputs { champion: "Xayah", pack: Some(&pack), aggregate: Some(&agg), traits: &traits, catalog: &cat, enemies: &[], live: Some(&live) });
        // The op.gg page has Magical Footwear: the boots slot says so, and NEXT skips it while no boots are owned.
        assert_eq!(p.path[1].tag.as_deref(), Some(FOOTWEAR_TAG));
        assert_eq!(p.next.as_ref().unwrap().name, "Infinity Edge");
        assert!(p.why[0].contains("Magical Footwear"), "{:?}", p.why);
        // The free footwear arrives: boots are the next item again, and the footwear counts as the Boots component.
        let me = data["allPlayers"].as_array_mut().unwrap().iter_mut().find(|p| p["riotId"] == "matteso#NA1").unwrap();
        me["items"].as_array_mut().unwrap().push(serde_json::json!({"itemID": 2422, "displayName": "Slightly Magical Footwear", "count": 1, "slot": 1}));
        let live = summarize(&data);
        let p = plan(&Inputs { champion: "Xayah", pack: Some(&pack), aggregate: Some(&agg), traits: &traits, catalog: &cat, enemies: &[], live: Some(&live) });
        let next = p.next.unwrap();
        assert_eq!(next.name, "Berserker's Greaves");
        assert_eq!(next.remaining_cost, 1100 - 300);
        assert!(next.components.iter().any(|c| c.id == 1001 && c.owned));
        assert!(!p.why.iter().any(|w| w.contains("locked")), "{:?}", p.why);
        // Without live data nothing is locked and NEXT is the first item.
        let p = plan(&Inputs { champion: "Xayah", pack: Some(&pack), aggregate: Some(&agg), traits: &traits, catalog: &cat, enemies: &[], live: None });
        assert_eq!(p.next.unwrap().name, "Yun Tal Wildarrows");
    }

    #[test]
    fn skill_sequence_follows_first_four_then_max_order() {
        let pack = load_xayah().unwrap();
        let seq: String = skill_sequence(&pack.skill_order).into_iter().collect();
        assert_eq!(seq, "QEWEEREEWWRWWQQRQQ");
        // The aggregate's 15-level order is completed with its max priority; R stays at 6/11/16.
        let agg = xayah_aggregate();
        let seq: String = sequence(&agg.skill_order, &agg.skill_max).into_iter().collect();
        assert_eq!(seq, "QEWEEREWEWRWWQQRQQ");
        assert_eq!(skill_label(&agg.skill_max), "max E > W > Q");
        assert_eq!(short_of(None, "Yun Tal Wildarrows"), "Yun Tal");
        assert_eq!(short_of(None, "Hubris"), "Hubris");
    }
}
