//! Boots a role quest moves into the hidden role slot. On 16.x Summoner's Rift the bot-lane quest takes
//! the player's boots out of the item bar; the Live Client `items` list stops reporting them although the
//! player still owns them and can upgrade them. Without this, the planner saw "no boots", kept boots on
//! the path and let other items overtake them (the Sivir game of 2026-09-27: boots vanished at 13:37 with
//! no gold refund, two Daggers vanished at 14:15 as 300 gold was spent on Berserker's Greaves).
//!
//! The tracker runs once per live observation, before planning, and re-adds the slotted boots to the
//! player's inventory as a virtual entry in [`ROLE_SLOT`], so ownership, legality and stats all see them.
use crate::ddragon::Catalog;
use crate::live::{InvItem, LiveSnapshot};

/// Inventory slot number for the virtual role-slot entry; the Live Client reports slots 0-6.
pub const ROLE_SLOT: u32 = 9;
/// Selling refunds 70% of an item's price; an undo (only right after buying, see `UNDO_WINDOW`) refunds
/// all of it. Boots that vanish with a gold change matching neither went into the role slot: when the
/// quest took the Sivir player's boots, gold happened to rise by 304 from other income.
const SELL_REFUND_SHARE: f64 = 0.7;
const UNDO_WINDOW_SECONDS: f64 = 30.0;
/// Slack when matching a gold drop to an upgrade's combine cost (income between polls, rounding).
const GOLD_SLACK: f64 = 40.0;
/// The Magical Footwear rune's free boots upgrade exactly like Boots, though no recipe lists them.
const SLIGHTLY_MAGICAL_FOOTWEAR: u32 = 2422;
const BOOTS: u32 = 1001;

#[derive(Clone, Debug, Default)]
pub struct RoleSlotTracker {
    slotted: Option<u32>,
    last_items: Vec<u32>,
    last_gold: Option<f64>,
    /// Game time the visible boots last appeared, for telling an undo from the quest.
    boots_since: Option<f64>,
}

impl RoleSlotTracker {
    /// The boots currently held in the role slot, if any.
    pub fn slotted(&self) -> Option<u32> {
        self.slotted
    }

    /// Update from one observation and add the slotted boots to `snap`'s own inventory.
    /// `preferred` orders boots for the ambiguous case (an upgrade bought with no visible components),
    /// e.g. the champion's most-bought boots first.
    pub fn apply(&mut self, cat: &Catalog, snap: &mut LiveSnapshot, preferred: &[u32]) {
        let Some(me) = snap.me.as_mut() else {
            return;
        };
        let is_boots = |id: u32| cat.item(id).is_some_and(|i| i.effects.boots);
        let price = |id: u32| cat.item(id).map_or(0.0, |i| f64::from(i.total));
        let items: Vec<u32> = me
            .player
            .items
            .iter()
            .filter(|i| i.slot <= 6)
            .flat_map(|i| std::iter::repeat_n(i.id, i.count.max(1) as usize))
            .collect();
        let gold = me.gold;
        let now = snap.game_time;
        let visible = items.iter().copied().any(is_boots);
        let removed = multiset_minus(&self.last_items, &items);
        let added = multiset_minus(&items, &self.last_items);
        let spent = self.last_gold.map_or(0.0, |g| g - gold);
        if visible {
            // Boots are on the bar: nothing is slotted (a later vanish is detected below).
            self.slotted = None;
            if !self.last_items.iter().copied().any(is_boots) {
                self.boots_since = Some(now);
            }
        } else if let Some(slot) = self.slotted {
            if let Some(upgrade) = upgrade(cat, slot, &removed, &added, spent, preferred, &price) {
                self.slotted = Some(upgrade);
            }
        } else if let Some(&gone) = removed
            .iter()
            .filter(|&&id| is_boots(id))
            .max_by(|a, b| price(**a).total_cmp(&price(**b)))
        {
            let rise = self.last_gold.map_or(0.0, |g| gold - g);
            let sold = (rise - SELL_REFUND_SHARE * price(gone)).abs() <= GOLD_SLACK;
            let undone = self
                .boots_since
                .is_some_and(|since| now - since <= UNDO_WINDOW_SECONDS)
                && (rise - price(gone)).abs() <= GOLD_SLACK;
            if !sold && !undone {
                self.slotted = Some(gone);
            }
        }
        self.last_items = items;
        self.last_gold = Some(gold);
        if let Some(slot) = self.slotted {
            me.player.items.push(InvItem {
                id: slot,
                name: cat.item(slot).map(|i| i.name.clone()).unwrap_or_default(),
                count: 1,
                slot: ROLE_SLOT,
            });
        }
    }
}

/// The boots `slot` became: the owned components of a boots recipe that builds from `slot` vanished
/// (any of them, or none when bought outright), nothing appeared on the bar, and gold dropped by the rest
/// of its price. The 05:19 Xayah game: the free rune boots in the role slot plus one of the two Daggers
/// became Berserker's Greaves for 550 gold.
fn upgrade(
    cat: &Catalog,
    slot: u32,
    removed: &[u32],
    added: &[u32],
    spent: f64,
    preferred: &[u32],
    price: &dyn Fn(u32) -> f64,
) -> Option<u32> {
    if spent <= GOLD_SLACK || !added.is_empty() {
        return None;
    }
    let slot = if slot == SLIGHTLY_MAGICAL_FOOTWEAR {
        BOOTS
    } else {
        slot
    };
    let mut matches: Vec<u32> = cat
        .items
        .values()
        .filter(|i| i.effects.boots && i.from.contains(&slot) && i.on_sr)
        .filter(|i| {
            let parts = multiset_minus(&i.from, &[slot]);
            // Every vanished item is one of the recipe's parts.
            if !multiset_minus(removed, &parts).is_empty() {
                return false;
            }
            let paid: f64 = removed.iter().map(|&p| price(p)).sum();
            let cost = f64::from(i.total) - price(slot) - paid;
            (spent - cost).abs() <= GOLD_SLACK
        })
        .map(|i| i.id)
        .collect();
    matches.sort_by_key(|id| {
        (
            preferred.iter().position(|p| p == id).unwrap_or(usize::MAX),
            *id,
        )
    });
    matches.first().copied()
}

fn multiset_minus(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut rest = b.to_vec();
    a.iter()
        .copied()
        .filter(|x| match rest.iter().position(|y| y == x) {
            Some(i) => {
                rest.swap_remove(i);
                false
            }
            None => true,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ddragon::test_support::catalog;
    use crate::live::{Me, Player};

    const DAGGER: u32 = 1042;
    const BERSERKERS: u32 = 3006;
    const ESSENCE_REAVER: u32 = 3508;

    fn snap(items: &[u32], gold: f64, time: f64) -> LiveSnapshot {
        LiveSnapshot {
            game_time: time,
            me: Some(Me {
                player: Player {
                    champion: "Sivir".into(),
                    position: "BOTTOM".into(),
                    items: items
                        .iter()
                        .enumerate()
                        .map(|(slot, &id)| InvItem {
                            id,
                            count: 1,
                            slot: slot as u32,
                            ..Default::default()
                        })
                        .collect(),
                    ..Default::default()
                },
                gold,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn owns(s: &LiveSnapshot, id: u32) -> bool {
        s.me.as_ref()
            .unwrap()
            .player
            .items
            .iter()
            .any(|i| i.id == id)
    }

    #[test]
    fn quest_boots_stay_owned_and_their_upgrade_is_seen() {
        // The Sivir game: Boots and two Daggers on the bar, the quest takes the Boots (no refund), then the
        // Daggers vanish while 300 gold is spent: Berserker's Greaves, in the role slot.
        let cat = catalog();
        let berserkers = cat.item(BERSERKERS).expect("fixture has Berserker's");
        assert!(berserkers.from.contains(&BOOTS));
        let mut t = RoleSlotTracker::default();
        let mut s = snap(&[ESSENCE_REAVER, BOOTS, DAGGER, DAGGER], 138.0, 780.0);
        t.apply(&cat, &mut s, &[]);
        assert!(t.slotted().is_none());
        // Gold rose by 304 from other income as the quest took the boots (the recorded game).
        let mut s = snap(&[ESSENCE_REAVER, DAGGER, DAGGER], 138.0 + 304.0, 818.0);
        t.apply(&cat, &mut s, &[]);
        assert_eq!(t.slotted(), Some(BOOTS));
        assert!(owns(&s, BOOTS), "the planner sees the slotted boots");
        let combine = f64::from(berserkers.total)
            - berserkers
                .from
                .iter()
                .map(|p| f64::from(cat.item(*p).unwrap().total))
                .sum::<f64>();
        let mut s = snap(&[ESSENCE_REAVER], 442.0 + 20.0 - combine, 856.0);
        t.apply(&cat, &mut s, &[]);
        assert_eq!(t.slotted(), Some(BERSERKERS));
        assert!(owns(&s, BERSERKERS) && !owns(&s, DAGGER));
    }

    #[test]
    fn the_runes_free_boots_upgrade_like_boots() {
        let cat = catalog();
        if cat.item(SLIGHTLY_MAGICAL_FOOTWEAR).is_none() {
            return; // the item subset fixture predates the rune's boots
        }
        let mut t = RoleSlotTracker::default();
        t.apply(
            &cat,
            &mut snap(&[SLIGHTLY_MAGICAL_FOOTWEAR], 200.0, 600.0),
            &[],
        );
        t.apply(&cat, &mut snap(&[], 210.0, 800.0), &[]);
        assert_eq!(t.slotted(), Some(SLIGHTLY_MAGICAL_FOOTWEAR));
        // Berserker's from the free boots and one of its two Daggers: 1100 - 300 - 250 = 550 gold.
        t.apply(&cat, &mut snap(&[DAGGER], 1000.0, 900.0), &[]);
        t.apply(&cat, &mut snap(&[], 460.0, 902.0), &[BERSERKERS]);
        assert_eq!(t.slotted(), Some(BERSERKERS));
    }

    #[test]
    fn sold_boots_are_not_slotted() {
        let cat = catalog();
        let mut t = RoleSlotTracker::default();
        t.apply(&cat, &mut snap(&[BOOTS], 100.0, 300.0), &[]);
        let mut s = snap(&[], 100.0 + 0.7 * 300.0, 600.0);
        t.apply(&cat, &mut s, &[]);
        assert_eq!(t.slotted(), None, "sold for 70%");
        assert!(!owns(&s, BOOTS));
        let mut t = RoleSlotTracker::default();
        t.apply(&cat, &mut snap(&[], 400.0, 100.0), &[]);
        t.apply(&cat, &mut snap(&[BOOTS], 100.0, 110.0), &[]);
        t.apply(&cat, &mut snap(&[], 400.0, 115.0), &[]);
        assert_eq!(t.slotted(), None, "undone right after buying");
    }
}
