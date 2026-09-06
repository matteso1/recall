//! Pure bookkeeping for observations, match boundaries, refreshes, and successful auto-imports.
use crate::aggregate::{Position, RunePageIds};
use crate::ddragon::{normalize, Catalog};
use crate::engine::Plan;
use crate::live::LiveSnapshot;
use crate::shop::{self, ShopContext};
use crate::state::SourceStatus;

/// Every setting that can change which population supplies a build.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AggregateKey {
    pub champion_key: u32,
    pub position: Option<Position>,
    pub region: String,
    pub tier: String,
}

#[derive(Clone, Debug)]
pub struct RefreshToken<K> {
    pub key: K,
    generation: u64,
}

/// Schedules one request for the desired key and rejects old completions, including A -> B -> A.
#[derive(Debug)]
pub struct RefreshGate<K> {
    key: Option<K>,
    generation: u64,
    in_flight: Option<u64>,
    fresh_until_ms: Option<u64>,
    next_try_ms: u64,
}

impl<K> Default for RefreshGate<K> {
    fn default() -> Self {
        Self {
            key: None,
            generation: 0,
            in_flight: None,
            fresh_until_ms: None,
            next_try_ms: 0,
        }
    }
}

impl<K: Clone + Eq> RefreshGate<K> {
    pub fn is_current(&self, key: &K) -> bool {
        self.key.as_ref() == Some(key)
    }

    pub fn begin(&mut self, key: K, now_ms: u64) -> Option<RefreshToken<K>> {
        if !self.is_current(&key) {
            self.clear();
            self.key = Some(key.clone());
        }
        if self.in_flight.is_some()
            || now_ms < self.next_try_ms
            || self.fresh_until_ms.is_some_and(|until| now_ms < until)
        {
            return None;
        }
        self.generation = self.generation.wrapping_add(1);
        self.in_flight = Some(self.generation);
        Some(RefreshToken {
            key,
            generation: self.generation,
        })
    }

    pub fn finish(
        &mut self,
        token: &RefreshToken<K>,
        fresh_until_ms: Option<u64>,
        next_try_ms: u64,
    ) -> bool {
        if !self.is_current(&token.key) || self.in_flight != Some(token.generation) {
            return false;
        }
        self.in_flight = None;
        self.fresh_until_ms = fresh_until_ms;
        self.next_try_ms = next_try_ms;
        true
    }

    pub fn clear(&mut self) {
        self.key = None;
        self.generation = self.generation.wrapping_add(1);
        self.in_flight = None;
        self.fresh_until_ms = None;
        self.next_try_ms = 0;
    }
}

#[derive(Clone, Debug)]
struct PendingSpells {
    before: Option<(u32, u32)>,
    target: (u32, u32),
    acknowledge_by_ms: u64,
}

/// Import signatures advance only on success. Observed manual edits take precedence for the match.
#[derive(Clone, Debug, Default)]
pub struct ImportTracker {
    runes_done: Option<(u32, String)>,
    spells_done: Option<(u32, (u32, u32))>,
    itemset_done: Option<(u32, String)>,
    observed_spells: Option<(u32, u32)>,
    pending_spells: Option<PendingSpells>,
    manual_spells: bool,
}

impl ImportTracker {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn runes_needed(&self, champion: u32, signature: &str) -> bool {
        champion > 0
            && !signature.is_empty()
            && self
                .runes_done
                .as_ref()
                .map(|(key, sig)| (*key, sig.as_str()))
                != Some((champion, signature))
    }

    pub fn record_runes(&mut self, champion: u32, signature: String, succeeded: bool) {
        if succeeded {
            self.runes_done = Some((champion, signature));
        }
    }

    pub fn itemset_needed(&self, champion: u32, signature: &str) -> bool {
        champion > 0
            && !signature.is_empty()
            && self
                .itemset_done
                .as_ref()
                .map(|(key, sig)| (*key, sig.as_str()))
                != Some((champion, signature))
    }

    pub fn record_itemset(&mut self, champion: u32, signature: String, succeeded: bool) {
        if succeeded {
            self.itemset_done = Some((champion, signature));
        }
    }

    pub fn observe_spells(&mut self, spells: (u32, u32), now_ms: u64) {
        if spells.0 == 0 || spells.1 == 0 || spells.0 == spells.1 {
            return;
        }
        if let Some(pending) = &self.pending_spells {
            if spells == pending.target {
                self.pending_spells = None;
                self.observed_spells = Some(spells);
                return;
            }
            if Some(spells) == pending.before && now_ms <= pending.acknowledge_by_ms {
                return;
            }
            self.manual_spells = true;
            self.pending_spells = None;
        } else if self
            .observed_spells
            .is_some_and(|previous| previous != spells)
        {
            self.manual_spells = true;
        }
        self.observed_spells = Some(spells);
    }

    pub fn manual_spell_override(&self) -> bool {
        self.manual_spells
    }

    pub fn spells_needed(&self, champion: u32, spells: (u32, u32), locked: bool) -> bool {
        if champion == 0
            || spells.0 == 0
            || spells.1 == 0
            || spells.0 == spells.1
            || self.manual_spells
        {
            return false;
        }
        match self.spells_done {
            Some((key, previous)) if key == champion => previous != spells && locked,
            _ => true,
        }
    }

    pub fn record_spells(
        &mut self,
        champion: u32,
        spells: (u32, u32),
        now_ms: u64,
        succeeded: bool,
    ) {
        if succeeded {
            self.spells_done = Some((champion, spells));
            self.pending_spells = (self.observed_spells != Some(spells)).then_some(PendingSpells {
                before: self.observed_spells,
                target: spells,
                acknowledge_by_ms: now_ms.saturating_add(3000),
            });
        }
    }
}

/// A transient LCU disconnect does not end a match. Match IDs and large clock resets catch missed transitions.
#[derive(Clone, Debug, Default)]
pub struct SessionTracker {
    generation: u64,
    active: bool,
    last_phase: String,
    select_id: Option<String>,
    last_game_time: Option<f64>,
}

impl SessionTracker {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    fn start(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.active = true;
        self.last_game_time = None;
        self.select_id = None;
    }

    /// Returns true only when a new match has begun, including a new champion select.
    pub fn observe_phase(&mut self, phase: Option<&str>, select_id: Option<&str>) -> bool {
        let Some(phase) = phase else { return false };
        let id_changed = select_id
            .zip(self.select_id.as_deref())
            .is_some_and(|(new, old)| new != old);
        let started = if phase == "ChampSelect" {
            self.last_phase != "ChampSelect" || !self.active || id_changed
        } else {
            matches!(phase, "GameStart" | "InProgress" | "Reconnect") && !self.active
        };
        if started {
            self.start();
        }
        if phase == "ChampSelect" {
            if let Some(id) = select_id.filter(|id| !id.is_empty()) {
                self.select_id = Some(id.to_string());
            }
        } else if matches!(
            phase,
            "None"
                | "Lobby"
                | "Matchmaking"
                | "ReadyCheck"
                | "WaitingForStats"
                | "PreEndOfGame"
                | "EndOfGame"
        ) {
            self.active = false;
            self.last_game_time = None;
        }
        self.last_phase = phase.to_string();
        started
    }

    pub fn observe_live(&mut self, game_time: f64) -> bool {
        if !game_time.is_finite() || game_time < 0.0 {
            return false;
        }
        let started = !self.active
            || self
                .last_game_time
                .is_some_and(|previous| previous - game_time > 5.0);
        if started {
            self.start();
        }
        self.last_phase = "InProgress".to_string();
        self.last_game_time = Some(game_time);
        started
    }
}

pub fn should_poll_live(phase: Option<&str>) -> bool {
    matches!(
        phase,
        None | Some("" | "GameStart" | "InProgress" | "Reconnect")
    )
}

const LIVE_STALE_AFTER_MS: u64 = 6000;

/// Commands must recompute age when invoked, even between two poller updates.
pub fn fresh_identity_at(source: &SourceStatus, now_ms: u64) -> bool {
    !source.stale
        && source.identity_known
        && source
            .observed_at_ms
            .and_then(|at| now_ms.checked_sub(at))
            .is_some_and(|age| age <= LIVE_STALE_AFTER_MS)
}

/// Only an affirmative gameflow observation ends a match, never a failed LCU request.
pub fn confirmed_game_end(phase: Option<&str>) -> bool {
    matches!(
        phase,
        Some(
            "WaitingForStats"
                | "PreEndOfGame"
                | "EndOfGame"
                | "None"
                | "Lobby"
                | "Matchmaking"
                | "ReadyCheck"
        )
    )
}

/// Validate a local target choice against the current plan and actual inventory.
/// The caller separately verifies freshness and, pre-game, the lobby's own identity.
pub fn validate_item_pin(
    plan: &Plan,
    cat: &Catalog,
    live: Option<&LiveSnapshot>,
    item_id: u32,
) -> Result<(), String> {
    let mut offered = plan
        .path
        .iter()
        .chain(&plan.options)
        .filter(|item| item.id == item_id);
    let Some(item) = offered.next() else {
        return Err("Choose an item from the current build or alternatives".into());
    };
    if item.owned || offered.any(|item| item.owned) {
        return Err("That target is already owned".into());
    }
    let me = match live {
        Some(live) => Some(
            live.me
                .as_ref()
                .filter(|me| {
                    !plan.champion.is_empty()
                        && normalize(&me.player.champion) == normalize(&plan.champion)
                })
                .ok_or("Your current player identity is unavailable")?,
        ),
        None => None,
    };
    let inventory = me.map(|me| me.player.items.as_slice()).unwrap_or(&[]);
    if inventory.iter().any(|item| item.id == item_id) {
        return Err("That target is already owned".into());
    }
    let inventory_ids: Vec<_> = inventory
        .iter()
        .flat_map(|item| std::iter::repeat_n(item.id, item.count.clamp(1, 6) as usize))
        .collect();
    let has_boots = inventory
        .iter()
        .any(|item| cat.item(item.id).is_some_and(|item| item.effects.boots));
    let footwear = cat.rune_id("Magical Footwear").unwrap_or(8304);
    let context = ShopContext {
        champion: Some(&plan.champion),
        spell_ids: me
            .and_then(|me| (!me.spell_ids.is_empty()).then_some(me.spell_ids.as_slice()))
            .or_else(|| {
                live.is_none()
                    .then_some(plan.spell_ids.as_slice())
                    .filter(|ids| !ids.is_empty())
            }),
        boots_locked: !has_boots
            && me.is_some_and(|me| {
                me.rune_ids
                    .as_ref()
                    .is_some_and(|ids| ids.contains(&footwear))
            }),
        swiftplay: live.is_some_and(|live| {
            crate::engine::GameMode::parse(&live.mode) == crate::engine::GameMode::Swiftplay
        }),
    };
    if !shop::compatible_with_context(cat, item_id, &inventory_ids, &context) {
        return Err("That target is incompatible with your current inventory or loadout".into());
    }
    // A completed support quest grants a real zero-gold purchase. Price alone
    // cannot distinguish that choice from an already-owned transformation.
    if let Some(reason) = shop::quote_with_context(
        cat,
        item_id,
        inventory,
        me.map(|me| me.gold).unwrap_or(0.0),
        &context,
    )
    .blocked
    {
        return Err(reason);
    }
    Ok(())
}

/// Aggregate rune IDs must still exist in this patch and occupy a legal page layout.
pub fn validate_rune_page(page: &RunePageIds, cat: &Catalog) -> Result<(), String> {
    if page.perks.len() != 9
        || page.primary_style == page.sub_style
        || !cat.style_names.contains_key(&page.primary_style)
        || !cat.style_names.contains_key(&page.sub_style)
    {
        return Err("Rune page has an invalid style or perk count for this patch".into());
    }
    for (slot, id) in page.perks[..4].iter().enumerate() {
        if !cat
            .runes
            .get(id)
            .is_some_and(|rune| rune.style == page.primary_style && rune.slot == slot)
        {
            return Err(format!(
                "Primary rune {id} is not valid in slot {} for this patch",
                slot + 1
            ));
        }
    }
    let mut secondary_slot = None;
    for id in &page.perks[4..6] {
        let rune = cat
            .runes
            .get(id)
            .filter(|rune| rune.style == page.sub_style && (1..=3).contains(&rune.slot))
            .ok_or_else(|| format!("Secondary rune {id} is not valid for this patch"))?;
        if secondary_slot == Some(rune.slot) {
            return Err("Secondary runes must use two different slots".into());
        }
        secondary_slot = Some(rune.slot);
    }
    let shards: [&[u32]; 3] = [
        &[5008, 5005, 5007],
        &[5008, 5010, 5001],
        &[5011, 5013, 5001],
    ];
    for (id, choices) in page.perks[6..].iter().zip(shards) {
        if !choices.contains(id) {
            return Err(format!("Stat shard {id} is not valid in this slot"));
        }
    }
    Ok(())
}

/// Age the last advancing, identifiable observation; repeated/failing responses cannot renew it.
#[derive(Clone, Debug, Default)]
pub struct LiveFreshness {
    observed_at_ms: Option<u64>,
    game_time: Option<f64>,
    identity_known: bool,
    last_request_ok: bool,
}

impl LiveFreshness {
    pub fn observe(&mut self, game_time: f64, identity_known: bool, now_ms: u64) {
        self.identity_known = identity_known;
        if !identity_known || !game_time.is_finite() || game_time < 0.0 {
            self.miss();
            return;
        }
        if self.game_time != Some(game_time) {
            self.observed_at_ms = Some(now_ms);
            self.game_time = Some(game_time);
        }
        self.last_request_ok = true;
    }

    pub fn miss(&mut self) {
        self.last_request_ok = false;
    }

    pub fn status(&self, now_ms: u64) -> SourceStatus {
        let age_ms = self
            .observed_at_ms
            .and_then(|observed| now_ms.checked_sub(observed));
        SourceStatus {
            observed_at_ms: self.observed_at_ms,
            age_ms,
            stale: !self.last_request_ok
                || !self.identity_known
                || age_ms.is_none_or(|age| age > LIVE_STALE_AFTER_MS),
            identity_known: self.identity_known,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_recheck_source_age_instead_of_trusting_a_cached_fresh_flag() {
        let source = SourceStatus {
            observed_at_ms: Some(100),
            age_ms: Some(0),
            stale: false,
            identity_known: true,
        };
        assert!(fresh_identity_at(&source, 200));
        assert!(!fresh_identity_at(&source, 6200));
        assert!(!fresh_identity_at(&source, 99));
        assert!(!fresh_identity_at(
            &SourceStatus {
                identity_known: false,
                ..source.clone()
            },
            200
        ));
        assert!(!fresh_identity_at(
            &SourceStatus {
                stale: true,
                ..source
            },
            200
        ));
    }

    #[test]
    fn pins_must_be_current_unowned_compatible_choices() {
        use crate::engine::{Plan, PlanItem};
        use crate::live::{InvItem, LiveSnapshot, Me, Player};
        let cat = crate::ddragon::test_support::catalog();
        let item = |id, owned| PlanItem {
            id,
            owned,
            ..Default::default()
        };
        let plan = Plan {
            champion: "Xayah".into(),
            path: vec![item(3031, false), item(3006, true)],
            options: vec![item(3036, false), item(3033, false)],
            ..Default::default()
        };
        let mut live = LiveSnapshot {
            me: Some(Me {
                player: Player {
                    champion: "Xayah".into(),
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(validate_item_pin(&plan, &cat, Some(&live), 3031).is_ok());
        assert!(validate_item_pin(&plan, &cat, Some(&live), 999999).is_err());
        assert!(validate_item_pin(&plan, &cat, Some(&live), 3006).is_err());
        live.me.as_mut().unwrap().player.items.push(InvItem {
            id: 3036,
            count: 1,
            ..Default::default()
        });
        assert!(validate_item_pin(&plan, &cat, Some(&live), 3036).is_err());
        assert!(validate_item_pin(&plan, &cat, Some(&live), 3033).is_err());
        live.me.as_mut().unwrap().player.champion = "Lux".into();
        assert!(validate_item_pin(&plan, &cat, Some(&live), 3031).is_err());
        live.me = None;
        assert!(validate_item_pin(&plan, &cat, Some(&live), 3031).is_err());
        assert!(validate_item_pin(&plan, &cat, None, 3031).is_ok());
    }

    #[test]
    fn a_free_quest_upgrade_can_be_pinned_but_an_owned_transformation_cannot() {
        use crate::engine::PlanItem;
        use crate::live::{InvItem, Me, Player};
        let cat = crate::ddragon::test_support::catalog();
        let mut plan = Plan {
            champion: "Lulu".into(),
            options: vec![PlanItem {
                id: 3870,
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut live = LiveSnapshot {
            me: Some(Me {
                player: Player {
                    champion: "Lulu".into(),
                    items: vec![InvItem {
                        id: 3867,
                        count: 1,
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(validate_item_pin(&plan, &cat, Some(&live), 3870).is_ok());
        plan.options[0].id = 3003;
        live.me.as_mut().unwrap().player.items[0].id = 3040;
        assert!(validate_item_pin(&plan, &cat, Some(&live), 3003).is_err());
    }

    #[test]
    fn rune_import_requires_the_current_catalogs_style_slots_and_shards() {
        let mut cat = crate::ddragon::test_support::catalog();
        cat.style_names.insert(8000, "Precision".into());
        cat.style_names.insert(8300, "Inspiration".into());
        for (id, style, slot) in [
            (8008, 8000, 0),
            (8009, 8000, 1),
            (9103, 8000, 2),
            (8014, 8000, 3),
            (8304, 8300, 1),
            (8345, 8300, 2),
        ] {
            cat.runes.insert(
                id,
                crate::ddragon::Rune {
                    id,
                    style,
                    slot,
                    ..Default::default()
                },
            );
        }
        let ids = RunePageIds {
            primary_style: 8000,
            sub_style: 8300,
            perks: vec![8008, 8009, 9103, 8014, 8304, 8345, 5005, 5008, 5001],
            ..Default::default()
        };
        assert!(validate_rune_page(&ids, &cat).is_ok());
        let mut bad = ids.clone();
        bad.perks[0] = 999999;
        assert!(validate_rune_page(&bad, &cat).is_err());
        bad = ids.clone();
        bad.sub_style = bad.primary_style;
        assert!(validate_rune_page(&bad, &cat).is_err());
        bad = ids.clone();
        bad.perks.swap(0, 1);
        assert!(validate_rune_page(&bad, &cat).is_err());
        bad = ids.clone();
        bad.perks[5] = bad.perks[4];
        assert!(validate_rune_page(&bad, &cat).is_err());
        bad = ids.clone();
        bad.perks[8] = 5005;
        assert!(validate_rune_page(&bad, &cat).is_err());
        bad = ids;
        bad.perks.pop();
        assert!(validate_rune_page(&bad, &cat).is_err());
    }

    #[test]
    fn an_unavailable_client_does_not_finish_a_journal() {
        assert!(!confirmed_game_end(None));
        assert!(!confirmed_game_end(Some("Reconnect")));
        assert!(!confirmed_game_end(Some("GameStart")));
        assert!(confirmed_game_end(Some("EndOfGame")));
        assert!(confirmed_game_end(Some("WaitingForStats")));
        assert!(confirmed_game_end(Some("Lobby")));
    }

    #[test]
    fn failed_imports_retry_independently_until_each_succeeds() {
        let mut imports = ImportTracker::default();
        imports.record_runes(498, "precision".into(), false);
        imports.record_spells(498, (4, 21), 1000, false);
        imports.record_itemset(498, "3508,3031".into(), false);
        assert!(imports.runes_needed(498, "precision"));
        assert!(imports.spells_needed(498, (4, 21), false));
        assert!(imports.itemset_needed(498, "3508,3031"));

        imports.record_runes(498, "precision".into(), true);
        assert!(!imports.runes_needed(498, "precision"));
        assert!(imports.spells_needed(498, (4, 21), false));
        assert!(imports.itemset_needed(498, "3508,3031"));
        imports.record_spells(498, (4, 21), 2000, true);
        imports.record_itemset(498, "3508,3031".into(), true);
        assert!(!imports.spells_needed(498, (4, 21), true));
        assert!(!imports.itemset_needed(498, "3508,3031"));
        assert!(imports.itemset_needed(498, "3508,3036"));
    }

    #[test]
    fn manual_spell_edit_prevents_later_adaptation() {
        let mut imports = ImportTracker::default();
        imports.observe_spells((4, 7), 0);
        imports.record_spells(498, (4, 21), 100, true);
        imports.observe_spells((4, 21), 1100);
        imports.observe_spells((4, 3), 2100);
        assert!(imports.manual_spell_override());
        assert!(!imports.spells_needed(498, (4, 1), true));
    }

    #[test]
    fn manual_spell_edit_before_build_arrives_is_preserved() {
        let mut imports = ImportTracker::default();
        imports.observe_spells((4, 7), 0);
        imports.observe_spells((4, 3), 1000);
        assert!(!imports.spells_needed(498, (4, 21), false));
    }

    #[test]
    fn late_spell_adaptation_waits_for_lock_and_ignores_one_old_observation() {
        let mut imports = ImportTracker::default();
        imports.observe_spells((4, 7), 0);
        imports.record_spells(498, (4, 21), 100, true);
        imports.observe_spells((4, 7), 1100);
        assert!(!imports.manual_spell_override());
        imports.observe_spells((4, 21), 2100);
        assert!(!imports.spells_needed(498, (4, 1), false));
        assert!(imports.spells_needed(498, (4, 1), true));
    }

    #[test]
    fn long_unacknowledged_spell_change_does_not_overwrite_the_player() {
        let mut imports = ImportTracker::default();
        imports.observe_spells((4, 7), 0);
        imports.record_spells(498, (4, 21), 100, true);
        imports.observe_spells((4, 7), 10_000);
        assert!(imports.manual_spell_override());
        assert!(!imports.spells_needed(498, (4, 1), true));
    }

    #[test]
    fn reset_allows_same_champion_to_import_in_the_next_match() {
        let mut imports = ImportTracker::default();
        imports.record_runes(498, "precision".into(), true);
        imports.record_spells(498, (4, 21), 100, true);
        imports.record_itemset(498, "3508,3031".into(), true);
        imports.observe_spells((4, 3), 1100);
        imports.reset();
        assert!(imports.runes_needed(498, "precision"));
        assert!(imports.spells_needed(498, (4, 21), false));
        assert!(imports.itemset_needed(498, "3508,3031"));
        assert!(!imports.manual_spell_override());
    }

    #[test]
    fn cache_refresh_expires_and_backoff_does_not_become_forever_cache() {
        let mut gate = RefreshGate::<u32>::default();
        let first = gate.begin(498, 100).unwrap();
        assert!(gate.begin(498, 101).is_none());
        assert!(gate.finish(&first, Some(1000), 0));
        assert!(gate.begin(498, 999).is_none());
        let expired = gate.begin(498, 1000).unwrap();
        assert!(gate.finish(&expired, None, 1500));
        assert!(gate.begin(498, 1499).is_none());
        assert!(gate.begin(498, 1500).is_some());
    }

    #[test]
    fn obsolete_refresh_cannot_replace_newer_champion_data() {
        let mut gate = RefreshGate::<u32>::default();
        let first = gate.begin(498, 100).unwrap();
        let second = gate.begin(222, 101).unwrap();
        assert!(!gate.finish(&first, Some(10_000), 0));
        assert!(gate.finish(&second, Some(1000), 0));
        assert!(gate.is_current(&222));
        assert!(!gate.is_current(&498));
    }

    #[test]
    fn changing_back_to_a_previous_key_still_rejects_its_old_request() {
        let mut gate = RefreshGate::<u32>::default();
        let old = gate.begin(498, 0).unwrap();
        gate.begin(222, 1).unwrap();
        let newest = gate.begin(498, 2).unwrap();
        assert!(!gate.finish(&old, Some(1000), 0));
        assert!(gate.finish(&newest, Some(1000), 0));
    }

    #[test]
    fn idle_reset_rejects_in_flight_aggregate() {
        let mut gate = RefreshGate::<u32>::default();
        let request = gate.begin(498, 0).unwrap();
        gate.clear();
        assert!(!gate.finish(&request, Some(1000), 0));
    }

    #[test]
    fn reconnect_keeps_match_but_next_same_champion_select_resets_it() {
        let mut session = SessionTracker::default();
        assert!(session.observe_phase(Some("ChampSelect"), Some("game-1")));
        let first = session.generation();
        assert!(!session.observe_phase(Some("GameStart"), None));
        assert!(!session.observe_phase(Some("InProgress"), None));
        assert!(!session.observe_live(800.0));
        assert!(!session.observe_phase(None, None));
        assert!(!session.observe_phase(Some("Reconnect"), None));
        assert_eq!(session.generation(), first);
        session.observe_phase(Some("EndOfGame"), None);
        assert!(session.observe_phase(Some("ChampSelect"), Some("game-2")));
        assert_eq!(session.generation(), first + 1);
    }

    #[test]
    fn session_id_change_and_game_clock_reset_detect_missed_boundaries() {
        let mut session = SessionTracker::default();
        session.observe_phase(Some("ChampSelect"), Some("game-1"));
        assert!(session.observe_phase(Some("ChampSelect"), Some("game-2")));
        session.observe_phase(Some("InProgress"), None);
        assert!(!session.observe_live(900.0));
        assert!(!session.observe_live(901.0));
        assert!(session.observe_live(10.0));
    }

    #[test]
    fn live_game_can_start_without_lcu_or_champion_select() {
        assert!(should_poll_live(None));
        let mut session = SessionTracker::default();
        assert!(session.observe_live(640.0));
        assert!(!session.observe_live(642.0));
        assert!(should_poll_live(Some("GameStart")));
        assert!(!should_poll_live(Some("EndOfGame")));
        assert!(!should_poll_live(Some("ChampSelect")));
    }

    #[test]
    fn missing_or_failed_live_observation_is_never_fresh() {
        let mut live = LiveFreshness::default();
        assert!(live.status(100).stale);
        assert_eq!(live.status(100).age_ms, None);
        live.observe(10.0, true, 100);
        assert!(!live.status(100).stale);
        live.miss();
        let failed = live.status(2100);
        assert!(failed.stale);
        assert_eq!(failed.age_ms, Some(2000));
        assert_eq!(failed.observed_at_ms, Some(100));
        live.observe(12.0, false, 2200);
        assert!(live.status(2200).stale);
        assert!(!live.status(2200).identity_known);
        assert_eq!(live.status(2200).age_ms, Some(2100));
    }

    #[test]
    fn a_frozen_game_clock_does_not_keep_old_observations_fresh() {
        let mut live = LiveFreshness::default();
        live.observe(10.0, true, 100);
        live.observe(10.0, true, 10_100);
        assert!(live.status(10_100).stale);
        assert_eq!(live.status(10_100).age_ms, Some(10_000));
        live.observe(12.0, true, 10_200);
        assert!(!live.status(10_200).stale);
    }

    #[test]
    fn invalid_game_time_or_clock_regression_has_unknown_age() {
        let mut live = LiveFreshness::default();
        live.observe(f64::NAN, true, 1000);
        assert!(live.status(1000).stale);
        live.observe(10.0, true, 1000);
        assert!(live.status(999).stale);
        assert_eq!(live.status(999).age_ms, None);
    }
}
