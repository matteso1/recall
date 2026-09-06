//! Pure, inventory-aware purchase quotes for the Summoner's Rift shop.
use crate::ddragon::{normalize, Catalog, Item};
use crate::live::InvItem;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const EQUIPMENT_SLOTS: u32 = 6;
const MAX_RECIPE_DEPTH: usize = 32;
const MAX_BASKET_PURCHASES: usize = 6;

/// Player data for purchase restrictions. Missing fields stay unknown.
/// Live purchase quotes must use the actual spell loadout, not a proposed page.
#[derive(Clone, Copy, Debug, Default)]
pub struct ShopContext<'a> {
    pub champion: Option<&'a str>,
    pub spell_ids: Option<&'a [u32]>,
    pub boots_locked: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShopComponent {
    pub id: u32,
    pub name: String,
    pub cost: u32,
    pub owned: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShopQuote {
    pub remaining_cost: Option<u32>,
    pub components: Vec<ShopComponent>,
    pub buy_now: Option<ShopComponent>,
    #[serde(default)]
    pub save_for: Option<ShopComponent>,
    pub basket: Vec<ShopComponent>,
    pub basket_cost: u32,
    pub affordable: bool,
    pub blocked: Option<String>,
}

/// `affordable` refers to the completed target. `buy_now` and `basket` may instead
/// contain affordable components. Basket entries are ordered shop actions; a later
/// action may consume an earlier purchase. No action implies selling an item.
pub fn quote(
    cat: &Catalog,
    target: u32,
    inventory: &[InvItem],
    gold: f64,
    boots_locked: bool,
) -> ShopQuote {
    quote_with_context(
        cat,
        target,
        inventory,
        gold,
        &ShopContext {
            boots_locked,
            ..Default::default()
        },
    )
}

/// A purchase quote using actual player identity and spells when available.
pub fn quote_with_context(
    cat: &Catalog,
    target: u32,
    inventory: &[InvItem],
    gold: f64,
    context: &ShopContext<'_>,
) -> ShopQuote {
    let mut result = ShopQuote::default();
    let Some(item) = cat.item(target) else {
        result.blocked = Some("Item data is unavailable for this patch".to_string());
        return result;
    };
    let stock = inventory_stock(inventory);
    let allocation = match allocate(cat, target, &stock, item.max_stacks <= 1) {
        Ok(allocation) => allocation,
        Err(reason) => {
            result.blocked = Some(reason);
            return result;
        }
    };
    result.remaining_cost = Some(allocation.root.remaining);
    result.components = allocation
        .root
        .children
        .iter()
        .map(|node| component(cat, node))
        .collect();
    if allocation.root.owned {
        result.blocked = Some("Target is already owned".to_string());
        return result;
    }
    if let Some(reason) = purchase_restriction(cat, item, &allocation.after, context, &stock) {
        result.blocked = Some(reason);
        return result;
    }
    if !gold.is_finite() || gold < 0.0 {
        result.blocked = Some("Current gold is unavailable".to_string());
        return result;
    }
    let target_fits = add_purchase(cat, item, &allocation.after).is_some();
    if target_fits && f64::from(allocation.root.remaining) <= gold {
        result.affordable = true;
        result.basket_cost = allocation.root.remaining;
        let purchase = component(cat, &allocation.root);
        result.buy_now = Some(purchase.clone());
        result.basket.push(purchase);
        return result;
    }
    if target_fits {
        result.save_for = Some(component(cat, &allocation.root));
    }

    let mut simulated = stock;
    let mut available_gold = gold;
    for _ in 0..MAX_BASKET_PURCHASES {
        let Ok(current) = allocate(cat, target, &simulated, true) else {
            break;
        };
        let mut ids = BTreeSet::new();
        for child in &current.root.children {
            missing_candidates(child, &mut ids);
        }
        let mut choices = Vec::new();
        for id in ids {
            let Some(candidate) = cat.item(id) else {
                continue;
            };
            // Buying another required Dagger must not treat an already allocated
            // Dagger as the newly purchased root. Descendants still receive credit.
            let Ok(parts) = allocate(cat, id, &simulated, false) else {
                continue;
            };
            let cost = parts.root.remaining;
            if purchase_restriction(cat, candidate, &parts.after, context, &simulated).is_some() {
                continue;
            }
            let Some(after) = add_purchase(cat, candidate, &parts.after) else {
                continue;
            };
            let Ok(progress) = allocate(cat, target, &after, true) else {
                continue;
            };
            // Reallocation prevents overbuying a repeated component or consuming
            // an owned branch without making equal-cost progress toward this target.
            if current.root.remaining.checked_sub(progress.root.remaining) != Some(cost)
                || cost == 0
            {
                continue;
            }
            if result.basket.is_empty()
                && result
                    .save_for
                    .as_ref()
                    .is_none_or(|saving| (cost, candidate.id) < (saving.cost, saving.id))
            {
                result.save_for = Some(component(cat, &parts.root));
            }
            if f64::from(cost) > available_gold {
                continue;
            }
            let consumed_power: f64 = parts
                .root
                .consumed
                .iter()
                .filter_map(|index| cat.item(simulated[*index].id).map(immediate_power))
                .sum();
            choices.push(Purchase {
                component: component(cat, &parts.root),
                complete_upgrade: !candidate.from.is_empty() && cost == candidate.base,
                stat_gain: immediate_power(candidate) - consumed_power,
                after,
            });
        }
        choices.sort_by(|a, b| {
            b.complete_upgrade
                .cmp(&a.complete_upgrade)
                .then_with(|| b.stat_gain.total_cmp(&a.stat_gain))
                .then_with(|| a.component.cost.cmp(&b.component.cost))
                .then_with(|| a.component.id.cmp(&b.component.id))
        });
        let Some(purchase) = choices.into_iter().next() else {
            break;
        };
        available_gold -= f64::from(purchase.component.cost);
        result.basket_cost += purchase.component.cost;
        result.basket.push(purchase.component);
        simulated = purchase.after;
    }
    result.buy_now = result.basket.first().cloned();
    if result.buy_now.is_some() {
        result.save_for = None;
    }
    if result.basket.is_empty() && !target_fits {
        result.blocked =
            Some("No inventory slot is available after combining owned components".to_string());
        result.save_for = None;
    }
    result
}

/// Equipment compatibility after consuming the candidate's recipe. This ignores
/// current gold and slot capacity so callers can compare eventual legal targets.
pub fn compatible(cat: &Catalog, candidate: u32, inventory_ids: &[u32]) -> bool {
    compatible_with_context(cat, candidate, inventory_ids, &ShopContext::default())
}

/// Checks restrictions with the given loadout. During pre-game planning this may
/// describe the planned loadout; live affordability must use observed spells.
pub fn compatible_with_context(
    cat: &Catalog,
    candidate: u32,
    inventory_ids: &[u32],
    context: &ShopContext<'_>,
) -> bool {
    let Some(item) = cat.item(candidate) else {
        return false;
    };
    let stock: Vec<StockItem> = inventory_ids
        .iter()
        .enumerate()
        .filter(|(_, id)| **id != 0)
        .map(|(slot, &id)| StockItem {
            id,
            count: 1,
            slot: slot as u32,
        })
        .collect();
    let Ok(parts) = allocate(cat, candidate, &stock, false) else {
        return false;
    };
    purchase_restriction(cat, item, &parts.after, context, &stock).is_none()
}

#[derive(Clone, Debug)]
struct StockItem {
    id: u32,
    count: u32,
    slot: u32,
}

struct RecipeNode {
    id: u32,
    remaining: u32,
    owned: bool,
    children: Vec<RecipeNode>,
    consumed: Vec<usize>,
}

struct Allocation {
    root: RecipeNode,
    after: Vec<StockItem>,
}

struct Purchase {
    component: ShopComponent,
    complete_upgrade: bool,
    stat_gain: f64,
    after: Vec<StockItem>,
}

fn inventory_stock(inventory: &[InvItem]) -> Vec<StockItem> {
    let mut stock: Vec<_> = inventory
        .iter()
        .filter(|item| item.id != 0)
        .map(|item| StockItem {
            id: item.id,
            count: item.count.max(1),
            slot: item.slot,
        })
        .collect();
    stock.sort_by_key(|item| (item.slot, item.id));
    stock
}

fn recipe_equivalent(cat: &Catalog, owned: u32, required: u32) -> bool {
    if owned == required || (owned == 2422 && required == 1001) {
        return true;
    }
    let mut predecessor = cat.item(owned).and_then(|item| item.special_recipe);
    for _ in 0..MAX_RECIPE_DEPTH {
        let Some(id) = predecessor else { break };
        if id == required {
            return true;
        }
        predecessor = cat.item(id).and_then(|item| item.special_recipe);
    }
    false
}

fn allocate(
    cat: &Catalog,
    target: u32,
    stock: &[StockItem],
    match_root: bool,
) -> Result<Allocation, String> {
    let mut after = stock.to_vec();
    let root = allocate_node(cat, target, &mut after, match_root, &mut Vec::new())?;
    Ok(Allocation { root, after })
}

fn allocate_node(
    cat: &Catalog,
    id: u32,
    remaining_stock: &mut [StockItem],
    may_match: bool,
    path: &mut Vec<u32>,
) -> Result<RecipeNode, String> {
    let item = cat
        .item(id)
        .ok_or_else(|| format!("Recipe data is unavailable for item {id}"))?;
    if !item.price_known {
        return Err(format!("The price of {} is unavailable", item.name));
    }
    if path.contains(&id) || path.len() >= MAX_RECIPE_DEPTH {
        return Err("Item recipe data is inconsistent".to_string());
    }
    if may_match {
        if let Some(index) = remaining_stock
            .iter()
            .position(|owned| owned.count > 0 && recipe_equivalent(cat, owned.id, id))
        {
            remaining_stock[index].count -= 1;
            return Ok(RecipeNode {
                id,
                remaining: 0,
                owned: true,
                children: Vec::new(),
                consumed: vec![index],
            });
        }
    }
    path.push(id);
    let mut children = Vec::new();
    let mut consumed = Vec::new();
    let mut remaining = item.base;
    let mut total = u64::from(item.base);
    for component_id in &item.from {
        let child = allocate_node(cat, *component_id, remaining_stock, true, path)?;
        total += u64::from(cat.item(*component_id).unwrap().total);
        remaining = remaining
            .checked_add(child.remaining)
            .ok_or_else(|| "Item recipe price exceeds supported range".to_string())?;
        consumed.extend_from_slice(&child.consumed);
        children.push(child);
    }
    path.pop();
    if total != u64::from(item.total) {
        return Err(format!(
            "Recipe prices for {} do not match this patch",
            item.name
        ));
    }
    Ok(RecipeNode {
        id,
        remaining,
        owned: false,
        children,
        consumed,
    })
}

fn component(cat: &Catalog, node: &RecipeNode) -> ShopComponent {
    ShopComponent {
        id: node.id,
        name: cat.item_name(node.id),
        cost: node.remaining,
        owned: node.owned,
    }
}

fn missing_candidates(node: &RecipeNode, ids: &mut BTreeSet<u32>) {
    if !node.owned {
        ids.insert(node.id);
        for child in &node.children {
            missing_candidates(child, ids);
        }
    }
}

fn purchase_restriction(
    cat: &Catalog,
    item: &Item,
    after_consumption: &[StockItem],
    context: &ShopContext<'_>,
    original: &[StockItem],
) -> Option<String> {
    if !item.on_sr {
        return Some("This item is unavailable on Summoner's Rift".to_string());
    }
    if !item.purchasable || !item.in_store {
        return Some("This item cannot be purchased from the shop".to_string());
    }
    if let Some(required) = item.required_champion.as_deref() {
        if !context
            .champion
            .is_some_and(|champion| champion_matches(cat, champion, required))
        {
            return Some(format!("This item requires a verified {required} identity"));
        }
    }
    if item.required_ally.is_some() {
        return Some("The required allied champion has not been verified".to_string());
    }
    let groups = item.exclusive_groups();
    let support_choice_unlocked = groups.contains(&"SupportQuest")
        && item.from == [3867]
        && original
            .iter()
            .any(|owned| owned.count > 0 && owned.id == 3867);
    if item.base == 0 && !item.from.is_empty() && !support_choice_unlocked {
        return Some(
            "This special upgrade requires an unlock that has not been verified".to_string(),
        );
    }
    if groups.contains(&"JungleCompanion")
        && !context.spell_ids.is_some_and(|ids| ids.contains(&11))
    {
        return Some("A jungle companion requires a verified Smite loadout".to_string());
    }
    if context.boots_locked
        && item.effects.boots
        && !original.iter().any(|owned| {
            owned.count > 0 && cat.item(owned.id).is_some_and(|item| item.effects.boots)
        })
    {
        return Some("Magical Footwear has not arrived yet".to_string());
    }
    for owned in after_consumption.iter().filter(|owned| owned.count > 0) {
        let Some(owned_item) = cat.item(owned.id) else {
            continue;
        };
        if item.is_finished(cat) && (item.id == owned.id || item.name == owned_item.name) {
            return Some("This completed item is already owned".to_string());
        }
    }
    for group in groups
        .iter()
        .copied()
        .chain(item.group_ids.iter().map(String::as_str))
    {
        // Preserve published finite and unlimited group limits. The narrow
        // verified families fill gaps when Data Dragon omits membership/limits.
        let Some(limit) = cat
            .group_limits
            .get(group)
            .copied()
            .or_else(|| groups.contains(&group).then_some(1))
            .filter(|limit| *limit >= 0)
        else {
            continue;
        };
        let count: u64 = after_consumption
            .iter()
            .filter(|owned| {
                cat.item(owned.id).is_some_and(|owned_item| {
                    owned_item.exclusive_groups().contains(&group)
                        || owned_item
                            .group_ids
                            .iter()
                            .any(|owned_group| owned_group == group)
                })
            })
            .map(|owned| u64::from(owned.count))
            .sum();
        if count >= limit as u64 {
            return Some(format!(
                "{} conflicts with an owned {group} item",
                item.name
            ));
        }
    }
    None
}

fn champion_matches(cat: &Catalog, champion: &str, required: &str) -> bool {
    let champion_name = normalize(champion);
    let required_name = normalize(required);
    if champion_name.is_empty() || required_name.is_empty() {
        return false;
    }
    if champion_name == required_name {
        return true;
    }
    match (cat.champion_key(champion), cat.champion_key(required)) {
        (Some(champion), Some(required)) => champion == required,
        _ => false,
    }
}

fn is_trinket(cat: &Catalog, id: u32) -> bool {
    cat.item(id)
        .is_some_and(|item| item.tags.iter().any(|tag| tag == "Trinket"))
}

fn add_purchase(
    cat: &Catalog,
    item: &Item,
    after_consumption: &[StockItem],
) -> Option<Vec<StockItem>> {
    let mut stock = after_consumption.to_vec();
    if is_trinket(cat, item.id) {
        stock.retain(|owned| !is_trinket(cat, owned.id));
        stock.push(StockItem {
            id: item.id,
            count: 1,
            slot: EQUIPMENT_SLOTS,
        });
        return Some(stock);
    }
    if item.max_stacks > 1 {
        if let Some(stack) = stock
            .iter_mut()
            .find(|owned| owned.id == item.id && owned.count > 0)
        {
            if stack.count >= item.max_stacks {
                return None;
            }
            stack.count += 1;
            return Some(stock);
        }
    }
    let occupied: BTreeSet<u32> = stock
        .iter()
        .filter(|owned| {
            owned.count > 0 && owned.slot < EQUIPMENT_SLOTS && !is_trinket(cat, owned.id)
        })
        .map(|owned| owned.slot)
        .collect();
    let slot = (0..EQUIPMENT_SLOTS).find(|slot| !occupied.contains(slot))?;
    stock.push(StockItem {
        id: item.id,
        count: 1,
        slot,
    });
    Some(stock)
}

/// A deterministic, local tie-breaker for parts of the same selected target.
/// These weights value immediate published stats; they do not predict win rate
/// or model unparsed passives. Ready-to-combine upgrades take priority above it.
fn immediate_power(item: &Item) -> f64 {
    [
        ("FlatPhysicalDamageMod", 35.0),
        ("FlatMagicDamageMod", 21.75),
        ("PercentAttackSpeedMod", 2500.0),
        ("FlatCritChanceMod", 4000.0),
        ("FlatArmorMod", 20.0),
        ("FlatSpellBlockMod", 20.0),
        ("FlatHPPoolMod", 2.67),
        ("PercentLifeStealMod", 5500.0),
        ("AbilityHaste", 50.0),
    ]
    .iter()
    .map(|(key, weight)| item.stat(key).unwrap_or(0.0).max(0.0) * weight)
    .sum::<f64>()
        + item.effects.flat_armor_pen.unwrap_or(0.0) * 30.0
        + item.effects.percent_armor_pen.unwrap_or(0.0) * 3000.0
        + item.effects.flat_magic_pen.unwrap_or(0.0) * 30.0
        + item.effects.percent_magic_pen.unwrap_or(0.0) * 3000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ddragon::test_support::catalog;

    fn inventory(ids: &[u32]) -> Vec<InvItem> {
        ids.iter()
            .enumerate()
            .map(|(slot, &id)| InvItem {
                id,
                name: String::new(),
                count: 1,
                slot: slot as u32,
            })
            .collect()
    }

    #[test]
    fn essence_reaver_credits_nested_long_sword() {
        let cat = catalog();
        let q = quote(&cat, 3508, &inventory(&[1036]), 1000.0, false);
        assert_eq!(q.remaining_cost, Some(2700));
        assert_eq!(
            q.components.iter().find(|c| c.id == 3133).unwrap().cost,
            700
        );
        assert!(!q.affordable);
    }

    #[test]
    fn navori_credits_two_daggers_and_nested_cloak_once_each() {
        let cat = catalog();
        let q = quote(&cat, 6675, &inventory(&[1042, 1042, 1018]), 1550.0, false);
        assert_eq!(q.remaining_cost, Some(1550));
        assert!(q.affordable);
        assert_eq!(
            q.buy_now.as_ref().map(|c| (c.id, c.cost)),
            Some((6675, 1550))
        );
    }

    #[test]
    fn infinity_edge_combines_its_three_components_in_a_full_inventory() {
        let cat = catalog();
        let inv = inventory(&[1038, 1037, 1018, 1055, 3006, 2003]);
        let q = quote(&cat, 3031, &inv, 725.0, false);
        assert_eq!(q.remaining_cost, Some(725));
        assert!(q.affordable);
        assert_eq!(
            q.basket.iter().map(|c| c.id).collect::<Vec<_>>(),
            vec![3031]
        );
        assert_eq!(q.basket_cost, 725);
    }

    #[test]
    fn an_owned_parent_does_not_also_credit_its_children() {
        let cat = catalog();
        // Warhammer consumes its own two Long Swords; the extra sword is unrelated.
        let q = quote(&cat, 3508, &inventory(&[3133, 1036]), 3000.0, false);
        assert_eq!(q.remaining_cost, Some(2000));
    }

    #[test]
    fn an_owned_finished_target_is_not_a_zero_gold_purchase() {
        let cat = catalog();
        let q = quote(&cat, 3031, &inventory(&[3031]), 1000.0, false);
        assert_eq!(q.remaining_cost, Some(0));
        assert!(!q.affordable);
        assert!(q.buy_now.is_none());
        assert!(q.blocked.is_some());
    }

    #[test]
    fn unknown_items_never_become_free_purchases() {
        let q = quote(&catalog(), 999999, &[], 10000.0, false);
        assert_eq!(q.remaining_cost, None);
        assert!(!q.affordable);
        assert!(q.buy_now.is_none());
        assert!(q.basket.is_empty());
        assert!(q.blocked.is_some());
    }

    #[test]
    fn missing_recipe_data_is_explicitly_unavailable() {
        let mut cat = catalog();
        cat.items.remove(&1036);
        let q = quote(&cat, 3508, &inventory(&[1036]), 5000.0, false);
        assert_eq!(q.remaining_cost, None);
        assert!(!q.affordable);
        assert!(q.blocked.is_some());
    }

    #[test]
    fn steelcaps_block_extra_boots_and_other_boot_upgrades() {
        let cat = catalog();
        assert!(!compatible(&cat, 1001, &[3047]));
        assert!(!compatible(&cat, 3006, &[3047]));
        let q = quote(&cat, 1001, &inventory(&[3047]), 300.0, false);
        assert!(!q.affordable);
        assert!(q.buy_now.is_none());
        assert!(q.blocked.is_some());
    }

    #[test]
    fn upgraded_boots_without_a_boots_tag_still_prevent_second_boots() {
        let cat = catalog();
        assert!(!compatible(&cat, 1001, &[3172]));
    }

    #[test]
    fn magical_footwear_replaces_boots_in_an_upgrade_recipe() {
        let cat = catalog();
        let q = quote(&cat, 3006, &inventory(&[2422, 1042, 1042]), 300.0, true);
        assert_eq!(q.remaining_cost, Some(300));
        assert!(q.affordable);
        assert!(compatible(&cat, 3006, &[2422]));
    }

    #[test]
    fn footwear_lock_blocks_boots_until_footwear_arrives() {
        let q = quote(&catalog(), 3006, &[], 2000.0, true);
        assert!(!q.affordable);
        assert!(q.buy_now.is_none());
        assert!(q.blocked.is_some());
    }

    #[test]
    fn last_whisper_exclusivity_does_not_treat_lethality_as_percent_penetration() {
        let cat = catalog();
        assert!(compatible(&cat, 3036, &[6676]));
        assert!(!compatible(&cat, 3033, &[3036]));
        assert!(compatible(&cat, 3036, &[3035]));
        assert!(!compatible(&cat, 3035, &[3036]));
    }

    #[test]
    fn lifeline_allows_an_upgrade_but_not_a_second_lifeline_item() {
        let cat = catalog();
        assert!(compatible(&cat, 3156, &[3155]));
        assert!(!compatible(&cat, 6673, &[3156]));
        assert!(!compatible(&cat, 3155, &[6673]));
    }

    #[test]
    fn map_and_store_restrictions_prevent_purchases() {
        for restriction in [0, 1, 2, 3, 4] {
            let mut cat = catalog();
            let item = cat.items.get_mut(&3031).unwrap();
            match restriction {
                0 => item.on_sr = false,
                1 => item.purchasable = false,
                2 => item.in_store = false,
                3 => item.required_champion = Some("Kalista".to_string()),
                _ => item.required_ally = Some("Ornn".to_string()),
            }
            let q = quote(&cat, 3031, &[], 5000.0, false);
            assert!(!q.affordable);
            assert!(q.basket.is_empty());
            assert!(q.blocked.is_some());
        }
    }

    #[test]
    fn zero_combine_special_forms_are_not_offered_without_unlock_state() {
        let q = quote(&catalog(), 3172, &inventory(&[3006]), 0.0, false);
        assert!(!q.affordable);
        assert!(q.buy_now.is_none());
        assert!(q.blocked.is_some());
    }

    #[test]
    fn a_full_inventory_does_not_imply_an_automatic_sale() {
        let inv = inventory(&[1055, 3508, 6675, 3031, 3072, 2003]);
        let q = quote(&catalog(), 3036, &inv, 10000.0, false);
        assert!(!q.affordable);
        assert!(q.buy_now.is_none());
        assert!(q.basket.is_empty());
        assert!(q.blocked.is_some());
    }

    #[test]
    fn a_component_upgrade_is_legal_with_no_empty_slots() {
        let inv = inventory(&[1036, 1018, 1055, 3006, 3072, 2003]);
        let q = quote(&catalog(), 3508, &inv, 700.0, false);
        assert_eq!(
            q.buy_now.as_ref().map(|c| (c.id, c.cost)),
            Some((3133, 700))
        );
        assert_eq!(q.basket_cost, 700);
    }

    #[test]
    fn finishing_a_component_takes_priority_over_expensive_loose_parts() {
        let q = quote(
            &catalog(),
            3508,
            &inventory(&[1036, 1036, 2022]),
            600.0,
            false,
        );
        assert_eq!(
            q.buy_now.as_ref().map(|c| (c.id, c.cost)),
            Some((3133, 100))
        );
        assert!(q.basket_cost <= 600);
    }

    #[test]
    fn a_basket_spends_only_its_budget_and_keeps_recipe_multiplicity() {
        let cat = catalog();
        let q = quote(&cat, 3031, &inventory(&[1038]), 1500.0, false);
        assert_eq!(
            q.basket.iter().map(|c| (c.id, c.cost)).collect::<Vec<_>>(),
            vec![(1037, 875), (1018, 600)]
        );
        assert_eq!(q.basket_cost, 1475);
        let daggers = quote(&cat, 6675, &[], 500.0, false);
        assert_eq!(
            daggers.basket.iter().map(|c| c.id).collect::<Vec<_>>(),
            vec![1042, 1042]
        );
        assert_eq!(daggers.basket_cost, 500);
    }

    #[test]
    fn basket_capacity_is_checked_after_every_purchase() {
        let inv = inventory(&[1055, 3508, 3031, 3072, 2003]);
        let q = quote(&catalog(), 6675, &inv, 1100.0, false);
        assert_eq!(q.basket.len(), 1);
        assert!(q.basket_cost <= 1100);
    }

    #[test]
    fn a_trinket_does_not_use_one_of_the_six_equipment_slots() {
        let inv = inventory(&[1055, 3508, 6675, 3031, 3072, 0, 3340]);
        let q = quote(&catalog(), 3036, &inv, 3300.0, false);
        assert!(q.affordable);
        assert_eq!(q.buy_now.as_ref().map(|c| c.id), Some(3036));
    }

    #[test]
    fn nonfinite_or_negative_gold_never_funds_a_purchase() {
        for gold in [f64::NAN, f64::INFINITY, -1.0] {
            let q = quote(&catalog(), 3031, &[], gold, false);
            assert!(!q.affordable);
            assert!(q.buy_now.is_none());
            assert!(q.blocked.is_some());
        }
    }

    #[test]
    fn observed_support_quest_completion_unlocks_its_free_choice() {
        let cat = catalog();
        let q = quote(&cat, 3869, &inventory(&[3867]), 0.0, false);
        assert_eq!(q.remaining_cost, Some(0));
        assert!(q.affordable);
        assert_eq!(q.buy_now.as_ref().map(|c| (c.id, c.cost)), Some((3869, 0)));
        assert!(!quote(&cat, 3869, &[], 400.0, false).affordable);
        assert!(!quote(&cat, 3869, &inventory(&[3865]), 400.0, false).affordable);
    }

    #[test]
    fn transformed_owned_items_satisfy_their_untransformed_target() {
        let cat = catalog();
        for (target, owned) in [(3004, 3042), (3003, 3040), (3119, 3121)] {
            let q = quote(&cat, target, &inventory(&[owned]), 5000.0, false);
            assert_eq!(q.remaining_cost, Some(0));
            assert!(!q.affordable);
            assert!(q.buy_now.is_none());
        }
    }

    #[test]
    fn mage_penetration_exclusivity_preserves_flat_penetration_options() {
        let cat = catalog();
        assert!(compatible(&cat, 3135, &[4645, 3020]));
        assert!(!compatible(&cat, 3135, &[3137]));
        assert!(compatible(&cat, 3135, &[4630]));
    }

    #[test]
    fn owned_support_quest_items_block_buying_a_second_quest() {
        let cat = catalog();
        assert!(!compatible(&cat, 3865, &[3869]));
        assert!(!compatible(&cat, 3865, &[3866]));
    }

    #[test]
    fn jungle_companion_purchase_needs_verified_loadout_information() {
        let q = quote(&catalog(), 1101, &[], 500.0, false);
        assert!(!q.affordable);
        assert!(q.blocked.is_some());
        assert!(!compatible(&catalog(), 1102, &[1101]));
    }

    #[test]
    fn saving_targets_use_the_cheapest_legal_nested_purchase() {
        let cat = catalog();
        let q = quote(&cat, 3508, &[], 100.0, false);
        assert_eq!(
            q.save_for.as_ref().map(|c| (c.id, c.cost)),
            Some((2022, 250))
        );
        assert!(q.buy_now.is_none());
        assert!(q.blocked.is_none());
        let inv = inventory(&[1036, 1018, 1055, 3006, 3072, 2003]);
        let full = quote(&cat, 3508, &inv, 100.0, false);
        assert_eq!(
            full.save_for.as_ref().map(|c| (c.id, c.cost)),
            Some((3133, 700))
        );
    }

    #[test]
    fn saving_for_a_combine_does_not_suggest_duplicate_components() {
        let q = quote(
            &catalog(),
            3031,
            &inventory(&[1038, 1037, 1018]),
            100.0,
            false,
        );
        assert_eq!(
            q.save_for.as_ref().map(|c| (c.id, c.cost)),
            Some((3031, 725))
        );
    }

    #[test]
    fn tank_and_fighter_exclusive_recipes_allow_upgrades_without_second_families() {
        let cat = catalog();
        assert!(!compatible(&cat, 3068, &[6664]));
        assert!(compatible(&cat, 3068, &[6660]));
        assert!(!compatible(&cat, 3074, &[3748]));
        assert!(!compatible(&cat, 3077, &[6631]));
        assert!(!compatible(&cat, 3076, &[3075]));
        assert!(compatible(&cat, 3075, &[3076]));
    }

    #[test]
    fn hidden_items_are_not_offered_as_store_purchases() {
        let mut raw: serde_json::Value =
            serde_json::from_str(crate::ddragon::test_support::ITEMS).unwrap();
        raw["data"]["3031"]["hideFromAll"] = true.into();
        let cat = Catalog::from_json(
            "16.17.1",
            &raw,
            &serde_json::Value::Null,
            &serde_json::Value::Null,
        );
        let q = quote(&cat, 3031, &[], 5000.0, false);
        assert!(!q.affordable);
        assert!(q.blocked.is_some());
    }
}
