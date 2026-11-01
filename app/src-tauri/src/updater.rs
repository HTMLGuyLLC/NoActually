//! Update checks. One thread asks GitHub Releases for `latest.json` a minute after launch and
//! every few hours, if the person allows it. A new version is only announced; nothing is
//! downloaded or installed until they click "Restart to update". The stable channel reads the
//! address in `tauri.conf.json`; the beta channel reads [`BETA_ENDPOINT`].

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
/// How long a check for a new version may take. One stalled request would otherwise block the only
/// update worker for good.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the download of an update may take, start to end. A stalled one would otherwise leave
/// the app "installing" until it's restarted.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Settings key. Anything but "off" means on, so automatic checks start enabled.
pub const AUTO_KEY: &str = "updates.auto";

/// Settings key. Anything but "beta" means stable, so people get full releases unless they ask.
pub const CHANNEL_KEY: &str = "updates.channel";
pub const STABLE: &str = "stable";
pub const BETA: &str = "beta";
/// Where the beta channel's `latest.json` lives: a release that holds only that file, which
/// `scripts/publish-release.sh` replaces. It names the newest release, beta or not.
const BETA_ENDPOINT: &str = "https://github.com/HTMLGuyLLC/NoActually/releases/download/beta-channel/latest.json";

/// The channel a saved setting means.
pub fn channel(setting: Option<&str>) -> &'static str {
    if setting == Some(BETA) {
        BETA
    } else {
        STABLE
    }
}

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
    pub channel: String,
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
        let reader = lock(&app.reader);
        let auto = auto_enabled(reader.get_setting(AUTO_KEY).ok().flatten().as_deref());
        let channel = channel(reader.get_setting(CHANNEL_KEY).ok().flatten().as_deref()).to_string();
        UpdateStatus { auto, channel, ..lock(&self.status).clone() }
    }

    /// Forgets what the old channel offered, so a beta isn't left waiting after a move to stable.
    pub fn forget_offer(&self, handle: &AppHandle) {
        *lock(&self.pending) = None;
        lock(&self.status).available = None;
        changed(handle, self);
    }
}

fn changed(handle: &AppHandle, updates: &Updates) {
    let _ = updates.menu.set_text(menu_text(lock(&updates.status).available.as_deref()));
    let _ = handle.emit("update-changed", ());
}

/// Asks the channel's address for the newest version.
async fn newest(handle: &AppHandle, channel: &str) -> tauri_plugin_updater::Result<Option<Update>> {
    let builder = handle.updater_builder().timeout(CHECK_TIMEOUT);
    let updater =
        if channel == BETA { builder.endpoints(vec![BETA_ENDPOINT.parse()?])?.build()? } else { builder.build()? };
    updater.check().await
}

fn check(handle: &AppHandle, app: &App, updates: &Updates) {
    lock(&updates.status).checking = true;
    changed(handle, updates);
    let channel = updates.status(app).channel;
    let found = tauri::async_runtime::block_on(newest(handle, &channel));
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
                // A background check failing isn't news while an update is already waiting.
                if status.available.is_none() {
                    status.last_error = Some(e.to_string());
                }
            }
        }
    }
    changed(handle, updates);
}

/// An install ended badly: the status keeps why, and no install is running any more. Every failure
/// path goes through here, so the window and the tray read the same `last_error`.
fn install_failed(status: &mut UpdateStatus, message: &str) {
    status.installing = false;
    status.last_error = Some(message.to_string());
}

/// Downloads and installs the waiting update, then restarts. Only ever started by a click.
pub async fn install(handle: AppHandle) -> Result<(), String> {
    let updates = handle.state::<Updates>();
    let Some(mut update) = lock(&updates.pending).clone() else {
        let message = "No update is waiting.";
        install_failed(&mut lock(&updates.status), message);
        changed(&handle, &updates);
        return Err(message.into());
    };
    {
        let mut status = lock(&updates.status);
        if status.installing {
            return Ok(());
        }
        status.installing = true;
        status.last_error = None;
    }
    // The check's short timeout was copied onto the update; a download needs longer.
    update.timeout = Some(DOWNLOAD_TIMEOUT);
    changed(&handle, &updates);
    log::info!("installing update {}", update.version);
    // On Windows this hands over to the installer, which closes the app and reopens it.
    // On Windows the installer closes the app mid-call, so remember the window first.
    crate::save_main_placement_before_exit(&handle);
    match update.download_and_install(|_, _| {}, || {}).await {
        Ok(()) => {
            handle.request_restart();
            Ok(())
        }
        Err(e) => {
            log::warn!("update install failed: {e}");
            install_failed(&mut lock(&updates.status), &e.to_string());
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
            // The failure is already in `last_error` (and `update-changed` fired) for the window to show.
            if let Err(e) = install(handle).await {
                log::warn!("update from the tray failed: {e}");
            }
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
                Ok(Msg::CheckNow) => check(&handle, &app, &updates),
                // Development builds never check on their own: they'd offer to replace themselves.
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if !cfg!(debug_assertions) && updates.status(&app).auto {
                        check(&handle, &app, &updates);
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
    fn the_channel_is_stable_unless_beta_was_picked() {
        assert_eq!(channel(None), STABLE);
        assert_eq!(channel(Some("stable")), STABLE);
        assert_eq!(channel(Some("nightly")), STABLE);
        assert_eq!(channel(Some("beta")), BETA);
    }

    #[test]
    fn the_beta_address_is_a_valid_url() {
        assert!(BETA_ENDPOINT.parse::<tauri::Url>().is_ok());
    }

    #[test]
    fn the_timeouts_are_bounded() {
        assert_eq!(CHECK_TIMEOUT, Duration::from_secs(30));
        assert_eq!(DOWNLOAD_TIMEOUT, Duration::from_secs(600));
        assert!(DOWNLOAD_TIMEOUT > CHECK_TIMEOUT, "a download can't be cut at the check's limit");
    }

    #[test]
    fn a_failed_install_stops_installing_and_keeps_why() {
        let mut status = UpdateStatus { installing: true, ..Default::default() };
        install_failed(&mut status, "request timed out");
        assert!(!status.installing);
        assert_eq!(status.last_error.as_deref(), Some("request timed out"));
    }

    #[test]
    fn menu_offers_install_only_when_waiting() {
        assert_eq!(menu_text(None), "Check for updates");
        assert_eq!(menu_text(Some("0.2.0")), "Restart to update to 0.2.0");
    }
}
