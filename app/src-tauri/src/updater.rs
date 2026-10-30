//! Update checks. One thread asks GitHub Releases for `latest.json` a minute after launch and
//! every few hours, if the person allows it. A new version is only announced; nothing is
//! downloaded or installed until they click "Restart to update".

use crate::{lock, App};
use serde::Serialize;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use tauri::menu::MenuItem;
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_updater::{Update, UpdaterExt};

pub enum Msg {
    CheckNow,
}

const FIRST_CHECK: Duration = Duration::from_secs(60);
const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// Settings key. Anything but "off" means on, so automatic checks start enabled.
pub const AUTO_KEY: &str = "updates.auto";

pub fn auto_enabled(setting: Option<&str>) -> bool {
    setting != Some("off")
}

/// Tray item text: an offer to install once a version is waiting, else a manual check.
pub fn menu_text(available: Option<&str>) -> String {
    match available {
        Some(v) => format!("Restart to update to {v}"),
        None => "Check for updates".into(),
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct UpdateStatus {
    pub current: String,
    pub auto: bool,
    pub checking: bool,
    pub installing: bool,
    /// The newer version on offer, if any.
    pub available: Option<String>,
    pub last_check: Option<chrono::DateTime<chrono::Utc>>,
    pub last_error: Option<String>,
}

pub struct Updates {
    pub tx: mpsc::Sender<Msg>,
    status: Mutex<UpdateStatus>,
    pending: Mutex<Option<Update>>,
    menu: MenuItem<Wry>,
}

impl Updates {
    pub fn new(handle: &AppHandle, tx: mpsc::Sender<Msg>, menu: MenuItem<Wry>) -> Self {
        let current = handle.package_info().version.to_string();
        Updates {
            tx,
            status: Mutex::new(UpdateStatus { current, ..Default::default() }),
            pending: Mutex::new(None),
            menu,
        }
    }

    pub fn status(&self, app: &App) -> UpdateStatus {
        let auto = auto_enabled(lock(&app.reader).get_setting(AUTO_KEY).ok().flatten().as_deref());
        UpdateStatus { auto, ..lock(&self.status).clone() }
    }
}

fn changed(handle: &AppHandle, updates: &Updates) {
    let _ = updates.menu.set_text(menu_text(lock(&updates.status).available.as_deref()));
    let _ = handle.emit("update-changed", ());
}

fn check(handle: &AppHandle, updates: &Updates) {
    lock(&updates.status).checking = true;
    changed(handle, updates);
    let found = tauri::async_runtime::block_on(async { handle.updater()?.check().await });
    {
        let mut status = lock(&updates.status);
        status.checking = false;
        status.last_check = Some(chrono::Utc::now());
        match found {
            Ok(update) => {
                status.last_error = None;
                status.available = update.as_ref().map(|u| u.version.clone());
                if let Some(v) = &status.available {
                    log::info!("update available: {v}");
                }
                *lock(&updates.pending) = update;
            }
            Err(e) => {
                log::warn!("update check failed: {e}");
                status.last_error = Some(e.to_string());
            }
        }
    }
    changed(handle, updates);
}

/// Downloads and installs the waiting update, then restarts. Only ever started by a click.
pub async fn install(handle: AppHandle) -> Result<(), String> {
    let updates = handle.state::<Updates>();
    let Some(update) = lock(&updates.pending).clone() else { return Err("No update is waiting.".into()) };
    {
        let mut status = lock(&updates.status);
        if status.installing {
            return Ok(());
        }
        status.installing = true;
    }
    changed(&handle, &updates);
    log::info!("installing update {}", update.version);
    // On Windows this hands over to the installer, which closes the app and reopens it.
    // On Windows the installer closes the app mid-call, so remember the window first.
    crate::save_main_placement(&handle);
    match update.download_and_install(|_, _| {}, || {}).await {
        Ok(()) => {
            handle.request_restart();
            Ok(())
        }
        Err(e) => {
            log::warn!("update install failed: {e}");
            let mut status = lock(&updates.status);
            status.installing = false;
            status.last_error = Some(e.to_string());
            drop(status);
            changed(&handle, &updates);
            Err(e.to_string())
        }
    }
}

/// The tray item: installs a waiting update, otherwise checks now.
pub fn on_menu(handle: &AppHandle) {
    let updates = handle.state::<Updates>();
    if lock(&updates.pending).is_some() {
        let handle = handle.clone();
        tauri::async_runtime::spawn(async move {
            let _ = install(handle).await;
        });
    } else {
        let _ = updates.tx.send(Msg::CheckNow);
    }
}

pub fn spawn(handle: AppHandle, app: Arc<App>, rx: mpsc::Receiver<Msg>) {
    std::thread::spawn(move || {
        let updates = handle.state::<Updates>();
        let mut wait = FIRST_CHECK;
        loop {
            match rx.recv_timeout(wait) {
                Ok(Msg::CheckNow) => check(&handle, &updates),
                // Development builds never check on their own: they'd offer to replace themselves.
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if !cfg!(debug_assertions) && updates.status(&app).auto {
                        check(&handle, &updates);
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
            wait = INTERVAL;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_checks_default_on() {
        assert!(auto_enabled(None));
        assert!(auto_enabled(Some("on")));
        assert!(!auto_enabled(Some("off")));
    }

    #[test]
    fn menu_offers_install_only_when_waiting() {
        assert_eq!(menu_text(None), "Check for updates");
        assert_eq!(menu_text(Some("0.2.0")), "Restart to update to 0.2.0");
    }
}
