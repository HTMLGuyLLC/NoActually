//! Desktop shell. All logic lives in the engine / sources / store crates; this file wires
//! them to a tray icon, two windows, a file watcher, notifications and update checks.

mod commands;
mod milestones;
mod tracker;
mod updater;

use serde::Serialize;
use sources::Source;
use std::sync::{mpsc, Mutex, MutexGuard};
use store::Store;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WindowEvent};
use tauri_plugin_positioner::{Position, WindowExt};

/// Passed by the login item so a launch at login starts quietly in the menu bar.
pub const HIDDEN_ARG: &str = "--hidden";

pub struct App {
    /// Sync and settings changes. Held for the length of a sync pass.
    pub writer: Mutex<Store>,
    /// Everything the UI reads. A second connection, so the UI never waits on a sync (SQLite WAL).
    pub reader: Mutex<Store>,
    pub sources: Vec<Box<dyn Source>>,
    pub tracker: mpsc::Sender<tracker::Msg>,
    pub status: Mutex<SyncStatus>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncStatus {
    pub syncing: bool,
    pub last_sync: Option<chrono::DateTime<chrono::Utc>>,
    pub last_error: Option<String>,
    pub files_with_errors: usize,
}

/// A panicked holder must not take the whole app down with it: the data behind the lock is
/// SQLite, which is transactional, so carrying on is safe.
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn show_main(handle: &AppHandle) {
    if let Some(w) = handle.get_webview_window("glance") {
        let _ = w.hide();
    }
    #[cfg(target_os = "macos")]
    let _ = handle.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(w) = handle.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn hide_main(window: &tauri::Window) {
    let _ = window.hide();
    // With the scorecard closed the app lives in the menu bar only, like other menu bar apps.
    #[cfg(target_os = "macos")]
    let _ = window.app_handle().set_activation_policy(tauri::ActivationPolicy::Accessory);
}

fn toggle_glance(handle: &AppHandle) {
    let Some(w) = handle.get_webview_window("glance") else { return };
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
    } else {
        // Above the icon on a bottom taskbar, below it on a top menu bar.
        let _ = w.move_window(Position::TrayCenter);
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn tray_icon() -> tauri::image::Image<'static> {
    // macOS tints a template icon for light and dark menu bars; elsewhere use the color icon.
    if cfg!(target_os = "macos") {
        tauri::include_image!("icons/tray.png")
    } else {
        tauri::include_image!("icons/32x32.png")
    }
}

/// Whether the menu bar icon was built. Without one, the scorecard is the only way into the app.
pub struct HasTray(pub bool);

/// Runs `build`, turning an error or a panic into `false`. On Linux the tray library is loaded
/// at runtime and panics when it's missing (it isn't bundled, and not every distro has it).
fn tray_built(build: impl FnOnce() -> tauri::Result<()>) -> bool {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(build)) {
        Ok(Ok(())) => true,
        Ok(Err(e)) => {
            log::warn!("no menu bar icon: {e}");
            false
        }
        Err(_) => {
            log::warn!("no menu bar icon: the system tray library is missing");
            false
        }
    }
}

/// A launch at login starts quietly in the menu bar, but only when there is a menu bar icon.
fn starts_hidden(hidden_arg: bool, has_tray: bool) -> bool {
    hidden_arg && has_tray
}

/// The menu bar icon: left click toggles the quick glance, the menu does the rest.
fn build_tray(tauri_app: &tauri::App, menu: &Menu<tauri::Wry>) -> tauri::Result<()> {
    TrayIconBuilder::with_id("tray")
        .icon(tray_icon())
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("No, Actually...")
        .menu(menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id.as_ref() {
            "open" => show_main(app),
            "sync" => {
                let _ = app.state::<std::sync::Arc<App>>().tracker.send(tracker::Msg::SyncNow);
            }
            "update" => updater::on_menu(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                toggle_glance(tray.app_handle());
            }
        })
        .build(tauri_app)?;
    Ok(())
}

/// Opens the database, builds the tray, and starts the tracker and update threads.
fn setup(tauri_app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = tauri_app.handle().clone();
    // The same file the CLI opens. NOACTUALLY_DB points at another one (development, demos).
    let db = store::default_db_path()?;
    if store::legacy_db_env_in_use() {
        log::warn!("{} is deprecated; set {} instead", store::LEGACY_DB_ENV, store::DB_ENV);
    }
    log::info!("database: {}", db.display());
    let writer = Store::open(&db)?;
    let reader = Store::open(&db)?;
    let (tx, rx) = mpsc::channel();
    let app = std::sync::Arc::new(App {
        writer: Mutex::new(writer),
        reader: Mutex::new(reader),
        sources: sources::all(),
        tracker: tx.clone(),
        status: Mutex::new(SyncStatus::default()),
    });
    tauri_app.manage(app.clone());

    let open = MenuItem::with_id(tauri_app, "open", "Open scorecard", true, None::<&str>)?;
    let sync = MenuItem::with_id(tauri_app, "sync", "Check now", true, None::<&str>)?;
    let update = MenuItem::with_id(tauri_app, "update", updater::menu_text(None), true, None::<&str>)?;
    let quit = MenuItem::with_id(tauri_app, "quit", "Quit No, Actually...", true, None::<&str>)?;
    let menu = Menu::with_items(tauri_app, &[&open, &sync, &update, &quit])?;
    let (update_tx, update_rx) = mpsc::channel();
    tauri_app.manage(updater::Updates::new(&handle, update_tx, update));
    let has_tray = tray_built(|| build_tray(tauri_app, &menu));
    tauri_app.manage(HasTray(has_tray));

    // A launch at login stays in the menu bar; a launch by the user opens the scorecard.
    if starts_hidden(std::env::args().any(|a| a == HIDDEN_ARG), has_tray) {
        #[cfg(target_os = "macos")]
        let _ = handle.set_activation_policy(tauri::ActivationPolicy::Accessory);
    } else {
        show_main(&handle);
    }
    updater::spawn(handle.clone(), app.clone(), update_rx);
    tracker::spawn(handle, app, tx, rx);
    Ok(())
}

/// Starts the desktop app.
///
/// # Panics
/// If Tauri can't start (no window system, a broken bundle).
pub fn run() {
    tauri::Builder::default()
        // First: a second launch just brings the running app forward, so two trackers never
        // write the same database.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .max_file_size(2_000_000)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepOne)
                .build(),
        )
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![HIDDEN_ARG]),
        ))
        .setup(setup)
        .on_window_event(|window, event| match event {
            // Closing the scorecard hides it; tracking keeps running from the menu bar. With no
            // menu bar icon there'd be no way back, so closing quits.
            WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
                if window.app_handle().try_state::<HasTray>().is_some_and(|t| t.0) {
                    api.prevent_close();
                    hide_main(window);
                } else {
                    window.app_handle().exit(0);
                }
            }
            WindowEvent::Focused(false) if window.label() == "glance" => {
                let _ = window.hide();
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::meta,
            commands::stats,
            commands::list_sources,
            commands::allow_source,
            commands::revoke_source,
            commands::forget_source,
            commands::pause_source,
            commands::import_export,
            commands::get_retention,
            commands::set_retention,
            commands::pin,
            commands::sync_now,
            commands::sync_status,
            commands::open_main,
            commands::get_autostart,
            commands::set_autostart,
            commands::save_share_card,
            commands::open_share,
            commands::update_status,
            commands::set_auto_update,
            commands::check_for_updates,
            commands::install_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running No, Actually...");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_tray_library_is_survived() {
        assert!(tray_built(|| Ok(())));
        assert!(!tray_built(|| Err(tauri::Error::FailedToReceiveMessage)));
        // What libappindicator-sys does when neither appindicator library loads.
        assert!(!tray_built(|| panic!("Failed to load ayatana-appindicator3 or appindicator3 dynamic library")));
    }

    #[test]
    fn hidden_launch_needs_a_tray() {
        assert!(starts_hidden(true, true));
        assert!(!starts_hidden(true, false), "no tray: the window must show");
        assert!(!starts_hidden(false, true));
        assert!(!starts_hidden(false, false));
    }
}
