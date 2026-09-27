//! One overlay at a time. Launching recall.exe while it already runs (the Startup copy hidden, or
//! its panel dismissed with the x button) asks the running overlay to show its panel and exits,
//! instead of starting a second poller that imports runes, spells and item sets in parallel with
//! the first (seen on 2026-09-26: two copies writing the same rune page and item sets).
use std::fs::File;
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// How often the running overlay looks for a show request from a second launch.
const CHECK: Duration = Duration::from_millis(1500);
/// Windows `ERROR_SHARING_VIOLATION`: the lock file is held open by another process.
const SHARING_VIOLATION: i32 = 32;

pub enum Claim {
    /// This is the only overlay; keep the handle for the life of the process.
    First(Option<File>),
    /// Another overlay holds the lock.
    Second,
}

fn lock_path() -> PathBuf {
    crate::settings::data_dir().join("instance.lock")
}

fn request_path() -> PathBuf {
    crate::settings::data_dir().join("show.request")
}

/// An exclusive handle on the lock file: the OS releases it when the process ends, crash or not.
pub fn claim() -> Claim {
    #[cfg(windows)]
    let opened = {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .open(lock_path())
    };
    #[cfg(not(windows))]
    let opened = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path());
    match opened {
        Ok(file) => {
            // A request left over from an earlier session must not pop the panel at logon.
            let _ = std::fs::remove_file(request_path());
            Claim::First(Some(file))
        }
        Err(error) if error.raw_os_error() == Some(SHARING_VIOLATION) => Claim::Second,
        Err(error) => {
            log::warn!("single-instance lock unavailable ({error}); starting anyway");
            Claim::First(None)
        }
    }
}

/// Called by a second launch before it exits.
pub fn ask_running_overlay_to_show() {
    if let Err(error) = std::fs::write(request_path(), b"show") {
        log::warn!("could not ask the running overlay to show its panel: {error}");
    }
}

/// In the running overlay: show the panel whenever a second launch asks, even after the x button
/// dismissed it for this client session.
pub async fn serve_show_requests(app: AppHandle, st: std::sync::Arc<crate::App>) {
    loop {
        tokio::time::sleep(CHECK).await;
        if std::fs::remove_file(request_path()).is_err() {
            continue;
        }
        *st.dismissed.lock().unwrap() = false;
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.unminimize();
            match window.show() {
                Ok(()) => log::info!("another launch asked for the panel; shown"),
                Err(error) => log::warn!("show on request failed: {error}"),
            }
            let _ = window.set_focus();
        }
    }
}
