//! Bounded local records of recommendations and visible inventory additions; no outcome attribution.
use crate::coaching::DecisionKind;
use crate::ddragon::normalize;
use crate::engine::Plan;
use crate::live::LiveSnapshot;
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

pub const MAX_SESSIONS: usize = 20;
pub const MAX_DECISIONS: usize = 80;
pub const MAX_RECAP_DECISIONS: usize = 8;
pub const MAX_PURCHASES: usize = 160;
pub const MAX_JOURNAL_BYTES: usize = 2 * 1024 * 1024;
const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Feedback {
    Useful,
    NotUseful,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct DecisionRecord {
    pub id: String,
    pub game_time: f64,
    pub target_id: u32,
    pub target_name: String,
    pub buy_id: Option<u32>,
    pub buy_name: Option<String>,
    pub buy_affordable: bool,
    pub reason: String,
    pub lesson: String,
    pub kind: DecisionKind,
    pub remaining_cost: Option<u32>,
    pub gold: Option<f64>,
    #[serde(default)]
    pub feedback: Option<Feedback>,
}

/// A positive inventory delta. Gifts, transformations, undo, and purchases can all produce it.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct ObservedPurchase {
    pub game_time: f64,
    pub item_id: u32,
    pub item_name: String,
    pub count: u32,
    /// Always true: neither a shop transaction nor following a recommendation is established.
    pub observed_only: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Recap {
    pub session_id: String,
    pub champion: String,
    pub role: Option<String>,
    pub patch: String,
    pub source: Option<String>,
    pub engine_version: String,
    /// Last valid observed game time, not a claim about the moment the match actually ended.
    pub end_game_time: Option<f64>,
    pub decisions: Vec<DecisionRecord>,
    pub purchases: Vec<ObservedPurchase>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct DecisionSignature {
    target: u32,
    buy: Option<u32>,
    kind: DecisionKind,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct InventoryEntry {
    name: String,
    count: u32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct ActiveSession {
    recap: Recap,
    inventory: Option<BTreeMap<u32, InventoryEntry>>,
    last_decision: Option<DecisionSignature>,
}

/// No raw snapshots or player account identities are retained. Saving is explicit so callers
/// can persist a clone outside their live-state mutex and polling loop.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Journal {
    schema_version: u32,
    next_decision: u64,
    active: Option<ActiveSession>,
    history: Vec<Recap>,
}

impl Default for Journal {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            next_decision: 1,
            active: None,
            history: Vec::new(),
        }
    }
}

impl Journal {
    /// Repeating an active session ID preserves its baseline and decision IDs. A different ID
    /// closes the previous record using its last observation, without inferring an outcome.
    pub fn begin(
        &mut self,
        session_id: String,
        champion: String,
        role: Option<String>,
        patch: String,
        source: Option<String>,
    ) {
        if session_id.trim().is_empty() || champion.trim().is_empty() {
            return;
        }
        let session_id = bounded_session_id(&session_id);
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.recap.session_id == session_id)
        {
            return;
        }
        self.finish();
        self.active = Some(ActiveSession {
            recap: Recap {
                session_id,
                champion: bounded(&champion, 128),
                role: role.map(|role| bounded(&role, 32)),
                patch: bounded(&patch, 32),
                source: source.map(|source| bounded(&source, 256)),
                engine_version: crate::engine::ENGINE_VERSION.into(),
                ..Default::default()
            },
            ..Default::default()
        });
        keep_recent(&mut self.history, MAX_SESSIONS - 1);
    }

    /// True means a significant decision or an inventory addition was recorded. Time and
    /// baseline updates alone do not ask the caller to write a file every two seconds.
    pub fn observe(&mut self, snapshot: &LiveSnapshot, plan: &Plan) -> bool {
        let Some(active) = self.active.as_mut() else {
            return false;
        };
        let Some(me) = snapshot.me.as_ref() else {
            return false;
        };
        if !valid_time(snapshot.game_time)
            || normalize(&me.player.champion) != normalize(&active.recap.champion)
            || active
                .recap
                .end_game_time
                .is_some_and(|previous| snapshot.game_time < previous)
        {
            return false;
        }
        active.recap.end_game_time = Some(snapshot.game_time);
        let mut inventory: BTreeMap<u32, InventoryEntry> = BTreeMap::new();
        for item in me
            .player
            .items
            .iter()
            .filter(|item| item.id > 0 && item.count > 0)
            .take(64)
        {
            let entry = inventory.entry(item.id).or_insert_with(|| InventoryEntry {
                name: bounded(&item.name, 128),
                count: 0,
            });
            entry.count = entry.count.saturating_add(item.count);
        }
        let mut dirty = false;
        if let Some(previous) = &active.inventory {
            for (&id, item) in &inventory {
                let before = previous.get(&id).map(|item| item.count).unwrap_or(0);
                let added = item.count.saturating_sub(before);
                if added > 0 {
                    active.recap.purchases.push(ObservedPurchase {
                        game_time: snapshot.game_time,
                        item_id: id,
                        item_name: item.name.clone(),
                        count: added,
                        observed_only: true,
                    });
                    dirty = true;
                }
            }
            keep_recent(&mut active.recap.purchases, MAX_PURCHASES);
        }
        active.inventory = Some(inventory);
        let Some(next) = plan.next.as_ref().filter(|next| next.id > 0) else {
            active.last_decision = None;
            return dirty;
        };
        if normalize(&plan.champion) != normalize(&active.recap.champion) {
            return dirty;
        }
        let kind = plan
            .learning
            .as_ref()
            .map(|tip| tip.kind)
            .unwrap_or_default();
        let buy = next.buy_now.as_ref().filter(|item| item.id > 0);
        let signature = DecisionSignature {
            target: next.id,
            buy: buy.map(|item| item.id),
            kind,
        };
        if active.last_decision.as_ref() == Some(&signature) {
            return dirty;
        }
        if active.recap.source.is_none() {
            active.recap.source = plan.source.as_ref().map(|source| bounded(source, 256));
        }
        let id_input = format!(
            "recall.learning.v1:{}:{}",
            active.recap.session_id, self.next_decision
        );
        let id = Uuid::new_v5(&Uuid::NAMESPACE_OID, id_input.as_bytes()).to_string();
        self.next_decision = self.next_decision.saturating_add(1);
        let reason = plan
            .learning
            .as_ref()
            .map(|tip| tip.reason.as_str())
            .or_else(|| plan.why.first().map(String::as_str))
            .unwrap_or("Current recommended target");
        active.recap.decisions.push(DecisionRecord {
            id,
            game_time: snapshot.game_time,
            target_id: next.id,
            target_name: bounded(&next.name, 128),
            buy_id: buy.map(|item| item.id),
            buy_name: buy.map(|item| bounded(&item.name, 128)),
            buy_affordable: buy.is_some() && next.buy_now_affordable,
            reason: bounded(reason, 512),
            lesson: plan
                .learning
                .as_ref()
                .map(|tip| bounded(&tip.lesson, 1024))
                .unwrap_or_default(),
            kind,
            remaining_cost: next.price_known.then_some(next.remaining_cost),
            gold: me
                .gold
                .is_finite()
                .then_some(me.gold)
                .filter(|gold| *gold >= 0.0),
            feedback: None,
        });
        keep_recent(&mut active.recap.decisions, MAX_DECISIONS);
        active.last_decision = Some(signature);
        true
    }

    pub fn finish(&mut self) -> Option<Recap> {
        let active = self.active.take()?;
        self.history.push(active.recap);
        keep_recent(&mut self.history, MAX_SESSIONS);
        self.recap()
    }

    pub fn feedback(&mut self, decision_id: &str, feedback: Feedback) -> bool {
        let active = self
            .active
            .iter_mut()
            .flat_map(|active| active.recap.decisions.iter_mut());
        let history = self
            .history
            .iter_mut()
            .rev()
            .flat_map(|recap| recap.decisions.iter_mut());
        for decision in active.chain(history) {
            if decision.id == decision_id {
                if decision.feedback == Some(feedback) {
                    return false;
                }
                decision.feedback = Some(feedback);
                return true;
            }
        }
        false
    }

    /// The most recently finished match, with only its last eight significant decisions for UI.
    pub fn recap(&self) -> Option<Recap> {
        let mut recap = self.history.last()?.clone();
        keep_recent(&mut recap.decisions, MAX_RECAP_DECISIONS);
        Some(recap)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default())
            }
            Err(error) => {
                return Err(error).with_context(|| format!("open journal {}", path.display()))
            }
        };
        ensure!(
            file.metadata()?.len() <= MAX_JOURNAL_BYTES as u64,
            "journal exceeds the 2 MB limit"
        );
        let mut bytes = Vec::new();
        file.take(MAX_JOURNAL_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= MAX_JOURNAL_BYTES,
            "journal grew beyond the 2 MB limit"
        );
        let mut journal: Self = serde_json::from_slice(&bytes).context("invalid journal JSON")?;
        ensure!(
            journal.schema_version == SCHEMA_VERSION,
            "unsupported journal schema {}",
            journal.schema_version
        );
        ensure!(
            journal.next_decision > 0 && journal.next_decision < u64::MAX,
            "invalid journal decision sequence"
        );
        journal.enforce_bounds();
        journal.validate()?;
        Ok(journal)
    }

    /// Write a bounded copy beside the destination, then atomically replace it. Oldest retained
    /// sessions are removed if large explanations would otherwise exceed the file byte limit.
    pub fn save(&self, path: &Path) -> Result<()> {
        let mut journal = self.clone();
        journal.enforce_bounds();
        journal.validate()?;
        let mut bytes = serde_json::to_vec(&journal)?;
        while bytes.len() > MAX_JOURNAL_BYTES && !journal.history.is_empty() {
            journal.history.remove(0);
            bytes = serde_json::to_vec(&journal)?;
        }
        atomic_write(path, &bytes)
    }

    fn enforce_bounds(&mut self) {
        keep_recent(
            &mut self.history,
            if self.active.is_some() {
                MAX_SESSIONS - 1
            } else {
                MAX_SESSIONS
            },
        );
        for recap in self
            .history
            .iter_mut()
            .chain(self.active.iter_mut().map(|active| &mut active.recap))
        {
            recap.session_id = bounded_session_id(&recap.session_id);
            recap.champion = bounded(&recap.champion, 128);
            recap.role = recap.role.take().map(|role| bounded(&role, 32));
            recap.patch = bounded(&recap.patch, 32);
            recap.source = recap.source.take().map(|source| bounded(&source, 256));
            recap.engine_version = bounded(&recap.engine_version, 32);
            keep_recent(&mut recap.decisions, MAX_DECISIONS);
            keep_recent(&mut recap.purchases, MAX_PURCHASES);
            for record in &mut recap.decisions {
                record.target_name = bounded(&record.target_name, 128);
                record.buy_name = record.buy_name.take().map(|name| bounded(&name, 128));
                record.reason = bounded(&record.reason, 512);
                record.lesson = bounded(&record.lesson, 1024);
                record.gold = record.gold.filter(|gold| gold.is_finite() && *gold >= 0.0);
            }
            for purchase in &mut recap.purchases {
                purchase.item_name = bounded(&purchase.item_name, 128);
                purchase.observed_only = true;
            }
        }
        if let Some(inventory) = self
            .active
            .as_mut()
            .and_then(|active| active.inventory.as_mut())
        {
            while inventory.len() > 64 {
                inventory.pop_last();
            }
            for item in inventory.values_mut() {
                item.name = bounded(&item.name, 128);
            }
        }
    }

    fn validate(&self) -> Result<()> {
        let mut ids = HashSet::new();
        for recap in self
            .history
            .iter()
            .chain(self.active.iter().map(|active| &active.recap))
        {
            ensure!(
                !recap.session_id.is_empty() && !recap.champion.is_empty(),
                "journal session metadata is missing"
            );
            ensure!(
                recap.end_game_time.is_none_or(valid_time),
                "invalid journal game time"
            );
            for decision in &recap.decisions {
                ensure!(
                    valid_time(decision.game_time) && decision.target_id > 0,
                    "invalid journal decision"
                );
                ensure!(
                    !decision.id.is_empty()
                        && decision.id.len() <= 128
                        && ids.insert(decision.id.as_str()),
                    "invalid or duplicate journal decision ID"
                );
            }
            for purchase in &recap.purchases {
                ensure!(
                    valid_time(purchase.game_time) && purchase.item_id > 0 && purchase.count > 0,
                    "invalid inventory observation"
                );
            }
        }
        Ok(())
    }
}

fn valid_time(time: f64) -> bool {
    time.is_finite() && time >= 0.0
}

fn keep_recent<T>(values: &mut Vec<T>, limit: usize) {
    if values.len() > limit {
        values.drain(..values.len() - limit);
    }
}

fn bounded(value: &str, bytes: usize) -> String {
    let mut end = bytes.min(value.len());
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

fn bounded_session_id(id: &str) -> String {
    if id.len() <= 128 {
        id.to_string()
    } else {
        Uuid::new_v5(&Uuid::NAMESPACE_OID, id.as_bytes()).to_string()
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    ensure!(
        bytes.len() <= MAX_JOURNAL_BYTES,
        "journal exceeds the 2 MB limit"
    );
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let filename = path.file_name().context("journal path must name a file")?;
    fs::create_dir_all(parent)?;
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    for _ in 0..16 {
        let mut temp_name = filename.to_os_string();
        temp_name.push(format!(
            ".{}.{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let temporary = parent.join(temp_name);
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error).context("create journal temporary file"),
        };
        let result = (|| -> Result<()> {
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path).context("replace journal file")
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        return result;
    }
    bail!("could not reserve a journal temporary file")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coaching::{DecisionKind, LearningTip};
    use crate::engine::{Component, NextItem};
    use crate::live::{InvItem, Me, Player};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn snapshot(time: f64, items: &[(u32, u32)]) -> LiveSnapshot {
        LiveSnapshot {
            game_time: time,
            mode: "CLASSIC".into(),
            me: Some(Me {
                player: Player {
                    name: "private-player-identity#TEST".into(),
                    champion: "Ahri".into(),
                    level: 8,
                    items: items
                        .iter()
                        .enumerate()
                        .map(|(slot, (id, count))| InvItem {
                            id: *id,
                            name: format!("Observed item {id}"),
                            count: *count,
                            slot: slot as u32,
                        })
                        .collect(),
                    ..Default::default()
                },
                gold: 700.0,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn plan(target: u32, buy: Option<u32>, kind: DecisionKind) -> Plan {
        Plan {
            champion: "Ahri".into(),
            next: Some(NextItem {
                id: target,
                name: format!("Target {target}"),
                remaining_cost: 3000,
                price_known: true,
                buy_now: buy.map(|id| Component {
                    id,
                    name: format!("Component {id}"),
                    cost: 1200,
                    owned: false,
                }),
                ..Default::default()
            }),
            learning: Some(LearningTip {
                kind,
                reason: "A relevant item for the current plan".into(),
                lesson: "Compare the cost left to spend.".into(),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn started() -> Journal {
        let mut journal = Journal::default();
        journal.begin(
            "match-1".into(),
            "Ahri".into(),
            Some("Mid".into()),
            "16.17".into(),
            Some("op.gg global emerald+".into()),
        );
        journal
    }

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "recall-journal-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn file(&self) -> std::path::PathBuf {
            self.0.join("journal.json")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn gold_prices_and_reason_numbers_do_not_duplicate_a_decision() {
        let mut journal = started();
        let first = snapshot(100.0, &[]);
        let mut recommendation = plan(3089, Some(1058), DecisionKind::Core);
        assert!(journal.observe(&first, &recommendation));
        let mut second = snapshot(102.0, &[]);
        second.me.as_mut().unwrap().gold = 1200.0;
        recommendation.next.as_mut().unwrap().remaining_cost = 2500;
        recommendation.next.as_mut().unwrap().buy_now_affordable = true;
        recommendation.learning.as_mut().unwrap().reason = "Only 2500 gold left now".into();
        assert!(!journal.observe(&second, &recommendation));
        let recap = journal.finish().unwrap();
        assert_eq!(recap.decisions.len(), 1);
        assert_eq!(recap.decisions[0].gold, Some(700.0));
        assert_eq!(recap.decisions[0].remaining_cost, Some(3000));
        assert_eq!(recap.end_game_time, Some(102.0));
    }

    #[test]
    fn target_component_and_reason_category_changes_are_significant() {
        let mut journal = started();
        assert!(journal.observe(
            &snapshot(100.0, &[]),
            &plan(3089, Some(1058), DecisionKind::Core)
        ));
        assert!(journal.observe(
            &snapshot(102.0, &[]),
            &plan(3089, Some(1052), DecisionKind::Core)
        ));
        assert!(journal.observe(
            &snapshot(104.0, &[]),
            &plan(3089, Some(1052), DecisionKind::Completion)
        ));
        assert!(journal.observe(
            &snapshot(106.0, &[]),
            &plan(3135, Some(1052), DecisionKind::Completion)
        ));
        let recap = journal.finish().unwrap();
        assert_eq!(recap.decisions.len(), 4);
        assert_eq!(recap.decisions[1].buy_id, Some(1052));
        assert_eq!(recap.decisions[2].kind, DecisionKind::Completion);
        assert_eq!(recap.decisions[3].target_id, 3135);
    }

    #[test]
    fn returning_to_an_earlier_action_keeps_both_distinct_occurrences() {
        let mut journal = started();
        journal.observe(&snapshot(100.0, &[]), &plan(3089, None, DecisionKind::Core));
        journal.observe(
            &snapshot(200.0, &[]),
            &plan(3135, None, DecisionKind::MagicPen),
        );
        journal.observe(&snapshot(300.0, &[]), &plan(3089, None, DecisionKind::Core));
        let recap = journal.finish().unwrap();
        assert_eq!(recap.decisions.len(), 3);
        assert_ne!(recap.decisions[0].id, recap.decisions[2].id);
    }

    #[test]
    fn initial_midgame_inventory_is_baseline_not_a_purchase() {
        let mut journal = started();
        journal.observe(
            &snapshot(900.0, &[(3089, 1), (3020, 1)]),
            &plan(3135, None, DecisionKind::MagicPen),
        );
        assert!(journal.finish().unwrap().purchases.is_empty());
    }

    #[test]
    fn inventory_additions_preserve_quantity_and_ignore_moves_and_removals() {
        let mut journal = started();
        let recommendation = plan(3089, None, DecisionKind::Core);
        journal.observe(&snapshot(100.0, &[(1052, 1), (2003, 2)]), &recommendation);
        assert!(!journal.observe(&snapshot(102.0, &[(2003, 2), (1052, 1)]), &recommendation));
        assert!(journal.observe(
            &snapshot(104.0, &[(1052, 2), (2003, 1), (1058, 1)]),
            &recommendation
        ));
        let recap = journal.finish().unwrap();
        assert_eq!(
            recap
                .purchases
                .iter()
                .map(|p| (p.item_id, p.count))
                .collect::<Vec<_>>(),
            vec![(1052, 1), (1058, 1)]
        );
        assert!(recap.purchases.iter().all(|p| p.observed_only));
    }

    #[test]
    fn missing_identity_does_not_create_an_empty_inventory_baseline() {
        let mut journal = started();
        let recommendation = plan(3089, None, DecisionKind::Core);
        assert!(!journal.observe(
            &LiveSnapshot {
                game_time: 100.0,
                ..Default::default()
            },
            &recommendation
        ));
        journal.observe(&snapshot(102.0, &[(3089, 1)]), &recommendation);
        assert!(journal.finish().unwrap().purchases.is_empty());
    }

    #[test]
    fn wrong_champion_and_out_of_order_snapshots_do_not_mix_sessions() {
        let mut journal = started();
        let recommendation = plan(3089, None, DecisionKind::Core);
        journal.observe(&snapshot(100.0, &[(1052, 1)]), &recommendation);
        assert!(!journal.observe(&snapshot(90.0, &[(1052, 3)]), &recommendation));
        let mut other = snapshot(110.0, &[(1052, 4)]);
        other.me.as_mut().unwrap().player.champion = "Jinx".into();
        assert!(!journal.observe(&other, &recommendation));
        let recap = journal.finish().unwrap();
        assert_eq!(recap.end_game_time, Some(100.0));
        assert!(recap.purchases.is_empty());
    }

    #[test]
    fn invalid_time_is_skipped_and_invalid_gold_is_unknown() {
        let mut journal = started();
        let recommendation = plan(3089, None, DecisionKind::Core);
        assert!(!journal.observe(&snapshot(f64::NAN, &[]), &recommendation));
        assert!(!journal.observe(&snapshot(-1.0, &[]), &recommendation));
        let mut valid = snapshot(100.0, &[]);
        valid.me.as_mut().unwrap().gold = f64::INFINITY;
        assert!(journal.observe(&valid, &recommendation));
        let recap = journal.finish().unwrap();
        assert_eq!(recap.decisions[0].gold, None);
        serde_json::to_string(&recap).unwrap();
    }

    #[test]
    fn a_missing_plan_still_records_observed_inventory_additions() {
        let mut journal = started();
        assert!(!journal.observe(&snapshot(100.0, &[]), &Plan::default()));
        assert!(journal.observe(&snapshot(102.0, &[(1052, 1)]), &Plan::default()));
        let recap = journal.finish().unwrap();
        assert!(recap.decisions.is_empty());
        assert_eq!(recap.purchases[0].item_id, 1052);
    }

    #[test]
    fn decision_ids_dedup_and_feedback_survive_an_active_session_reload() {
        let temp = TempDir::new();
        let mut journal = started();
        let recommendation = plan(3089, Some(1058), DecisionKind::Core);
        journal.observe(&snapshot(100.0, &[]), &recommendation);
        let first_id = journal.active.as_ref().unwrap().recap.decisions[0]
            .id
            .clone();
        journal.save(&temp.file()).unwrap();
        let mut loaded = Journal::load(&temp.file()).unwrap();
        assert!(!loaded.observe(&snapshot(102.0, &[]), &recommendation));
        assert!(loaded.feedback(&first_id, Feedback::Useful));
        assert!(!loaded.feedback(&first_id, Feedback::Useful));
        assert!(!loaded.feedback("does-not-exist", Feedback::NotUseful));
        assert!(loaded.observe(
            &snapshot(104.0, &[]),
            &plan(3135, None, DecisionKind::MagicPen)
        ));
        let recap = loaded.finish().unwrap();
        assert_eq!(recap.decisions[0].id, first_id);
        assert_ne!(recap.decisions[1].id, first_id);
        assert_eq!(recap.decisions[0].feedback, Some(Feedback::Useful));
        loaded.save(&temp.file()).unwrap();
        let round_trip = Journal::load(&temp.file()).unwrap();
        assert_eq!(round_trip.recap(), loaded.recap());
        assert!(!std::fs::read_to_string(temp.file())
            .unwrap()
            .contains("private-player-identity"));
    }

    #[test]
    fn begin_is_idempotent_and_finishes_a_previous_match_without_outcome_claims() {
        let mut journal = started();
        journal.observe(&snapshot(100.0, &[]), &plan(3089, None, DecisionKind::Core));
        journal.begin(
            "match-1".into(),
            "Ahri".into(),
            Some("Mid".into()),
            "16.17".into(),
            None,
        );
        assert_eq!(journal.active.as_ref().unwrap().recap.decisions.len(), 1);
        journal.begin(
            "match-2".into(),
            "Ahri".into(),
            Some("Mid".into()),
            "16.17".into(),
            None,
        );
        assert_eq!(journal.history.len(), 1);
        assert_eq!(journal.history[0].end_game_time, Some(100.0));
        journal.finish().unwrap();
        assert!(journal.finish().is_none());
        assert_eq!(journal.recap().unwrap().session_id, "match-2");
    }

    #[test]
    fn storage_bounds_sessions_and_decisions_but_recap_shows_only_recent_eight() {
        let mut journal = Journal::default();
        for session in 0..24 {
            journal.begin(
                format!("match-{session}"),
                "Ahri".into(),
                None,
                "16.17".into(),
                None,
            );
            for decision in 0..90 {
                journal.observe(
                    &snapshot(decision as f64, &[]),
                    &plan(1000 + decision, None, DecisionKind::Core),
                );
            }
            journal.finish().unwrap();
        }
        assert_eq!(journal.history.len(), 20);
        assert_eq!(journal.history[0].session_id, "match-4");
        assert_eq!(journal.history[0].decisions.len(), 80);
        assert_eq!(journal.history[0].decisions[0].target_id, 1010);
        let recap = journal.recap().unwrap();
        assert_eq!(recap.decisions.len(), 8);
        assert_eq!(recap.decisions[0].target_id, 1082);
    }

    #[test]
    fn the_active_match_counts_toward_the_twenty_session_limit() {
        let mut journal = Journal::default();
        for session in 0..20 {
            journal.begin(
                format!("match-{session}"),
                "Ahri".into(),
                None,
                "16.17".into(),
                None,
            );
            journal.finish();
        }
        journal.begin(
            "active-match".into(),
            "Ahri".into(),
            None,
            "16.17".into(),
            None,
        );
        assert!(journal.active.is_some());
        assert_eq!(journal.history.len(), 19);
        assert_eq!(journal.history[0].session_id, "match-1");
    }

    #[test]
    fn observed_purchase_history_is_bounded_for_long_sessions() {
        let mut journal = started();
        for count in 0..200 {
            journal.observe(
                &snapshot(count as f64, &[(1052, count + 1)]),
                &Plan::default(),
            );
        }
        let recap = journal.finish().unwrap();
        assert_eq!(recap.purchases.len(), 160);
        assert!(recap.purchases.iter().all(|purchase| purchase.count == 1));
    }

    #[test]
    fn large_text_is_bounded_on_utf8_boundaries_and_saved_file_stays_under_two_megabytes() {
        let temp = TempDir::new();
        let mut journal = Journal::default();
        for session in 0..20 {
            journal.begin(
                format!("match-{session}"),
                "Ahri".into(),
                None,
                "16.17".into(),
                Some("來源".repeat(1000)),
            );
            for decision in 0..80 {
                let mut recommendation = plan(1000 + decision, Some(1058), DecisionKind::Core);
                recommendation.learning.as_mut().unwrap().reason = "é".repeat(10_000);
                recommendation.learning.as_mut().unwrap().lesson = "學".repeat(10_000);
                journal.observe(&snapshot(decision as f64, &[]), &recommendation);
            }
            journal.finish();
        }
        journal.save(&temp.file()).unwrap();
        assert!(std::fs::metadata(temp.file()).unwrap().len() <= 2 * 1024 * 1024);
        let loaded = Journal::load(&temp.file()).unwrap();
        assert_eq!(loaded.recap().unwrap().session_id, "match-19");
        assert!(loaded.recap().unwrap().decisions[0].reason.len() <= 512);
        assert!(loaded.recap().unwrap().decisions[0].lesson.len() <= 1024);
    }

    #[test]
    fn load_rejects_oversized_or_corrupt_files_and_missing_file_is_empty() {
        let temp = TempDir::new();
        assert!(Journal::load(&temp.file()).unwrap().recap().is_none());
        std::fs::write(temp.file(), b"{ broken").unwrap();
        assert!(Journal::load(&temp.file()).is_err());
        std::fs::write(temp.file(), vec![b' '; 2 * 1024 * 1024 + 1]).unwrap();
        assert!(Journal::load(&temp.file()).is_err());
    }

    #[test]
    fn rejected_oversized_write_preserves_the_last_readable_file() {
        let temp = TempDir::new();
        let mut journal = started();
        journal.observe(&snapshot(100.0, &[]), &plan(3089, None, DecisionKind::Core));
        journal.finish();
        journal.save(&temp.file()).unwrap();
        let original = std::fs::read(temp.file()).unwrap();
        assert!(atomic_write(&temp.file(), &vec![b'x'; 2 * 1024 * 1024 + 1]).is_err());
        assert_eq!(std::fs::read(temp.file()).unwrap(), original);
        assert_eq!(
            Journal::load(&temp.file()).unwrap().recap(),
            journal.recap()
        );
        assert_eq!(std::fs::read_dir(&temp.0).unwrap().count(), 1);
    }
}
