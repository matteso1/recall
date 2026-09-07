//! Recall overlay: a small always-on-top panel driven by the core "brain".
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod commands;
mod controller;
mod demo;
mod journal_store;
mod poller;
mod probe;
mod rune_queue;
mod settings;
mod swiftplay;

use recall_core::aggregate::Aggregate;
use recall_core::champselect::Lobby;
use recall_core::ddragon::{normalize, Catalog};
use recall_core::engine::{Plan, PlannerPreferences};
use recall_core::journal::Journal;
use recall_core::lcu::Lcu;
use recall_core::live::LiveSnapshot;
use recall_core::pack::{ChampionPack, Traits};
use recall_core::placement::{self, Screen};
use recall_core::state::PanelState;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

pub use recall_core::placement::{PANEL_H, PANEL_H_COLLAPSED, PANEL_W};

/// The aggregate (op.gg) build for the champion currently in play, and when to retry a failed fetch.
#[derive(Default)]
pub struct AggState {
    pub refresh: recall_core::session::RefreshGate<recall_core::session::AggregateKey>,
    pub value: Option<Arc<Aggregate>>,
    pub error: Option<String>,
    pub task: Option<tokio::task::JoinHandle<()>>,
}

/// Everything the poller and the commands share. Locks are held only for quick copies.
pub struct App {
    /// Serializes only local planning/commits. Never held over network or disk I/O.
    pub planning: Mutex<()>,
    pub panel: Mutex<PanelState>,
    pub catalog: Mutex<Option<Arc<Catalog>>>,
    pub pack: ChampionPack,
    pub traits: Traits,
    pub lcu: Mutex<Option<Lcu>>,
    pub lobby: Mutex<Option<Lobby>>,
    pub summoner_id: Mutex<Option<u64>>,
    pub plan: Mutex<Option<Plan>>,
    pub settings: Mutex<settings::Settings>,
    pub aggregate: Mutex<AggState>,
    pub preferences: Mutex<PlannerPreferences>,
    pub latest_live: Mutex<Option<LiveSnapshot>>,
    pub lobby_observed_at_ms: Mutex<Option<u64>>,
    pub session: Mutex<controller::RecommendationSession>,
    pub journal: Mutex<Journal>,
    pub journal_sink: journal_store::JournalSink,
    /// `--autostart`: the panel follows the League client (see autostart.rs).
    pub autostart: bool,
    /// In autostart mode the x button hides the panel until the client restarts.
    pub dismissed: Mutex<bool>,
}

impl App {
    pub fn snapshot(&self) -> PanelState {
        self.panel.lock().unwrap().clone()
    }

    /// Optional factual champion-specific coaching, never the source of a fallback build.
    pub fn pack_for(&self, champion: &str) -> Option<&ChampionPack> {
        (normalize(champion) == normalize(&self.pack.champion)).then_some(&self.pack)
    }

    /// Mutate the panel state and push it to the webview if anything changed.
    pub fn update(&self, app: &tauri::AppHandle, f: impl FnOnce(&mut PanelState)) {
        let changed = {
            let mut panel = self.panel.lock().unwrap();
            let before = panel.clone();
            f(&mut panel);
            *panel != before
        };
        if changed {
            let state = self.snapshot();
            if let Err(e) = app.emit("state", &state) {
                log::warn!("emit failed: {e}");
            }
        }
    }
}

fn init_logging() {
    let path = settings::data_dir().join("recall.log");
    let mut builder = env_logger::Builder::new();
    builder.filter_level(log::LevelFilter::Info);
    if let Ok(env) = std::env::var("RECALL_LOG") {
        builder.parse_filters(&env);
    }
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(file) => {
            builder.target(env_logger::Target::Pipe(Box::new(file)));
        }
        Err(_) => {
            builder.target(env_logger::Target::Stderr);
        }
    }
    let _ = builder.try_init();
}

fn screen_of(m: &tauri::Monitor) -> Screen {
    let (pos, size, work) = (m.position(), m.size(), m.work_area());
    Screen::new(
        pos.x,
        pos.y,
        size.width as i32,
        size.height as i32,
        m.scale_factor(),
    )
    .with_work_area(
        work.position.x,
        work.position.y,
        work.size.width as i32,
        work.size.height as i32,
    )
}

/// Put the panel at its saved position when that is still on a screen, otherwise bottom-right of
/// the primary monitor. A stale settings.json (other monitor, other resolution, hand edits) must
/// never leave the panel off-screen.
fn place_window(window: &tauri::WebviewWindow, state: &App) {
    // Copy the saved position out: `set_position` fires `Moved`, whose handler takes this lock.
    let saved = {
        let s = state.settings.lock().unwrap();
        s.x.zip(s.y)
    };
    let screens: Vec<Screen> = window
        .available_monitors()
        .map(|ms| ms.iter().map(screen_of).collect())
        .unwrap_or_default();
    let primary = window
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| screen_of(&m));
    for s in &screens {
        log::info!(
            "monitor {}x{} at ({},{}) scale {}, work area {}x{} at ({},{})",
            s.bounds.w,
            s.bounds.h,
            s.bounds.x,
            s.bounds.y,
            s.scale,
            s.work.w,
            s.work.h,
            s.work.x,
            s.work.y
        );
    }
    match placement::startup_position(saved, primary.as_ref(), &screens) {
        Some((x, y)) => {
            let source = if saved == Some((x, y)) {
                "saved position"
            } else {
                "default placement"
            };
            log::info!("panel at ({x},{y}) [{source}], saved was {saved:?}");
            match window.set_position(tauri::PhysicalPosition::new(x, y)) {
                Ok(()) => {
                    // A programmatic move does not always raise `Moved`; record the placement ourselves.
                    let mut s = state.settings.lock().unwrap();
                    s.x = Some(x);
                    s.y = Some(y);
                    if let Err(e) = settings::save(&s) {
                        log::warn!("could not save settings: {e}");
                    }
                }
                Err(e) => log::warn!("set_position failed: {e}"),
            }
        }
        None => log::warn!("no monitor information; leaving the window where the OS put it"),
    }
}

fn main() {
    init_logging();
    log::info!("recall {} starting", env!("CARGO_PKG_VERSION"));
    if std::env::args().any(|a| a == "--probe") {
        std::process::exit(probe::run());
    }
    // `--demo [champselect|ingame|ingame-flash|idle]`: staged panel, no client (design work, screenshots).
    let autostart = std::env::args().any(|a| a == "--autostart");
    let demo: Option<String> = {
        let args: Vec<String> = std::env::args().collect();
        args.iter().position(|a| a == "--demo").map(|i| {
            args.get(i + 1)
                .cloned()
                .unwrap_or_else(|| "ingame".to_string())
        })
    };

    let pack = recall_core::pack::load_xayah().expect("data pack");
    let traits = recall_core::pack::load_traits().expect("champion traits");
    let saved = settings::load();
    let (journal, journal_sink, journal_writer) = if demo.is_some() {
        journal_store::disabled()
    } else {
        journal_store::open(settings::data_dir().join("decisions.json"))
    };
    let recap = journal.recap();
    let journal_error = journal_sink.warning.clone();
    let state = Arc::new(App {
        planning: Mutex::new(()),
        panel: Mutex::new(PanelState {
            phase: "noclient".into(),
            message: Some("Waiting for the League client...".into()),
            version: env!("CARGO_PKG_VERSION").into(),
            recap,
            journal_error,
            ..Default::default()
        }),
        catalog: Mutex::new(None),
        pack,
        traits,
        lcu: Mutex::new(None),
        lobby: Mutex::new(None),
        summoner_id: Mutex::new(None),
        plan: Mutex::new(None),
        settings: Mutex::new(saved),
        aggregate: Mutex::new(AggState::default()),
        preferences: Mutex::new(PlannerPreferences::default()),
        latest_live: Mutex::new(None),
        lobby_observed_at_ms: Mutex::new(None),
        session: Mutex::new(controller::RecommendationSession::default()),
        journal: Mutex::new(journal),
        journal_sink,
        autostart: autostart && demo.is_none(),
        dismissed: Mutex::new(false),
    });

    tauri::Builder::default()
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::import_item_set,
            commands::import_runes,
            commands::import_spells,
            commands::set_build_preference,
            commands::pin_item,
            commands::clear_item_pin,
            commands::rate_decision,
            commands::set_collapsed,
            commands::quit,
        ])
        .setup(move |app| {
            let window = app.get_webview_window("main").expect("main window");
            // Always start expanded; collapsing is a per-session choice (see settings.rs).
            let _ = window.set_size(tauri::LogicalSize::new(PANEL_W, PANEL_H));
            let st = state.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::Moved(pos) = event {
                    let mut s = st.settings.lock().unwrap();
                    s.x = Some(pos.x);
                    s.y = Some(pos.y);
                    let _ = settings::save(&s);
                }
            });
            place_window(&window, &state);
            // The window is created hidden (tauri.conf.json) so autostart never flashes at logon.
            if state.autostart {
                let handle = app.handle().clone();
                let st = state.clone();
                tauri::async_runtime::spawn(async move {
                    autostart::run(handle, st).await;
                });
            } else if let Err(e) = window.show() {
                log::warn!("show failed: {e}");
            }
            let handle = app.handle().clone();
            let st = state.clone();
            let writer_app = handle.clone();
            let writer_state = st.clone();
            tauri::async_runtime::spawn(journal_writer.run(move |error| {
                writer_state.update(&writer_app, |panel| panel.journal_error = error);
            }));
            match demo.clone() {
                Some(phase) => {
                    tauri::async_runtime::spawn(async move {
                        demo::run(handle, st, phase).await;
                    });
                }
                None => {
                    tauri::async_runtime::spawn(async move {
                        poller::run(handle, st).await;
                    });
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Recall");
}
