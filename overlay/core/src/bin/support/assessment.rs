//! Shared legality checks for recorded replay and corpus evaluation.
use recall_core::{
    ddragon::Catalog,
    engine::{self, Plan},
    live::{InvItem, LiveSnapshot},
    shop,
};
use serde::Serialize;

pub(super) fn owns_equivalent(cat: &Catalog, inventory: &[InvItem], target: u32) -> bool {
    inventory.iter().filter(|item| item.count > 0).any(|item| {
        let mut current = Some(item.id);
        for _ in 0..32 {
            let Some(id) = current else { break };
            if id == target || (id == 2422 && target == 1001) {
                return true;
            }
            current = cat.item(id).and_then(|item| item.special_recipe);
        }
        false
    })
}

#[derive(Debug, Serialize)]
pub(super) struct Violation {
    pub(super) kind: &'static str,
    pub(super) message: String,
}

#[derive(Debug, Default, Serialize)]
pub(super) struct Assessment {
    pub(super) quote_checked: bool,
    pub(super) target_legal: bool,
    pub(super) target_owned: bool,
    pub(super) target_blocked: bool,
    pub(super) target_unpriced: bool,
    pub(super) free_affordable_action: bool,
    pub(super) unknown_inventory_items: usize,
    pub(super) violations: Vec<Violation>,
}

impl Assessment {
    fn fail(&mut self, kind: &'static str, message: impl Into<String>) {
        self.violations.push(Violation {
            kind,
            message: message.into(),
        });
    }
}

pub(super) fn component_key(component: &shop::ShopComponent) -> (u32, u32, bool) {
    (component.id, component.cost, component.owned)
}

pub(super) fn validate_plan(
    plan: &Plan,
    cat: &Catalog,
    snapshot: Option<&LiveSnapshot>,
) -> Assessment {
    let mut assessment = Assessment::default();
    let me = snapshot.and_then(|snapshot| snapshot.me.as_ref());
    let inventory = me.map(|me| me.player.items.as_slice()).unwrap_or(&[]);
    let gold = me.map(|me| me.gold).unwrap_or(0.0);
    let boots_locked = me
        .is_some_and(|me| me.rune_ids.as_ref().is_some_and(|ids| ids.contains(&8304)))
        && !inventory
            .iter()
            .any(|item| item.count > 0 && cat.item(item.id).is_some_and(|item| item.effects.boots));
    let swiftplay = snapshot.is_some_and(|snapshot| {
        engine::GameMode::parse(&snapshot.mode) == engine::GameMode::Swiftplay
    });
    let context = shop::ShopContext {
        champion: me.map(|me| me.player.champion.as_str()),
        spell_ids: me
            .filter(|me| !me.spell_ids.is_empty())
            .map(|me| me.spell_ids.as_slice()),
        boots_locked,
        swiftplay,
    };
    // A future boot upgrade can be legal even before Magical Footwear arrives.
    // Only pregame planning may use the proposed spell page for compatibility.
    let path_context = shop::ShopContext {
        champion: context
            .champion
            .or_else(|| (!plan.champion.is_empty()).then_some(plan.champion.as_str())),
        spell_ids: context
            .spell_ids
            .or_else(|| snapshot.is_none().then_some(plan.spell_ids.as_slice())),
        boots_locked: false,
        swiftplay,
    };
    let immutable: Vec<_> = inventory
        .iter()
        .filter(|owned| owned.count > 0)
        .filter(|owned| {
            cat.item(owned.id)
                .is_some_and(|item| item.is_owned_commitment(cat) || item.effects.boots)
        })
        .map(|owned| owned.id)
        .collect();
    assessment.unknown_inventory_items = inventory
        .iter()
        .filter(|owned| cat.item(owned.id).is_none())
        .count();
    if snapshot.is_some() && me.is_none() && plan.next.is_some() {
        assessment.fail(
            "identity",
            "purchase advice was produced without the player's live identity",
        );
    }
    for score in &plan.score_trace {
        if [
            score.prior,
            score.completion,
            score.situation,
            score.delay,
            score.total,
        ]
        .iter()
        .any(|score| !score.is_finite())
        {
            assessment.fail("score", format!("nonfinite score for item {}", score.id));
        }
    }
    let mut future = Vec::new();
    for item in &plan.path {
        let Some(catalog_item) = cat.item(item.id) else {
            continue;
        };
        if item.owned {
            if me.is_some() && !owns_equivalent(cat, inventory, item.id) {
                assessment.fail(
                    "ownership",
                    format!("path marks unowned item {} as owned", item.id),
                );
            }
            continue;
        }
        if !shop::compatible_with_context(cat, item.id, &immutable, &path_context) {
            assessment.fail(
                "path",
                format!(
                    "future item {} conflicts with immutable owned equipment or a shop restriction",
                    item.id
                ),
            );
        }
        for &earlier in &future {
            if !shop::compatible_with_context(cat, item.id, &[earlier], &path_context)
                && !shop::compatible_with_context(cat, earlier, &[item.id], &path_context)
            {
                assessment.fail(
                    "path",
                    format!("future items {earlier} and {} cannot coexist", item.id),
                );
            }
        }
        if catalog_item.is_finished(cat) || catalog_item.effects.boots {
            future.push(item.id);
        }
    }
    let Some(next) = &plan.next else {
        return assessment;
    };
    let quote = shop::quote_with_context(cat, next.id, inventory, gold, &context);
    assessment.quote_checked = true;
    assessment.target_owned = cat.item(next.id).is_some_and(|item| item.max_stacks <= 1)
        && owns_equivalent(cat, inventory, next.id);
    assessment.target_blocked = quote.blocked.is_some();
    assessment.target_legal = quote.blocked.is_none();
    assessment.target_unpriced = quote.remaining_cost.is_none();
    let mut price_errors = Vec::new();
    if next.price_known != quote.remaining_cost.is_some()
        || (next.price_known && quote.remaining_cost != Some(next.remaining_cost))
    {
        price_errors.push(format!(
            "remaining price {} (known={}) differs from quote {:?}",
            next.remaining_cost, next.price_known, quote.remaining_cost
        ));
    }
    if cat
        .item(next.id)
        .is_some_and(|item| item.price_known && item.total != next.cost)
    {
        price_errors.push("displayed full price differs from catalog".into());
    }
    if next
        .components
        .iter()
        .map(component_key)
        .collect::<Vec<_>>()
        != quote
            .components
            .iter()
            .map(component_key)
            .collect::<Vec<_>>()
    {
        price_errors
            .push("displayed recipe components differ from the inventory-aware quote".into());
    }
    let expected_hint = if quote.blocked.is_some() {
        None
    } else {
        quote
            .buy_now
            .as_ref()
            .or(quote.save_for.as_ref())
            .map(component_key)
            .or_else(|| {
                quote
                    .components
                    .iter()
                    .filter(|component| !component.owned)
                    .min_by_key(|component| (component.cost, component.id))
                    .map(component_key)
            })
            .or_else(|| quote.remaining_cost.map(|cost| (next.id, cost, false)))
    };
    if next
        .buy_now
        .as_ref()
        .is_some_and(|action| expected_hint != Some(component_key(action)))
    {
        price_errors.push("purchase or saving hint differs from the legal quoted action".into());
    }
    let expected_gap = next
        .buy_now
        .as_ref()
        .filter(|_| gold.is_finite())
        .and_then(|action| {
            (f64::from(action.cost) > gold).then(|| (f64::from(action.cost) - gold).ceil() as u32)
        });
    if next.save_gap != expected_gap {
        price_errors.push("saving gap differs from current gold and the quoted action".into());
    }
    let basket_sum: u64 = next.basket.iter().map(|item| u64::from(item.cost)).sum();
    if basket_sum != u64::from(next.basket_cost) {
        price_errors.push("basket price differs from the sum of its actions".into());
    }
    if !price_errors.is_empty() {
        assessment.fail(
            "price",
            format!("target {}: {}", next.id, price_errors.join("; ")),
        );
    }
    if next.buy_now_affordable {
        let valid = next.buy_now.as_ref().is_some_and(|action| {
            !action.owned
                && f64::from(action.cost) <= gold
                && quote.blocked.is_none()
                && quote.buy_now.as_ref().map(component_key) == Some(component_key(action))
        });
        if !valid {
            assessment.fail(
                "affordable_action",
                format!(
                    "target {} claims an affordable action absent from its legal quote",
                    next.id
                ),
            );
        } else {
            assessment.free_affordable_action =
                next.buy_now.as_ref().is_some_and(|action| action.cost == 0);
        }
    }
    if !next.basket.is_empty()
        && (quote.blocked.is_some()
            || !gold.is_finite()
            || basket_sum as f64 > gold
            || next.basket.iter().map(component_key).collect::<Vec<_>>()
                != quote.basket.iter().map(component_key).collect::<Vec<_>>())
    {
        assessment.fail(
            "affordable_basket",
            format!(
                "target {} exposes a basket that differs from the legal affordable quote",
                next.id
            ),
        );
    }
    assessment
}
