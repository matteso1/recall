//! Background loop: find the client, follow the gameflow, poll champ select / live data, fetch the
//! aggregate build for the champion in play, run the engine, publish panel state, auto-import.
use crate::{controller, App};
use featherstorm_core::aggregate::{self, Aggregate, Position};
use featherstorm_core::champselect::{self, Lobby};
use featherstorm_core::ddragon::{self, Catalog};
use featherstorm_core::engine::Plan;
use featherstorm_core::lcu::Lcu;
use featherstorm_core::live::{self, LiveClient, LiveSnapshot};
use featherstorm_core::pack::ChampionPack;
use featherstorm_core::runes;
use featherstorm_core::session::{
    self, AggregateKey, ImportTracker, LiveFreshness, SessionTracker,
};
use featherstorm_core::state::{Flash, LiveView, LobbyView, SourceStatus};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tauri::AppHandle;
use tokio::sync::watch;
use tokio::task::JoinHandle;

const AGGREGATE_RETRY_MS: u64 = 30_000;
const CLIENT_MAX_AGE_MS: u64 = 8000;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn compute(
    st: &App,
    catalog: &Catalog,
    champion: &str,
    _pack: Option<&ChampionPack>,
    aggregate: Option<&Aggregate>,
    enemies: &[String],
    live: Option<&LiveSnapshot>,
) -> Plan {
    controller::compute_locked(st, catalog, champion, aggregate, enemies, live)
}

/// One line for the log: the path with tags, the NEXT item, spells, runes and the why lines.
fn plan_summary(plan: &Plan) -> String {
    let path: Vec<String> = plan
        .path
        .iter()
        .map(|p| match &p.tag {
            Some(t) => format!("{} ({t})", p.short),
            None => p.short.clone(),
        })
        .collect();
    let next = match &plan.next {
        Some(n) => match &n.buy_now {
            Some(c) if c.id != n.id => format!("{} (buy {})", n.name, c.name),
            _ => n.name.clone(),
        },
        None => "-".to_string(),
    };
    format!(
        "path {}; next {next}; spells {:?}; runes {}; decision {:?}",
        path.join(" > "),
        plan.spells,
        plan.runes_summary,
        plan.learning.as_ref().map(|tip| tip.kind)
    )
}

fn lobby_view(lobby: &Lobby, catalog: &Catalog) -> LobbyView {
    LobbyView {
        allies: lobby
            .allies
            .iter()
            .map(|k| catalog.champion_name(*k))
            .collect(),
        enemies: lobby
            .enemies
            .iter()
            .map(|k| catalog.champion_name(*k))
            .collect(),
        my_position: lobby.my_position.clone(),
    }
}

/// Read the current value immediately and schedule at most one bounded refresh in the background.
fn aggregate_for(
    st: &Arc<App>,
    champion_key: u32,
    requested: Option<Position>,
) -> Option<Arc<Aggregate>> {
    let (region, tier) = {
        let s = st.settings.lock().unwrap();
        (s.region.to_ascii_lowercase(), s.tier.to_ascii_lowercase())
    };
    let key = AggregateKey {
        champion_key,
        position: requested,
        region,
        tier,
    };
    let (cached, request) = {
        let mut state = st.aggregate.lock().unwrap();
        if !state.refresh.is_current(&key) {
            if let Some(task) = state.task.take() {
                task.abort();
            }
            state.value = None;
            state.error = None;
        }
        let request = state.refresh.begin(key, now_ms());
        (state.value.clone(), request)
    };
    if let Some(request) = request {
        let app_state = st.clone();
        let task = tokio::spawn(async move {
            let key = &request.key;
            let dir = crate::settings::data_dir().join("aggregate");
            let result = match tokio::time::timeout(
                Duration::from_secs(28),
                aggregate::load(&dir, &key.region, &key.tier, key.champion_key, key.position),
            )
            .await
            {
                Ok(result) => result.map_err(|e| e.to_string()),
                Err(_) => Err("Build source timed out".to_string()),
            };
            let now = now_ms();
            let mut state = app_state.aggregate.lock().unwrap();
            match result {
                Ok(value) => {
                    let fresh = value.provenance.is_fresh_at(now);
                    let expires = fresh.then(|| {
                        value
                            .provenance
                            .fetched_at_unix_ms
                            .unwrap_or(now)
                            .saturating_add(aggregate::CACHE_TTL.as_millis() as u64)
                    });
                    if !state.refresh.finish(
                        &request,
                        expires,
                        if fresh { 0 } else { now + AGGREGATE_RETRY_MS },
                    ) {
                        return;
                    }
                    log::info!(
                        "aggregate: champion {} as {} ({}, patch {})",
                        key.champion_key,
                        value.position.label(),
                        value.describe(),
                        value.patch
                    );
                    state.error = value.provenance.warning.clone();
                    state.value = Some(Arc::new(value));
                }
                Err(error) => {
                    if !state
                        .refresh
                        .finish(&request, None, now + AGGREGATE_RETRY_MS)
                    {
                        return;
                    }
                    log::warn!("aggregate: champion {}: {error}", key.champion_key);
                    // Keep a last good value for this exact key, with its original age.
                    state.error = Some(error);
                }
            }
        });
        st.aggregate.lock().unwrap().task = Some(task);
    }
    cached
}

fn cancel_aggregate(st: &App) {
    let mut state = st.aggregate.lock().unwrap();
    if let Some(task) = state.task.take() {
        task.abort();
    }
    state.refresh.clear();
    state.value = None;
    state.error = None;
}

fn aggregate_source(aggregate: Option<&Aggregate>, now: u64) -> Option<SourceStatus> {
    aggregate.map(|a| SourceStatus {
        observed_at_ms: a.provenance.fetched_at_unix_ms,
        age_ms: a.provenance.age_at(now).map(|age| age.as_millis() as u64),
        stale: !a.provenance.is_fresh_at(now),
        identity_known: true,
    })
}

async fn load_catalog(app: &AppHandle, st: &App) -> Arc<Catalog> {
    let cache = crate::settings::data_dir().join("ddragon");
    loop {
        match ddragon::load(&cache).await {
            Ok(c) => {
                let c = Arc::new(c);
                *st.catalog.lock().unwrap() = Some(c.clone());
                let version = c.version.clone();
                st.update(app, |p| p.ddragon = Some(version));
                return c;
            }
            Err(e) => {
                log::warn!("Data Dragon: {e}");
                st.update(app, |p| {
                    p.message = Some(format!("Data Dragon unavailable: {e}"))
                });
                tokio::time::sleep(Duration::from_secs(15)).await;
            }
        }
    }
}

async fn connect(app: &AppHandle, st: &App) -> Option<Lcu> {
    if let Some(l) = st.lcu.lock().unwrap().clone() {
        return Some(l);
    }
    match Lcu::connect() {
        Ok(lcu) => {
            match lcu.current_summoner().await {
                Ok(me) => {
                    let name = format!(
                        "{}#{}",
                        me.get("gameName").and_then(|v| v.as_str()).unwrap_or("?"),
                        me.get("tagLine").and_then(|v| v.as_str()).unwrap_or("")
                    );
                    *st.summoner_id.lock().unwrap() = me.get("summonerId").and_then(|v| v.as_u64());
                    log::info!("connected to client on port {} as {name}", lcu.port());
                    st.update(app, |p| {
                        p.summoner = Some(name);
                    });
                }
                Err(e) => {
                    log::warn!("client found but not answering yet: {e}");
                    return None;
                }
            }
            *st.lcu.lock().unwrap() = Some(lcu.clone());
            Some(lcu)
        }
        Err(_) => None,
    }
}

fn drop_client(app: &AppHandle, st: &App) {
    *st.lcu.lock().unwrap() = None;
    *st.summoner_id.lock().unwrap() = None;
    st.update(app, |p| {
        p.summoner = None;
    });
}

fn no_data_message(champion: &str, error: Option<&str>) -> String {
    match error {
        Some(e) => format!("No build data for {champion} yet ({e})"),
        None => format!("No build data for {champion} yet"),
    }
}

#[derive(Clone, Default)]
struct ClientObservation {
    phase: Option<String>,
    phase_since_ms: u64,
    observed_at_ms: u64,
    session: Option<Value>,
    session_at_ms: u64,
    swiftplay: Option<featherstorm_core::swiftplay::SwiftplayLobby>,
    swiftplay_mode: bool,
    queue_known: bool,
    lobby_at_ms: u64,
    lobby_error: Option<String>,
}

impl ClientObservation {
    fn wait_for_queue_assignment(&self) -> bool {
        !self.queue_known || self.swiftplay_mode
    }
    fn phase_at(&self, now: u64) -> Option<&str> {
        now.checked_sub(self.observed_at_ms)
            .filter(|age| *age <= CLIENT_MAX_AGE_MS)?;
        self.phase.as_deref()
    }

    fn session_at(&self, now: u64) -> Option<&Value> {
        now.checked_sub(self.session_at_ms)
            .filter(|age| *age <= CLIENT_MAX_AGE_MS)?;
        self.session.as_ref()
    }
}

/// LCU discovery and its HTTP requests cannot hold up the live-data clock.
fn watch_client(app: AppHandle, st: Arc<App>) -> watch::Receiver<ClientObservation> {
    let (sender, receiver) = watch::channel(ClientObservation::default());
    tokio::spawn(async move {
        let mut observation = ClientObservation::default();
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let phase = match connect(&app, &st).await {
                Some(lcu) => match lcu.gameflow_phase().await {
                    Ok(phase) => Some((lcu, phase)),
                    Err(error) => {
                        log::info!("client went away: {error}");
                        drop_client(&app, &st);
                        None
                    }
                },
                None => None,
            };
            let Some((lcu, phase)) = phase else {
                observation = ClientObservation::default();
                if sender.send(observation.clone()).is_err() {
                    break;
                }
                continue;
            };
            let now = now_ms();
            if observation.phase.as_deref() != Some(phase.as_str()) {
                observation.phase_since_ms = now;
                observation.session = None;
                observation.queue_known = false;
                observation.phase = Some(phase.clone());
                observation.observed_at_ms = now;
                if sender.send(observation.clone()).is_err() {
                    break;
                }
            }
            observation.observed_at_ms = now;
            observation.phase = Some(phase.clone());
            if matches!(phase.as_str(), "Lobby" | "Matchmaking" | "ReadyCheck") {
                // Timestamp the request start: a response to an older read cannot undo a
                // just-confirmed write or be mistaken for a later manual edit.
                let requested_at = now_ms();
                match lcu.lobby().await {
                    Ok(raw) => {
                        observation.lobby_at_ms = requested_at;
                        observation.queue_known = raw
                            .as_ref()
                            .and_then(|lobby| lobby.pointer("/gameConfig/queueId"))
                            .and_then(Value::as_u64)
                            .is_some();
                        observation.swiftplay_mode = raw
                            .as_ref()
                            .and_then(|lobby| lobby.pointer("/gameConfig/queueId"))
                            .and_then(Value::as_u64)
                            == Some(480);
                        let parsed = raw
                            .as_ref()
                            .map(featherstorm_core::swiftplay::SwiftplayLobby::from_lobby)
                            .transpose();
                        match parsed {
                            Ok(lobby) => {
                                observation.swiftplay = lobby.flatten();
                                observation.lobby_error = None;
                            }
                            Err(error) => {
                                observation.swiftplay = None;
                                observation.lobby_error = Some(error.to_string());
                            }
                        }
                    }
                    Err(error) => {
                        observation.swiftplay = None;
                        observation.lobby_error = Some(format!("Lobby data unavailable: {error}"));
                    }
                }
            }
            if phase == "ChampSelect" {
                // Also works when launched/reconnected during the instant assignment;
                // a prior Lobby observation is not required. Read only queue identity.
                let queue = lcu
                    .get("/lol-gameflow/v1/session")
                    .await
                    .ok()
                    .and_then(|s| s.pointer("/gameData/queue/id").and_then(Value::as_u64));
                observation.queue_known = queue.is_some();
                observation.swiftplay_mode = queue == Some(480);
                match lcu.champ_select_session().await {
                    Ok(session) => {
                        observation.session = session;
                        observation.session_at_ms = now_ms();
                    }
                    Err(error) => {
                        log::warn!("champ select: {error}");
                        observation.session = None;
                    }
                }
            } else {
                observation.session = None;
            }
            if sender.send(observation.clone()).is_err() {
                break;
            }
        }
    });
    receiver
}

#[derive(Clone, Default)]
struct LiveObservation {
    sequence: u64,
    received_at_ms: u64,
    data: Option<Value>,
}

fn watch_live() -> watch::Receiver<LiveObservation> {
    let (sender, receiver) = watch::channel(LiveObservation::default());
    tokio::spawn(async move {
        let client = LiveClient::new().expect("http client");
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut sequence: u64 = 0;
        loop {
            interval.tick().await;
            let data = client.all_game_data().await;
            sequence = sequence.wrapping_add(1);
            if sender
                .send(LiveObservation {
                    sequence,
                    received_at_ms: now_ms(),
                    data,
                })
                .is_err()
            {
                break;
            }
        }
    });
    receiver
}

enum ImportSignature {
    Runes(String),
    Spells((u32, u32)),
    ItemSet(String),
}

impl ImportSignature {
    fn name(&self) -> &'static str {
        match self {
            Self::Runes(_) => "runes",
            Self::Spells(_) => "spells",
            Self::ItemSet(_) => "item set",
        }
    }
}

struct ImportJob {
    generation: u64,
    champion: u32,
    signature: ImportSignature,
    task: JoinHandle<Result<String, String>>,
}

impl ImportJob {
    fn start(
        app: &AppHandle,
        st: &Arc<App>,
        generation: u64,
        champion: u32,
        signature: ImportSignature,
        plan: &Plan,
    ) -> Self {
        let app = app.clone();
        let st = st.clone();
        let plan = plan.clone();
        let which = signature.name();
        let task = tokio::spawn(async move {
            let panel = st.snapshot();
            if st.session.lock().unwrap().generation != generation
                || panel.phase != "champselect"
                || panel.champion.as_deref() != Some(plan.champion.as_str())
            {
                return Err("Champion select changed before import".to_string());
            }
            match which {
                "runes" => crate::commands::do_import_runes_for_plan(&app, &st, plan).await,
                "spells" => crate::commands::do_import_spells_for_plan(&app, &st, plan).await,
                _ => crate::commands::do_import_item_set_for_plan(&app, &st, plan).await,
            }
        });
        Self {
            generation,
            champion,
            signature,
            task,
        }
    }
}

#[derive(Default)]
struct ImportJobs {
    runes: Option<ImportJob>,
    spells: Option<ImportJob>,
    itemset: Option<ImportJob>,
}

impl ImportJobs {
    fn cancel(&mut self) {
        for slot in [&mut self.runes, &mut self.spells, &mut self.itemset] {
            if let Some(job) = slot.take() {
                job.task.abort();
            }
        }
    }

    async fn collect(&mut self, imports: &mut ImportTracker, generation: u64, champion: u32) {
        for slot in [&mut self.runes, &mut self.spells, &mut self.itemset] {
            if slot
                .as_ref()
                .is_some_and(|job| job.generation != generation || job.champion != champion)
            {
                if let Some(job) = slot.take() {
                    job.task.abort();
                }
                continue;
            }
            if !slot.as_ref().is_some_and(|job| job.task.is_finished()) {
                continue;
            }
            let job = slot.take().unwrap();
            let which = job.signature.name();
            let result = job
                .task
                .await
                .unwrap_or_else(|e| Err(format!("Import task stopped: {e}")));
            let succeeded = result.is_ok();
            match result {
                Ok(message) => log::info!("auto-import {which}: {message}"),
                Err(error) => log::warn!("auto-import {which}: {error}"),
            }
            match job.signature {
                ImportSignature::Runes(signature) => {
                    imports.record_runes(champion, signature, succeeded)
                }
                ImportSignature::Spells(spells) => {
                    imports.record_spells(champion, spells, now_ms(), succeeded)
                }
                ImportSignature::ItemSet(signature) => {
                    imports.record_itemset(champion, signature, succeeded)
                }
            }
        }
    }
}

fn reset_match(
    app: &AppHandle,
    st: &App,
    imports: &mut ImportTracker,
    jobs: &mut ImportJobs,
    freshness: &mut LiveFreshness,
) {
    let _planning = st.planning.lock().unwrap();
    controller::reset_session_locked(app, st);
    imports.reset();
    jobs.cancel();
    *freshness = LiveFreshness::default();
    cancel_aggregate(st);
    *st.lobby.lock().unwrap() = None;
    *st.plan.lock().unwrap() = None;
    st.update(app, |p| {
        p.champion = None;
        p.supported = false;
        p.lobby = None;
        p.plan = None;
        p.live = None;
        p.live_source = None;
        p.aggregate_source = None;
        p.flash = None;
        p.imports = Default::default();
    });
}

fn publish_stale_live(
    app: &AppHandle,
    st: &App,
    freshness: &LiveFreshness,
    phase: Option<&str>,
    identity_failed: bool,
) {
    let _planning = st.planning.lock().unwrap();
    *st.latest_live.lock().unwrap() = None;
    let source = freshness.status(now_ms());
    st.update(app, |p| {
        p.live_source = Some(source.clone());
        p.flash = None;
        if source.observed_at_ms.is_none() {
            p.phase = if phase.is_some() {
                "loading"
            } else {
                "noclient"
            }
            .into();
            p.live = None;
        }
        p.message = Some(if identity_failed {
            "Game data is available, but your player identity is not verified.".to_string()
        } else if let Some(age) = source.age_ms {
            format!(
                "Game data is {}s old. Waiting for a fresh observation...",
                age / 1000
            )
        } else if phase.is_some() {
            "Waiting for playable game data...".to_string()
        } else {
            "Waiting for the League client or a running game...".to_string()
        });
    });
}

fn rune_signature(plan: &Plan) -> Option<String> {
    plan.runes.as_ref().map(|page| {
        format!(
            "{}:{}:{}",
            page.primary_style,
            page.sub_style,
            page.perks
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",")
        )
    })
}

fn itemset_signature(plan: &Plan) -> String {
    let ids = |items: &[featherstorm_core::engine::PlanItem]| {
        items
            .iter()
            .map(|item| item.id.to_string())
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        "{}|{}|{}",
        ids(&plan.start),
        ids(&plan.path),
        ids(&plan.options)
    )
}

fn session_id(session: &Value) -> Option<String> {
    match session.get("gameId") {
        Some(Value::Number(id)) if id.as_u64().is_some_and(|id| id > 0) => Some(id.to_string()),
        Some(Value::String(id)) if !id.is_empty() && id != "0" => Some(id.clone()),
        _ => None,
    }
}

pub async fn run(app: AppHandle, st: Arc<App>) {
    let catalog = load_catalog(&app, &st).await;
    let client_observations = watch_client(app.clone(), st.clone());
    let live_observations = watch_live();
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut session_tracker = SessionTracker::default();
    let mut imports = ImportTracker::default();
    let mut import_jobs = ImportJobs::default();
    let mut freshness = LiveFreshness::default();
    let mut last_level: Option<u32> = None;
    let mut last_phase: Option<String> = None;
    let mut last_summary = String::new();
    let mut last_live_sequence = 0;
    let mut identity_failed = false;
    let mut swiftplay = crate::swiftplay::Runtime::default();

    loop {
        interval.tick().await;
        let client = client_observations.borrow().clone();
        let live_observation = live_observations.borrow().clone();
        let now = now_ms();
        let phase_owned = client.phase_at(now).map(str::to_string);
        let phase = phase_owned.as_deref();
        let select = (phase == Some("ChampSelect"))
            .then(|| client.session_at(now))
            .flatten();
        let select_id = select.and_then(session_id);
        let phase_changed = phase_owned != last_phase;
        if phase_changed {
            log::info!(
                "gameflow {} -> {}",
                last_phase.as_deref().unwrap_or("unavailable"),
                phase.unwrap_or("unavailable")
            );
            last_phase = phase_owned.clone();
        }
        if session_tracker.observe_phase(phase, select_id.as_deref()) {
            reset_match(&app, &st, &mut imports, &mut import_jobs, &mut freshness);
            last_level = None;
            last_summary.clear();
            identity_failed = false;
        }
        if phase_changed && phase == Some("GameStart") {
            let _planning = st.planning.lock().unwrap();
            freshness = LiveFreshness::default();
            last_level = None;
            *st.latest_live.lock().unwrap() = None;
            *st.lobby_observed_at_ms.lock().unwrap() = None;
            st.update(&app, |p| {
                p.phase = "loading".into();
                p.live = None;
                p.live_source = Some(SourceStatus::default());
                p.flash = None;
                p.message = Some("Loading...".into());
            });
        }
        st.update(&app, |p| {
            if p.flash.as_ref().is_some_and(|flash| flash.until_ms < now) {
                p.flash = None;
            }
            p.gameflow = phase.unwrap_or("").to_string();
            if p.live_source.is_some() {
                p.live_source = Some(freshness.status(now));
            }
        });

        let prequeue = matches!(phase, Some("Lobby" | "Matchmaking" | "ReadyCheck"));
        if prequeue && client.swiftplay_mode {
            import_jobs.cancel();
            let fresh = now
                .checked_sub(client.lobby_at_ms)
                .is_some_and(|age| age <= 6_000);
            let view = if let Some(lobby) = client.swiftplay.as_ref().filter(|_| fresh) {
                let lcu = st.lcu.lock().unwrap().clone();
                swiftplay
                    .observe(
                        lobby,
                        client.lobby_at_ms,
                        phase.unwrap_or(""),
                        &st,
                        &catalog,
                        lcu,
                    )
                    .await
            } else {
                swiftplay.stop();
                featherstorm_core::state::SwiftplayView {
                    // A fresh but incomplete choice needs an actionable explanation,
                    // not the stale-connection message. It still cannot claim readiness.
                    observed_at_ms: fresh.then_some(client.lobby_at_ms),
                    message: Some(
                        client
                            .lobby_error
                            .clone()
                            .unwrap_or_else(|| "Waiting for fresh Swiftplay choices".into()),
                    ),
                    ..Default::default()
                }
            };
            let _planning = st.planning.lock().unwrap();
            controller::finish_journal_locked(&app, &st);
            *st.plan.lock().unwrap() = None;
            *st.lobby.lock().unwrap() = None;
            *st.latest_live.lock().unwrap() = None;
            *st.lobby_observed_at_ms.lock().unwrap() = None;
            st.update(&app, |p| {
                p.phase = "swiftplay".into();
                p.swiftplay = Some(view);
                p.champion = None;
                p.supported = false;
                p.plan = None;
                p.lobby = None;
                p.live = None;
                p.live_source = None;
                p.aggregate_source = None;
                p.flash = None;
                p.imports = Default::default();
                p.message = None;
            });
            continue;
        }
        swiftplay.stop();
        if prequeue && client.lobby_error.is_none() && !client.swiftplay_mode {
            swiftplay.clear();
        }
        st.update(&app, |p| p.swiftplay = None);
        // Swiftplay's instant assignment is not draft. Do not overwrite its saved
        // per-choice loadouts through ordinary champion-select APIs.
        if phase == Some("ChampSelect") && client.wait_for_queue_assignment() {
            import_jobs.cancel();
            st.update(&app, |p| {
                p.phase = "loading".into();
                p.plan = None;
                p.champion = None;
                p.supported = false;
                p.message = Some(
                    if client.swiftplay_mode {
                        "Waiting for your assigned Swiftplay champion..."
                    } else {
                        "Checking queue type before preparing loadouts..."
                    }
                    .into(),
                );
            });
            continue;
        }

        if phase == Some("ChampSelect") {
            last_live_sequence = live_observation.sequence;
            let Some(raw_session) = select else {
                import_jobs.cancel();
                let _planning = st.planning.lock().unwrap();
                *st.lobby_observed_at_ms.lock().unwrap() = None;
                *st.latest_live.lock().unwrap() = None;
                *st.plan.lock().unwrap() = None;
                st.update(&app, |p| {
                    p.phase = "champselect".into();
                    p.live = None;
                    p.live_source = None;
                    p.plan = None;
                    p.supported = false;
                    p.message = Some("Waiting for champion select data...".into());
                });
                continue;
            };
            let mut lobby = champselect::extract(raw_session);
            let own_rows = raw_session
                .get("myTeam")
                .and_then(Value::as_array)
                .map(|team| {
                    team.iter()
                        .filter(|player| {
                            player.get("cellId").and_then(Value::as_i64) == Some(lobby.my_cell)
                        })
                        .count()
                })
                .unwrap_or(0);
            let own_identity = lobby.my_cell >= 0 && own_rows == 1;
            if !own_identity {
                lobby.my_champion = 0;
                lobby.my_locked = false;
            }
            let my_champion = lobby.my_champion;
            let generation = st.session.lock().unwrap().generation;
            import_jobs
                .collect(&mut imports, generation, my_champion)
                .await;
            let _planning = st.planning.lock().unwrap();
            let champion = (my_champion > 0).then(|| catalog.champion_name(my_champion));
            let champion_changed =
                controller::ensure_champion_locked(&app, &st, champion.as_deref());
            if champion_changed {
                imports.reset();
                import_jobs.cancel();
            }
            // An applied selection can appear before the request's response. Observe it after
            // that job completes so the app's own write is not mistaken for a manual edit.
            if import_jobs.spells.is_none() {
                imports.observe_spells(lobby.my_spells, now);
            }
            let enemies: Vec<String> = lobby
                .enemies
                .iter()
                .map(|key| catalog.champion_name(*key))
                .collect();
            let requested = Position::parse(&lobby.my_position);
            let agg = (my_champion > 0)
                .then(|| aggregate_for(&st, my_champion, requested))
                .flatten();
            let pack = champion.as_deref().and_then(|name| st.pack_for(name));
            let plan = compute(
                &st,
                &catalog,
                champion.as_deref().unwrap_or(""),
                pack,
                agg.as_deref(),
                &enemies,
                None,
            );
            let supported = !plan.path.is_empty();
            let agg_error = st.aggregate.lock().unwrap().error.clone();
            let position_label = plan
                .position
                .clone()
                .unwrap_or_else(|| lobby.my_position.clone());
            let summary = format!(
                "champ select: {} ({position_label}) vs {:?}; {}",
                champion.as_deref().unwrap_or("(no pick yet)"),
                enemies,
                plan_summary(&plan)
            );
            if summary != last_summary {
                log::info!("{summary}");
                last_summary = summary;
            }
            let view = lobby_view(&lobby, &catalog);
            let source = aggregate_source(agg.as_deref(), now);
            *st.lobby.lock().unwrap() = Some(lobby.clone());
            *st.lobby_observed_at_ms.lock().unwrap() = own_identity.then_some(client.session_at_ms);
            *st.latest_live.lock().unwrap() = None;
            controller::store_plan_locked(&st, &catalog, &plan, None);
            st.update(&app, |p| {
                p.phase = "champselect".into();
                p.champion = champion.clone();
                p.supported = supported;
                p.lobby = Some(view);
                p.plan = Some(plan.clone());
                p.live = None;
                p.live_source = None;
                p.aggregate_source = source;
                if champion_changed {
                    p.imports = Default::default();
                }
                p.message = match &champion {
                    None => Some("Pick a champion".into()),
                    Some(name) if !supported => plan
                        .note
                        .clone()
                        .or_else(|| Some(no_data_message(name, agg_error.as_deref()))),
                    _ => None,
                };
            });
            if supported && my_champion > 0 {
                let (auto_runes, auto_spells, auto_itemset) = {
                    let settings = st.settings.lock().unwrap();
                    (
                        settings.auto_runes,
                        settings.auto_spells,
                        settings.auto_itemset,
                    )
                };
                if auto_runes && import_jobs.runes.is_none() {
                    if let Some(signature) =
                        rune_signature(&plan).filter(|sig| imports.runes_needed(my_champion, sig))
                    {
                        import_jobs.runes = Some(ImportJob::start(
                            &app,
                            &st,
                            st.session.lock().unwrap().generation,
                            my_champion,
                            ImportSignature::Runes(signature),
                            &plan,
                        ));
                    }
                }
                if auto_spells && import_jobs.spells.is_none() {
                    let current =
                        Some(lobby.my_spells).filter(|spells| spells.0 > 0 && spells.1 > 0);
                    if let Some(spells) =
                        runes::order_spells(&plan.spell_ids, current).filter(|spells| {
                            imports.spells_needed(my_champion, *spells, lobby.my_locked)
                        })
                    {
                        import_jobs.spells = Some(ImportJob::start(
                            &app,
                            &st,
                            st.session.lock().unwrap().generation,
                            my_champion,
                            ImportSignature::Spells(spells),
                            &plan,
                        ));
                    }
                }
                if auto_itemset && lobby.my_locked && import_jobs.itemset.is_none() {
                    let signature = itemset_signature(&plan);
                    if imports.itemset_needed(my_champion, &signature) {
                        import_jobs.itemset = Some(ImportJob::start(
                            &app,
                            &st,
                            st.session.lock().unwrap().generation,
                            my_champion,
                            ImportSignature::ItemSet(signature),
                            &plan,
                        ));
                    }
                }
            } else {
                import_jobs.cancel();
            }
            continue;
        }

        // Imports belong to champion select only. A slow rune request cannot delay game detection.
        import_jobs.cancel();
        if session::should_poll_live(phase) {
            if live_observation.sequence != last_live_sequence
                && (phase != Some("GameStart")
                    || live_observation.received_at_ms >= client.phase_since_ms)
            {
                last_live_sequence = live_observation.sequence;
                match live_observation.data.as_ref() {
                    Some(data) => {
                        let snap = live::summarize(data);
                        let game_time = data["gameData"]["gameTime"]
                            .as_f64()
                            .filter(|time| time.is_finite() && *time > 0.0);
                        let identity_known = snap.me.as_ref().is_some_and(|me| {
                            !me.player.champion.is_empty() && me.player.level > 0
                        });
                        identity_failed = !identity_known;
                        if let Some(time) = game_time.filter(|_| identity_known) {
                            if session_tracker.observe_live(time) {
                                reset_match(
                                    &app,
                                    &st,
                                    &mut imports,
                                    &mut import_jobs,
                                    &mut freshness,
                                );
                                last_level = None;
                                last_summary.clear();
                            }
                        }
                        freshness.observe(
                            game_time.unwrap_or(f64::NAN),
                            identity_known,
                            live_observation.received_at_ms,
                        );
                        if !freshness.status(now).stale {
                            let _planning = st.planning.lock().unwrap();
                            let me = snap.me.as_ref().expect("identifiable live player");
                            let champion = me.player.champion.clone();
                            if controller::ensure_champion_locked(&app, &st, Some(&champion)) {
                                imports.reset();
                                last_level = None;
                            }
                            let enemies: Vec<String> = if snap.enemies.is_empty() {
                                st.lobby
                                    .lock()
                                    .unwrap()
                                    .as_ref()
                                    .map(|lobby| {
                                        lobby
                                            .enemies
                                            .iter()
                                            .map(|key| catalog.champion_name(*key))
                                            .collect()
                                    })
                                    .unwrap_or_default()
                            } else {
                                snap.enemies
                                    .iter()
                                    .map(|player| player.champion.clone())
                                    .collect()
                            };
                            // In a mid-game launch there may be no champion select observation.
                            let requested = Position::parse(&me.player.position)
                                .or_else(|| {
                                    st.lobby
                                        .lock()
                                        .unwrap()
                                        .as_ref()
                                        .and_then(|lobby| Position::parse(&lobby.my_position))
                                })
                                .or_else(|| {
                                    catalog
                                        .champion_key(&champion)
                                        .and_then(|key| swiftplay.role_for(key))
                                });
                            let agg = catalog
                                .champion_key(&champion)
                                .and_then(|key| aggregate_for(&st, key, requested));
                            let pack = st.pack_for(&champion);
                            let plan = compute(
                                &st,
                                &catalog,
                                &champion,
                                pack,
                                agg.as_deref(),
                                &enemies,
                                Some(&snap),
                            );
                            let supported = !plan.path.is_empty();
                            let agg_error = st.aggregate.lock().unwrap().error.clone();
                            let level = me.player.level;
                            let summary = format!(
                                "live: {champion} lvl {level} vs {:?}; {}",
                                enemies,
                                plan_summary(&plan)
                            );
                            if summary != last_summary {
                                log::info!("{summary}");
                                last_summary = summary;
                            }
                            let flash =
                                last_level
                                    .filter(|previous| level > *previous)
                                    .and_then(|_| {
                                        plan.skill.next.map(|skill| Flash {
                                            skill,
                                            until_ms: now + 3500,
                                        })
                                    });
                            last_level = Some(level);
                            let live_view = LiveView {
                                game_time: snap.game_time,
                                gold: me.gold,
                                level,
                                kda: me.player.kda(),
                            };
                            let allies = snap
                                .allies
                                .iter()
                                .map(|player| player.champion.clone())
                                .collect();
                            let position = if me.player.position.is_empty() {
                                requested
                                    .map(|position| position.label().to_string())
                                    .unwrap_or_default()
                            } else {
                                me.player.position.clone()
                            };
                            let source = aggregate_source(agg.as_deref(), now);
                            *st.latest_live.lock().unwrap() = Some(snap.clone());
                            *st.lobby_observed_at_ms.lock().unwrap() = None;
                            controller::store_plan_locked(&st, &catalog, &plan, Some(&snap));
                            st.update(&app, |p| {
                                p.phase = "ingame".into();
                                p.champion = Some(champion.clone());
                                p.supported = supported;
                                p.plan = Some(plan.clone());
                                p.live = Some(live_view);
                                p.live_source = Some(freshness.status(now));
                                p.aggregate_source = source;
                                if let Some(flash) = flash {
                                    p.flash = Some(flash);
                                }
                                p.lobby = Some(LobbyView {
                                    allies,
                                    enemies: enemies.clone(),
                                    my_position: position,
                                });
                                p.message = if supported {
                                    None
                                } else {
                                    plan.note.clone().or_else(|| {
                                        Some(no_data_message(&champion, agg_error.as_deref()))
                                    })
                                };
                            });
                        }
                    }
                    None => {
                        freshness.miss();
                        identity_failed = false;
                    }
                }
            }
            if freshness.status(now).stale {
                publish_stale_live(&app, &st, &freshness, phase, identity_failed);
            }
            continue;
        }

        last_live_sequence = live_observation.sequence;
        let _planning = st.planning.lock().unwrap();
        last_level = None;
        freshness = LiveFreshness::default();
        identity_failed = false;
        *st.latest_live.lock().unwrap() = None;
        *st.lobby_observed_at_ms.lock().unwrap() = None;
        if session::confirmed_game_end(phase) {
            controller::finish_journal_locked(&app, &st);
        }
        let ordinary_idle = matches!(phase, Some("None" | "Lobby" | "Matchmaking" | "ReadyCheck"));
        if phase_changed {
            imports.reset();
            cancel_aggregate(&st);
        }
        if ordinary_idle {
            *st.lobby.lock().unwrap() = None;
            *st.plan.lock().unwrap() = None;
        }
        st.update(&app, |p| {
            p.phase = "idle".into();
            p.live = None;
            p.live_source = None;
            p.aggregate_source = None;
            p.flash = None;
            if ordinary_idle {
                p.lobby = None;
                p.plan = None;
                p.champion = None;
                p.supported = false;
                p.imports = Default::default();
            }
            p.message = Some(match phase {
                Some("Lobby") => "In lobby. Runes and spells are set when you pick.".to_string(),
                Some("Matchmaking") => "In queue...".to_string(),
                Some("ReadyCheck") => "Match found!".to_string(),
                Some("EndOfGame" | "PreEndOfGame" | "WaitingForStats") => {
                    "Game over. GG.".to_string()
                }
                _ => "Ready. Start a game.".to_string(),
            });
        });
    }
}

#[cfg(test)]
mod swiftplay_tests {
    use super::ClientObservation;

    #[test]
    fn restart_in_assignment_cannot_enter_draft_imports_with_unknown_queue() {
        let mut observation = ClientObservation::default();
        assert!(observation.wait_for_queue_assignment());
        observation.queue_known = true;
        observation.swiftplay_mode = true;
        assert!(observation.wait_for_queue_assignment());
        observation.swiftplay_mode = false;
        assert!(!observation.wait_for_queue_assignment());
        observation.queue_known = false;
        assert!(observation.wait_for_queue_assignment());
    }
}
