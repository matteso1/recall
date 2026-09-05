//! The build brain. Explicit, readable rules turn (enemy comp, live state) into an ordered
//! path, the literal "buy this next", the next skill point, and one line of why per change.
use crate::ddragon::{normalize, Catalog};
use crate::live::{LiveSnapshot, Me, Player};
use crate::pack::{ChampionPack, Traits};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct PlanItem {
    pub id: u32,
    pub name: String,
    pub short: String,
    pub cost: u32,
    pub owned: bool,
    /// damage | boots | armor_pen | defensive
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
    pub start: Vec<PlanItem>,
    pub path: Vec<PlanItem>,
    pub next: Option<NextItem>,
    pub skill: SkillPlan,
    /// One line per change, most important first
    pub why: Vec<String>,
    pub matchup: Option<String>,
    pub matchup_champion: Option<String>,
    pub runes_summary: String,
    pub spells: Vec<String>,
    pub enemy: EnemyProfile,
}

pub struct Inputs<'a> {
    pub pack: &'a ChampionPack,
    pub traits: &'a Traits,
    pub catalog: &'a Catalog,
    /// Enemy champion display names
    pub enemies: &'a [String],
    pub live: Option<&'a LiveSnapshot>,
}

/// Item ids that mean "this enemy is stacking armor".
const ARMOR_ITEM_MIN_COST: u32 = 900;

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

fn make_item(cat: &Catalog, pack: &ChampionPack, name: &str, role: &str, why: Option<String>) -> Option<PlanItem> {
    let id = cat.item_id(name)?;
    Some(PlanItem {
        id,
        name: cat.item_name(id),
        short: pack.short(name),
        cost: cat.item_cost(id),
        owned: false,
        role: role.to_string(),
        why,
        tag: None,
    })
}

fn position_of(path: &[PlanItem], name: &str, cat: &Catalog) -> Option<usize> {
    let id = cat.item_id(name)?;
    path.iter().position(|p| p.id == id)
}

fn replace(path: &mut [PlanItem], idx: usize, cat: &Catalog, pack: &ChampionPack, name: &str, tag: &str, why: &str) -> bool {
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

/// The enemy in our lane: a champion the pack/traits place in our role, else a Marksman for bot.
fn lane_opponent(inp: &Inputs) -> Option<String> {
    let role = inp.pack.role.as_str();
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

pub fn skill_sequence(order: &crate::pack::SkillOrder) -> Vec<char> {
    let key = |s: &String| s.chars().next().unwrap_or('Q').to_ascii_uppercase();
    let idx = |c: char| match c {
        'Q' => 0,
        'W' => 1,
        'E' => 2,
        _ => 3,
    };
    let mut counts = [0u8; 4];
    let mut seq = Vec::with_capacity(18);
    for level in 1..=18usize {
        let ability = if matches!(level, 6 | 11 | 16) {
            'R'
        } else if level <= order.first.len() {
            key(&order.first[level - 1])
        } else {
            order
                .max
                .iter()
                .map(key)
                .find(|&c| c != 'R' && counts[idx(c)] < 5)
                .unwrap_or('Q')
        };
        counts[idx(ability)] += 1;
        seq.push(ability);
    }
    seq
}

pub fn skill_plan(order: &crate::pack::SkillOrder, me: Option<&Me>) -> SkillPlan {
    let seq = skill_sequence(order);
    match me {
        None => SkillPlan { next: seq.first().copied(), point_available: false, label: order.label.clone(), levels: [0; 4] },
        Some(m) => {
            let spent = m.abilities.total() as usize;
            SkillPlan {
                next: seq.get(spent).copied(),
                point_available: (spent as u32) < m.player.level,
                label: order.label.clone(),
                levels: [m.abilities.q, m.abilities.w, m.abilities.e, m.abilities.r],
            }
        }
    }
}

fn component_ids(cat: &Catalog, pack: &ChampionPack, item: &PlanItem) -> Vec<u32> {
    let from_pack = pack
        .core
        .iter()
        .find(|c| normalize(&c.item) == normalize(&item.name))
        .and_then(|c| c.components.as_ref())
        .map(|names| names.iter().filter_map(|n| cat.item_id(n)).collect::<Vec<u32>>());
    match from_pack {
        Some(ids) if !ids.is_empty() => ids,
        _ => cat.components(item.id),
    }
}

pub fn next_item(cat: &Catalog, pack: &ChampionPack, path: &[PlanItem], me: Option<&Me>) -> Option<NextItem> {
    let target = path.iter().find(|p| !p.owned)?;
    let mut inventory: HashMap<u32, u32> = HashMap::new();
    if let Some(m) = me {
        for i in &m.player.items {
            *inventory.entry(i.id).or_insert(0) += i.count;
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

pub fn plan(inp: &Inputs) -> Plan {
    let (cat, pack) = (inp.catalog, inp.pack);
    let alt = &pack.alternatives;
    let enemy = profile(inp.traits, cat, inp.enemies);
    let mut why: Vec<String> = Vec::new();

    // Base path from the pack.
    let mut path: Vec<PlanItem> = pack
        .core
        .iter()
        .filter_map(|c| make_item(cat, pack, &c.item, c.role.as_deref().unwrap_or("damage"), c.why.clone()))
        .collect();

    // Lane matchup: line, optional first-item / start / spell overrides.
    let opponent = lane_opponent(inp);
    let matchup = opponent.as_deref().and_then(|o| pack.matchup(o));
    let mut spells = pack.spells.clone();
    let mut start_names = pack.start.clone();
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
            spells = s.clone();
        }
        if let Some(s) = &m.start {
            start_names = s.clone();
        }
    }

    // R1 - anti-heal: swap the armor-pen slot to the anti-heal item when they have healing.
    if !enemy.healers.is_empty() {
        if let Some(idx) = position_of(&path, &alt.armor_pen, cat) {
            let reason = format!("{} over {}: {} heal{}", alt.anti_heal, pack.short(&alt.armor_pen), join(&enemy.healers),
                                 if enemy.healers.len() == 1 { "s" } else { "" });
            if replace(&mut path, idx, cat, pack, &alt.anti_heal, &enemy.healers[0], &reason) {
                why.push(reason);
            }
        }
    } else if let Some(idx) = position_of(&path, &alt.anti_heal, cat) {
        let reason = format!("{} over {}: no healing on their team", pack.short(&alt.armor_pen), pack.short(&alt.anti_heal));
        if replace(&mut path, idx, cat, pack, &alt.armor_pen, "no healing", &reason) {
            why.push(reason);
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

    // R3 - lockdown ult: the defensive slot becomes the cleanse item.
    let mut defensive_replaced = false;
    if let Some(champ) = enemy.lockdown.first() {
        let idx = path.iter().position(|p| p.role == "defensive").unwrap_or(path.len().saturating_sub(1));
        if !path.is_empty() {
            let reason = format!("{} cleanses {}'s ult", pack.short(&alt.cleanse), champ);
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
            let reason = format!("{} over {}: mostly magic damage", pack.short(&alt.defensive_ap), pack.short(&alt.defensive_ad));
            if replace(&mut path, idx, cat, pack, &alt.defensive_ap, "AP comp", &reason) {
                defensive_replaced = true;
                why.push(reason);
            }
        }
    }

    // R6 - poke lane without assassins: sustain instead of GA.
    if !defensive_replaced && enemy.poke.len() >= 2 && enemy.assassins.is_empty() {
        if let Some(idx) = position_of(&path, &alt.defensive_ad, cat) {
            let reason = format!("{} for sustain: {} poke", pack.short(&alt.sustain), join(&enemy.poke));
            if replace(&mut path, idx, cat, pack, &alt.sustain, &enemy.poke[0], &reason) {
                why.push(reason);
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
        // R7 - behind: cheaper spike first (Navori before IE) among items not yet owned.
        if let Some(m) = me {
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

    let start: Vec<PlanItem> = start_names
        .iter()
        .filter_map(|n| make_item(cat, pack, n, "start", None))
        .map(|mut i| {
            i.owned = me.map(|m| m.player.has_item(i.id)).unwrap_or(false);
            i
        })
        .collect();
    let next = next_item(cat, pack, &path, me);
    let skill = skill_plan(&pack.skill_order, me);

    Plan {
        champion: pack.champion.clone(),
        start,
        path,
        next,
        skill,
        why,
        matchup: matchup.map(|m| m.line.clone()),
        matchup_champion: opponent,
        runes_summary: format!("{} / {}", pack.runes.keystone, pack.runes.secondary),
        spells,
        enemy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ddragon::test_support::catalog;
    use crate::live::summarize;
    use crate::pack::{load_traits, load_xayah};

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn short_path(plan: &Plan) -> Vec<String> {
        plan.path.iter().map(|p| p.short.clone()).collect()
    }

    #[test]
    fn base_path_without_enemies() {
        let (cat, pack, traits) = (catalog(), load_xayah().unwrap(), load_traits().unwrap());
        let plan = plan(&Inputs { pack: &pack, traits: &traits, catalog: &cat, enemies: &[], live: None });
        assert_eq!(short_path(&plan), vec!["ER", "Greaves", "IE", "Navori", "LDR", "GA"]);
        assert!(plan.why.is_empty());
        let next = plan.next.unwrap();
        assert_eq!(next.name, "Essence Reaver");
        assert_eq!(next.components.iter().map(|c| c.id).collect::<Vec<_>>(), vec![3057, 3133, 1018]);
        assert_eq!(plan.skill.next, Some('Q'));
        assert_eq!(plan.start.iter().map(|s| s.id).collect::<Vec<_>>(), vec![1055, 2003, 3340]);
    }

    #[test]
    fn healer_swaps_ldr_for_mortal_reminder() {
        let (cat, pack, traits) = (catalog(), load_xayah().unwrap(), load_traits().unwrap());
        let enemies = names(&["Tristana", "Soraka", "Malphite", "Ornn", "Thresh"]);
        let plan = plan(&Inputs { pack: &pack, traits: &traits, catalog: &cat, enemies: &enemies, live: None });
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
        let plan = plan(&Inputs { pack: &pack, traits: &traits, catalog: &cat, enemies: &enemies, live: None });
        assert_eq!(plan.path.last().unwrap().short, "Merc");
        assert!(plan.why.iter().any(|w| w.contains("Malzahar")));
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
        let plan = plan(&Inputs { pack: &pack, traits: &traits, catalog: &cat, enemies: &enemies, live: Some(&live) });
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
    fn skill_sequence_follows_first_four_then_max_order() {
        let pack = load_xayah().unwrap();
        let seq: String = skill_sequence(&pack.skill_order).into_iter().collect();
        assert_eq!(seq, "QEWEEREEWWRWWQQRQQ");
    }
}
