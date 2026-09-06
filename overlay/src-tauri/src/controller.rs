//! Local planning transactions shared by observations and webview commands. No remote I/O.
use crate::App;
use recall_core::aggregate::{Aggregate, Position};
use recall_core::ddragon::{normalize, Catalog};
use recall_core::engine::{self, BuildPreference, Inputs, Plan, PlannerPreferences};
use recall_core::journal::Feedback;
use recall_core::live::LiveSnapshot;
use recall_core::session;
use std::sync::Arc;
use tauri::AppHandle;

#[derive(Default)]
pub struct RecommendationSession {
    pub generation: u64,
    pub id: Option<String>,
    pub champion: Option<String>,
    journal_started: bool,
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|time| time.as_millis() as u64)
        .unwrap_or(0)
}

/// Call under App::planning. New boundaries invalidate in-flight import acknowledgments too.
pub fn reset_session_locked(app: &AppHandle, st: &App) {
    finish_journal_locked(app, st);
    let generation = st.session.lock().unwrap().generation.wrapping_add(1);
    *st.session.lock().unwrap() = RecommendationSession {
        generation,
        ..Default::default()
    };
    *st.preferences.lock().unwrap() = PlannerPreferences::default();
    *st.latest_live.lock().unwrap() = None;
    *st.lobby_observed_at_ms.lock().unwrap() = None;
}

/// Returns true for an actual champion change, not the initial identification of a new match.
pub fn ensure_champion_locked(app: &AppHandle, st: &App, champion: Option<&str>) -> bool {
    let champion = champion.filter(|name| !name.is_empty()).map(str::to_string);
    let changed = st
        .session
        .lock()
        .unwrap()
        .champion
        .as_ref()
        .is_some_and(|previous| {
            champion
                .as_ref()
                .is_none_or(|name| normalize(previous) != normalize(name))
        });
    if changed {
        reset_session_locked(app, st);
    }
    let mut current = st.session.lock().unwrap();
    current.champion = champion;
    if current.id.is_none() && current.champion.is_some() {
        current.id = Some(format!(
            "local-{}-{}-{}",
            now_ms(),
            std::process::id(),
            current.generation
        ));
    }
    changed
}

pub fn finish_journal_locked(app: &AppHandle, st: &App) {
    let finished = {
        let mut journal = st.journal.lock().unwrap();
        journal.finish().map(|recap| (recap, journal.clone()))
    };
    st.session.lock().unwrap().journal_started = false;
    if let Some((recap, journal)) = finished {
        st.journal_sink.queue(journal);
        st.update(app, |panel| panel.recap = Some(recap));
    }
}

/// Call under App::planning after validating the current observation.
pub fn compute_locked(
    st: &App,
    catalog: &Catalog,
    champion: &str,
    aggregate: Option<&Aggregate>,
    enemies: &[String],
    live: Option<&LiveSnapshot>,
) -> Plan {
    let preferences = st.preferences.lock().unwrap().clone();
    engine::plan_with_preferences(
        &Inputs {
            champion,
            pack: st.pack_for(champion),
            aggregate,
            traits: &st.traits,
            catalog,
            enemies,
            live,
        },
        &preferences,
    )
}

/// A completed or rejected pin is cleared by the engine's effective preferences.
pub fn store_plan_locked(st: &App, catalog: &Catalog, plan: &Plan, live: Option<&LiveSnapshot>) {
    *st.preferences.lock().unwrap() = plan.preferences.clone();
    *st.plan.lock().unwrap() = Some(plan.clone());
    let Some(live) = live else { return };
    if plan.path.is_empty() {
        return;
    }
    let (id, begin) = {
        let mut current = st.session.lock().unwrap();
        let Some(id) = current.id.clone() else { return };
        if !current
            .champion
            .as_ref()
            .is_some_and(|champion| normalize(champion) == normalize(&plan.champion))
        {
            return;
        }
        let begin = !current.journal_started;
        current.journal_started = true;
        (id, begin)
    };
    let dirty = {
        let mut journal = st.journal.lock().unwrap();
        if begin {
            journal.begin(
                id,
                plan.champion.clone(),
                plan.position.clone(),
                catalog.version.clone(),
                plan.source.clone(),
            );
        }
        let changed = journal.observe(live, plan);
        (begin || changed).then(|| journal.clone())
    };
    if let Some(journal) = dirty {
        st.journal_sink.queue(journal);
    }
}

#[derive(Clone, Copy)]
pub enum PreferenceChange {
    Mode(BuildPreference),
    Pin(u32),
    ClearPin,
}

struct Context {
    champion: String,
    catalog: Arc<Catalog>,
    aggregate: Option<Arc<Aggregate>>,
    enemies: Vec<String>,
    live: Option<LiveSnapshot>,
}

/// All locks are short copies, under the planning transaction; no network awaits here.
fn context_locked(st: &App) -> Result<Context, String> {
    let panel = st.snapshot();
    let champion = panel
        .champion
        .filter(|name| !name.is_empty())
        .ok_or("Choose a champion or wait for your live identity")?;
    let catalog = st
        .catalog
        .lock()
        .unwrap()
        .clone()
        .ok_or("Item data is not loaded yet")?;
    let lobby = st.lobby.lock().unwrap().clone();
    let (live, enemies, position) = match panel.phase.as_str() {
        "ingame" => {
            if !panel
                .live_source
                .as_ref()
                .is_some_and(|source| session::fresh_identity_at(source, now_ms()))
            {
                return Err(
                    "Game data is stale; wait for a fresh observation before changing the build"
                        .into(),
                );
            }
            let live = st
                .latest_live
                .lock()
                .unwrap()
                .clone()
                .ok_or("Waiting for a fresh live snapshot")?;
            let me = live
                .me
                .as_ref()
                .filter(|me| normalize(&me.player.champion) == normalize(&champion))
                .ok_or("Your live identity changed; wait for the next observation")?;
            let position = Position::parse(&me.player.position).or_else(|| {
                lobby
                    .as_ref()
                    .and_then(|lobby| Position::parse(&lobby.my_position))
            });
            let enemies = if live.enemies.is_empty() {
                lobby
                    .as_ref()
                    .map(|lobby| {
                        lobby
                            .enemies
                            .iter()
                            .map(|id| catalog.champion_name(*id))
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                live.enemies
                    .iter()
                    .map(|player| player.champion.clone())
                    .collect()
            };
            (Some(live), enemies, position)
        }
        "champselect" => {
            let lobby = lobby
                .filter(|lobby| {
                    lobby.my_cell >= 0
                        && lobby.my_champion > 0
                        && catalog.champion_key(&champion) == Some(lobby.my_champion)
                })
                .ok_or("Your champion-select identity is unavailable")?;
            let observed = *st.lobby_observed_at_ms.lock().unwrap();
            if !observed
                .and_then(|at| now_ms().checked_sub(at))
                .is_some_and(|age| age <= 6000)
            {
                return Err("Champion select data is stale; wait for a fresh observation".into());
            }
            let enemies = lobby
                .enemies
                .iter()
                .map(|id| catalog.champion_name(*id))
                .collect();
            (None, enemies, Position::parse(&lobby.my_position))
        }
        _ => {
            return Err("Build controls are available during champion select or a live game".into())
        }
    };
    let (region, tier) = {
        let settings = st.settings.lock().unwrap();
        (
            settings.region.to_ascii_lowercase(),
            settings.tier.to_ascii_lowercase(),
        )
    };
    let aggregate = {
        let aggregate = st.aggregate.lock().unwrap();
        let key = session::AggregateKey {
            champion_key: catalog.champion_key(&champion).unwrap_or(0),
            position,
            region,
            tier,
        };
        aggregate
            .refresh
            .is_current(&key)
            .then(|| aggregate.value.clone())
            .flatten()
    };
    Ok(Context {
        champion,
        catalog,
        aggregate,
        enemies,
        live,
    })
}

pub fn change_preference(
    app: &AppHandle,
    st: &App,
    change: PreferenceChange,
) -> Result<(), String> {
    let _planning = st.planning.lock().unwrap();
    let context = context_locked(st)?;
    let mut preferences = st.preferences.lock().unwrap().clone();
    match change {
        PreferenceChange::Mode(mode) => preferences.mode = mode,
        PreferenceChange::ClearPin => preferences.pinned_item = None,
        PreferenceChange::Pin(id) => {
            let current = st
                .plan
                .lock()
                .unwrap()
                .clone()
                .ok_or("No build is available to pin yet")?;
            session::validate_item_pin(&current, &context.catalog, context.live.as_ref(), id)?;
            preferences.pinned_item = Some(id);
        }
    }
    let plan = engine::plan_with_preferences(
        &Inputs {
            champion: &context.champion,
            pack: st.pack_for(&context.champion),
            aggregate: context.aggregate.as_deref(),
            traits: &st.traits,
            catalog: &context.catalog,
            enemies: &context.enemies,
            live: context.live.as_ref(),
        },
        &preferences,
    );
    if let PreferenceChange::Pin(id) = change {
        if plan.path.is_empty() || plan.preferences.pinned_item != Some(id) {
            return Err("That target is no longer compatible with the latest build".into());
        }
    }
    store_plan_locked(st, &context.catalog, &plan, context.live.as_ref());
    st.update(app, |panel| {
        panel.supported = !plan.path.is_empty();
        panel.message = if panel.supported {
            None
        } else {
            plan.note.clone()
        };
        panel.plan = Some(plan);
    });
    Ok(())
}

pub fn rate_decision(
    app: &AppHandle,
    st: &App,
    decision_id: &str,
    feedback: Feedback,
) -> Result<(), String> {
    let _planning = st.planning.lock().unwrap();
    let (journal, recap) = {
        let mut journal = st.journal.lock().unwrap();
        // Repeated clicks are idempotent; unknown IDs are rejected instead of silently accepted.
        let current = journal.recap();
        let known = current.as_ref().and_then(|recap| {
            recap
                .decisions
                .iter()
                .find(|decision| decision.id == decision_id)
        });
        if known.is_none() {
            return Err("That decision is no longer in the current recap".into());
        }
        if known.and_then(|decision| decision.feedback) == Some(feedback) {
            return Ok(());
        }
        if !journal.feedback(decision_id, feedback) {
            return Err("That decision could not be updated".into());
        }
        (journal.clone(), journal.recap())
    };
    st.journal_sink.queue(journal);
    st.update(app, |panel| panel.recap = recap);
    Ok(())
}
