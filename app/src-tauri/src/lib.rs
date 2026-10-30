//! Desktop shell. All logic lives in the engine / sources / store crates; this file wires
//! them to a tray icon, two windows, a file watcher, notifications and update checks.

mod commands;
mod milestones;
mod tracker;
mod updater;

use serde::Serialize;
use sources::Source;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex, MutexGuard};
use store::Store;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WindowEvent};
use tauri_plugin_positioner::{Position, WindowExt};

/// Passed by the login item so a launch at login starts quietly in the menu bar.
pub const HIDDEN_ARG: &str = "--hidden";

/// Settings key: "off" makes it an ordinary app (no menu bar or tray icon, quits when its
/// window closes). Anything else, or never set, keeps it running in the menu bar.
pub const MENU_BAR_KEY: &str = "menu_bar";

pub struct App {
    /// Sync and settings changes. Held for the length of a sync pass.
    pub writer: Mutex<Store>,
    /// Everything the UI reads. A second connection, so the UI never waits on a sync (SQLite WAL).
    pub reader: Mutex<Store>,
    pub sources: Vec<Box<dyn Source>>,
    pub tracker: mpsc::Sender<tracker::Msg>,
    pub status: Mutex<SyncStatus>,
    /// Keep running in the menu bar or tray after the window closes.
    pub menu_bar: AtomicBool,
}

impl App {
    pub fn menu_bar(&self) -> bool {
        self.menu_bar.load(Ordering::Relaxed)
    }
}

/// Shows or hides the menu bar icon and remembers the choice for when the window closes.
pub fn apply_menu_bar(handle: &AppHandle, app: &App, enabled: bool) {
    app.menu_bar.store(enabled, Ordering::Relaxed);
    if let Some(tray) = handle.tray_by_id("tray") {
        let _ = tray.set_visible(enabled);
    }
    if !enabled {
        if let Some(w) = handle.get_webview_window("glance") {
            let _ = w.hide();
        }
    }
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
            "quit" => {
                save_main_placement(app);
                app.exit(0);
            }
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

/// Default width of the scorecard window: the page's content plus a little room on each side.
const MAIN_WIDTH: f64 = 940.0;
/// Settings key: the scorecard window's last position and size, "x,y,width,height" in points.
const MAIN_WINDOW_KEY: &str = "window.main";

/// A screen rectangle in points (logical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Rect {
    fn parse(saved: &str) -> Option<Rect> {
        let parts: Vec<f64> = saved.split(',').map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
        match parts[..] {
            [x, y, width, height] if width > 0.0 && height > 0.0 => Some(Rect { x, y, w: width, h: height }),
            _ => None,
        }
    }

    /// A monitor's usable area (below the menu bar, above the Dock), in points.
    fn work_area(m: &tauri::Monitor) -> Rect {
        let (a, scale) = (m.work_area(), m.scale_factor());
        Rect {
            x: f64::from(a.position.x) / scale,
            y: f64::from(a.position.y) / scale,
            w: f64::from(a.size.width) / scale,
            h: f64::from(a.size.height) / scale,
        }
    }

    /// Whether `self` sits entirely inside `area` (a pixel of slack for rounding).
    fn inside(&self, area: &Rect) -> bool {
        self.x >= area.x - 1.0
            && self.y >= area.y - 1.0
            && self.x + self.w <= area.x + area.w + 1.0
            && self.y + self.h <= area.y + area.h + 1.0
    }
}

/// The best fit for a screen: as tall as its usable area, [`MAIN_WIDTH`] wide, centered.
fn best_fit(area: &Rect) -> Rect {
    let w = MAIN_WIDTH.min(area.w);
    Rect { x: area.x + (area.w - w) / 2.0, y: area.y, w, h: area.h }
}

/// Where the scorecard opens: where you left it, if that still fits fully on a screen; otherwise
/// the best fit for the main screen. `None` when no screen can be read (the config size stands).
fn main_placement(saved: Option<Rect>, areas: &[Rect], primary: Option<Rect>) -> Option<Rect> {
    if let Some(r) = saved.filter(|r| areas.iter().any(|a| r.inside(a))) {
        return Some(r);
    }
    primary.or_else(|| areas.first().copied()).map(|a| best_fit(&a))
}

/// Opens the scorecard where it was last left, or sized to the screen the first time.
fn place_main(handle: &AppHandle, saved: Option<&str>) {
    let Some(w) = handle.get_webview_window("main") else { return };
    let areas: Vec<Rect> = w.available_monitors().unwrap_or_default().iter().map(Rect::work_area).collect();
    let primary = w.primary_monitor().ok().flatten().as_ref().map(Rect::work_area);
    let Some(r) = main_placement(saved.and_then(Rect::parse), &areas, primary) else { return };
    let _ = w.set_size(tauri::LogicalSize::new(r.w, r.h));
    let _ = w.set_position(tauri::LogicalPosition::new(r.x, r.y));
}

/// Remembers the scorecard's position and size (outer frame, in points) for next launch.
pub(crate) fn save_main_placement(handle: &AppHandle) {
    let Some(w) = handle.get_webview_window("main") else { return };
    if w.is_minimized().unwrap_or(false) || w.is_fullscreen().unwrap_or(false) {
        return;
    }
    let (Ok(pos), Ok(size), Ok(scale)) = (w.outer_position(), w.outer_size(), w.scale_factor()) else { return };
    let r = format!(
        "{},{},{},{}",
        f64::from(pos.x) / scale,
        f64::from(pos.y) / scale,
        f64::from(size.width) / scale,
        f64::from(size.height) / scale
    );
    let app = handle.state::<std::sync::Arc<App>>();
    let _ = lock(&app.writer).set_setting(MAIN_WINDOW_KEY, &r);
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
    let menu_bar = writer.get_setting(MENU_BAR_KEY)?.as_deref() != Some("off");
    place_main(&handle, writer.get_setting(MAIN_WINDOW_KEY)?.as_deref());
    let (tx, rx) = mpsc::channel();
    let app = std::sync::Arc::new(App {
        writer: Mutex::new(writer),
        reader: Mutex::new(reader),
        sources: sources::all(),
        tracker: tx.clone(),
        status: Mutex::new(SyncStatus::default()),
        menu_bar: AtomicBool::new(menu_bar),
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
    apply_menu_bar(&handle, &app, menu_bar);

    // A launch at login stays in the menu bar; a launch by the user opens the scorecard. With no
    // menu bar icon (turned off, or no tray on this system) there'd be nothing to click, so it
    // always opens.
    if starts_hidden(std::env::args().any(|a| a == HIDDEN_ARG), menu_bar && has_tray) {
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
            // Closing the scorecard hides it and tracking keeps running from the menu bar, unless
            // it's set to be an ordinary app or there's no tray on this system: then closing quits.
            WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
                save_main_placement(window.app_handle());
                let handle = window.app_handle();
                if handle.state::<std::sync::Arc<App>>().menu_bar()
                    && handle.try_state::<HasTray>().is_some_and(|t| t.0)
                {
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
            commands::progress,
            commands::set_goal,
            commands::clear_goal,
            commands::check_prompt,
            commands::learned,
            commands::set_learned,
            commands::get_menu_bar,
            commands::set_menu_bar,
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
        .build(tauri::generate_context!())
        .expect("error while building No, Actually...")
        .run(|handle, event| {
            // Cmd+Q and logout quit without closing the window first.
            if let tauri::RunEvent::ExitRequested { .. } = event {
                save_main_placement(handle);
            }
        });
}

#[cfg(test)]
mod placement_tests {
    use super::{main_placement, Rect, MAIN_WIDTH};

    const LAPTOP: Rect = Rect { x: 0.0, y: 25.0, w: 1512.0, h: 920.0 };
    const SIDE: Rect = Rect { x: 1512.0, y: 0.0, w: 1920.0, h: 1055.0 };

    #[test]
    fn the_first_open_fills_the_screen_height_and_centers() {
        let r = main_placement(None, &[LAPTOP], Some(LAPTOP)).unwrap();
        assert_eq!(r, Rect { x: (1512.0 - MAIN_WIDTH) / 2.0, y: 25.0, w: MAIN_WIDTH, h: 920.0 });
    }

    #[test]
    fn a_saved_window_reopens_as_you_left_it_on_any_screen() {
        let saved = Rect::parse("1600,40,1200,900");
        assert_eq!(main_placement(saved, &[LAPTOP, SIDE], Some(LAPTOP)), saved);
    }

    #[test]
    fn a_saved_window_that_no_longer_fits_goes_back_to_the_best_fit() {
        let best = main_placement(None, &[LAPTOP], Some(LAPTOP));
        // Left on a monitor that's gone, too tall for this one, or hanging off the edge.
        for saved in ["1600,40,1200,900", "100,25,900,1400", "1000,25,900,600"] {
            assert_eq!(main_placement(Rect::parse(saved), &[LAPTOP], Some(LAPTOP)), best, "{saved}");
        }
        assert_eq!(Rect::parse("nonsense"), None);
        assert_eq!(Rect::parse("1,2,0,4"), None);
    }

    #[test]
    fn a_narrow_screen_caps_the_width() {
        let small = Rect { x: 0.0, y: 0.0, w: 800.0, h: 600.0 };
        assert_eq!(main_placement(None, &[small], None).map(|r| r.w), Some(800.0));
    }
}

#[cfg(test)]
mod tray_tests {
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
