//! `recall.exe --autostart`: the mode the Startup-folder shortcut uses. The panel stays hidden
//! until the League client is running, shows while it is, and hides again once the client has
//! been gone for a grace period (patch restarts bring it back within seconds). The x button hides
//! the panel for the rest of that client session instead of quitting. No watcher process, no
//! console window: the overlay is its own launcher.
use crate::App;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const POLL: Duration = Duration::from_secs(2);
const CLOSE_AFTER: Duration = Duration::from_secs(20);

pub async fn run(app: AppHandle, st: Arc<App>) {
    log::info!("autostart: hidden until the League client is running");
    let mut missing_since: Option<Instant> = None;
    loop {
        tokio::time::sleep(POLL).await;
        let Some(window) = app.get_webview_window("main") else {
            continue;
        };
        // "noclient" is the poller's own verdict; every other phase means the client answered.
        let client_up = st.snapshot().phase != "noclient";
        let visible = window.is_visible().unwrap_or(false);
        if client_up {
            missing_since = None;
            let dismissed = *st.dismissed.lock().unwrap();
            if !visible && !dismissed {
                if let Err(e) = window.show() {
                    log::warn!("autostart: show failed: {e}");
                } else {
                    log::info!("autostart: client up, panel shown");
                }
            }
        } else {
            // A dismissal lasts one client session.
            *st.dismissed.lock().unwrap() = false;
            if visible {
                let since = *missing_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= CLOSE_AFTER {
                    if let Err(e) = window.hide() {
                        log::warn!("autostart: hide failed: {e}");
                    } else {
                        log::info!("autostart: client closed, panel hidden");
                    }
                }
            } else {
                missing_since = None;
            }
        }
    }
}
