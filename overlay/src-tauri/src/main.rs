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
use featherstorm_core::state::PanelState;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

pub const PANEL_W: f64 = 380.0;
pub const PANEL_H: f64 = 300.0;
pub const PANEL_H_COLLAPSED: f64 = 64.0;

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
            collapsed: saved.collapsed,
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
            let saved = state.settings.lock().unwrap().clone();
            let height = if saved.collapsed { PANEL_H_COLLAPSED } else { PANEL_H };
            let _ = window.set_size(tauri::LogicalSize::new(PANEL_W, height));
            match (saved.x, saved.y) {
                (Some(x), Some(y)) => {
                    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
                }
                _ => {
                    // Default: bottom-right, just left of where the minimap sits.
                    if let Ok(Some(monitor)) = window.primary_monitor() {
                        let scale = monitor.scale_factor();
                        let size = monitor.size();
                        let w = (PANEL_W * scale) as i32;
                        let h = (PANEL_H * scale) as i32;
                        let minimap = (size.height as f64 * 0.30) as i32;
                        let margin = (12.0 * scale) as i32;
                        let x = (size.width as i32 - minimap - w - margin).max(0);
                        let y = (size.height as i32 - h - margin).max(0);
                        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
                    }
                }
            }
            let st = state.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::Moved(pos) = event {
                    let mut s = st.settings.lock().unwrap();
                    s.x = Some(pos.x);
                    s.y = Some(pos.y);
                    let _ = settings::save(&s);
                }
            });
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
