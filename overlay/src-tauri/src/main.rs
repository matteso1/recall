//! Featherstorm overlay: a small always-on-top panel driven by the core "brain".
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod poller;
mod probe;
mod settings;

use featherstorm_core::champselect::Lobby;
use featherstorm_core::ddragon::Catalog;
use featherstorm_core::engine::Plan;
use featherstorm_core::lcu::Lcu;
use featherstorm_core::pack::{ChampionPack, Traits};
use featherstorm_core::placement::{self, Screen};
use featherstorm_core::state::PanelState;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

pub use featherstorm_core::placement::{PANEL_H, PANEL_H_COLLAPSED, PANEL_W};

/// Everything the poller and the commands share. Locks are held only for quick copies.
pub struct App {
    pub panel: Mutex<PanelState>,
    pub catalog: Mutex<Option<Arc<Catalog>>>,
    pub pack: ChampionPack,
    pub traits: Traits,
    pub lcu: Mutex<Option<Lcu>>,
    pub lobby: Mutex<Option<Lobby>>,
    pub summoner_id: Mutex<Option<u64>>,
    pub plan: Mutex<Option<Plan>>,
    pub settings: Mutex<settings::Settings>,
}

impl App {
    pub fn snapshot(&self) -> PanelState {
        self.panel.lock().unwrap().clone()
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
    let path = settings::data_dir().join("featherstorm.log");
    let mut builder = env_logger::Builder::new();
    builder.filter_level(log::LevelFilter::Info);
    if let Ok(env) = std::env::var("FEATHERSTORM_LOG") {
        builder.parse_filters(&env);
    }
    match std::fs::OpenOptions::new().create(true).append(true).open(&path) {
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
    Screen::new(pos.x, pos.y, size.width as i32, size.height as i32, m.scale_factor())
        .with_work_area(work.position.x, work.position.y, work.size.width as i32, work.size.height as i32)
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
    let primary = window.primary_monitor().ok().flatten().map(|m| screen_of(&m));
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
            let source = if saved == Some((x, y)) { "saved position" } else { "default placement" };
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
    log::info!("featherstorm {} starting", env!("CARGO_PKG_VERSION"));
    if std::env::args().any(|a| a == "--probe") {
        std::process::exit(probe::run());
    }

    let pack = featherstorm_core::pack::load_xayah().expect("data pack");
    let traits = featherstorm_core::pack::load_traits().expect("champion traits");
    let saved = settings::load();
    let state = Arc::new(App {
        panel: Mutex::new(PanelState {
            phase: "noclient".into(),
            message: Some("Waiting for the League client...".into()),
            version: env!("CARGO_PKG_VERSION").into(),
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
    });

    tauri::Builder::default()
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::import_item_set,
            commands::import_runes,
            commands::import_spells,
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
            let handle = app.handle().clone();
            let st = state.clone();
            tauri::async_runtime::spawn(async move {
                poller::run(handle, st).await;
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Featherstorm");
}
