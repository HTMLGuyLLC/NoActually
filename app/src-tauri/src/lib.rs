//! Desktop shell. All logic lives in the engine / sources / store crates; this file wires
//! them to a tray icon, two windows, a file watcher, notifications and update checks.

mod commands;
mod milestones;
mod report_cards;
mod tracker;
mod updater;

use serde::Serialize;
use sources::Source;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex, MutexGuard};
use std::time::Duration;
use store::Store;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WindowEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_positioner::{Position, WindowExt};

/// Passed by the login item so a launch at login starts quietly in the menu bar.
pub const HIDDEN_ARG: &str = "--hidden";

/// Passed by CI: opens a temporary database, builds the app and exits 0, with no window and no
/// network. Hidden: it isn't in any help or doc a user reads.
pub const SELF_CHECK_ARG: &str = "--self-check";

/// Settings key: "off" turns off every popup the app sends (milestones and the rescored notice).
/// Anything else, or never set, keeps them on.
pub const POPUPS_KEY: &str = "popups.enabled";

/// Settings key: when scores were last rebuilt under new rules (RFC 3339, UTC), so the window can
/// tell the person even if the popup was off or missed.
pub const RESCORED_NOTICE_KEY: &str = "rescored.notice";

/// The daily backup's file name, beside the database (`BACKUP_FILE` in the store crate).
const BACKUP_FILE_NAME: &str = "noactually-backup.db";

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
    /// The pass in progress reads history again, so the numbers are partial until it ends.
    pub rebuilding: bool,
}

/// Whether popups are on, given the saved setting.
pub fn popups_enabled(setting: Option<&str>) -> bool {
    setting != Some("off")
}

/// Shows a popup, unless the person turned popups off. Every notification goes through here.
pub fn notify(handle: &AppHandle, app: &App, title: impl Into<String>, body: impl Into<String>) {
    let setting = lock(&app.reader).get_setting(POPUPS_KEY).ok().flatten();
    if popups_enabled(setting.as_deref()) {
        let _ = handle.notification().builder().title(title).body(body).show();
    }
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

/// Whether the desktop shows tray icons. GNOME without the `AppIndicator` extension loads the
/// tray library fine but never shows the icon, so ask D-Bus whether a `StatusNotifierWatcher` is
/// running. When unsure, say no: the window then stays the way in, and closing it quits.
#[cfg(target_os = "linux")]
fn tray_host_running() -> bool {
    let has_owner = || {
        let bus = zbus::blocking::Connection::session().ok()?;
        let dbus = zbus::blocking::fdo::DBusProxy::new(&bus).ok()?;
        let name = zbus::names::BusName::try_from("org.kde.StatusNotifierWatcher").ok()?;
        dbus.name_has_owner(name).ok()
    };
    let shown = has_owner().unwrap_or(false);
    if !shown {
        log::warn!("no menu bar icon: the desktop has no tray (StatusNotifierWatcher) running");
    }
    shown
}

#[cfg(not(target_os = "linux"))]
fn tray_host_running() -> bool {
    true
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
                save_main_placement_before_exit(app);
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
const MAIN_WIDTH: f64 = 1085.0;
/// Default height of the scorecard window, on a screen tall enough: the whole scorecard, no scrolling.
const MAIN_HEIGHT: f64 = 1195.0;
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

/// The best fit for a screen: [`MAIN_WIDTH`] by [`MAIN_HEIGHT`], or as much of that as it has, centered.
fn best_fit(area: &Rect) -> Rect {
    let (w, h) = (MAIN_WIDTH.min(area.w), MAIN_HEIGHT.min(area.h));
    Rect { x: area.x + (area.w - w) / 2.0, y: area.y + (area.h - h) / 2.0, w, h }
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

/// The scorecard's position and size as saved: "x,y,width,height" in points.
fn placement_string(pos: (i32, i32), size: (u32, u32), scale: f64) -> String {
    format!(
        "{},{},{},{}",
        f64::from(pos.0) / scale,
        f64::from(pos.1) / scale,
        f64::from(size.0) / scale,
        f64::from(size.1) / scale
    )
}

/// How long a quit waits for the placement to be written. A sync pass can hold the writer far longer.
const EXIT_SAVE_WAIT: Duration = Duration::from_millis(1500);

/// Remembers the scorecard's position and size (outer frame, in points) for next launch. The window
/// is read here, but the write waits for the writer on its own thread, so a long sync pass never
/// freezes the window. The receiver hears when it's written.
pub(crate) fn save_main_placement(handle: &AppHandle) -> Option<mpsc::Receiver<()>> {
    let w = handle.get_webview_window("main")?;
    if w.is_minimized().unwrap_or(false) || w.is_fullscreen().unwrap_or(false) {
        return None;
    }
    let (Ok(pos), Ok(size), Ok(scale)) = (w.outer_position(), w.outer_size(), w.scale_factor()) else { return None };
    let r = placement_string((pos.x, pos.y), (size.width, size.height), scale);
    let app = std::sync::Arc::clone(&*handle.try_state::<std::sync::Arc<App>>()?);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = lock(&app.writer).set_setting(MAIN_WINDOW_KEY, &r);
        let _ = tx.send(());
    });
    Some(rx)
}

/// [`save_main_placement`] for a quit: waits a moment for the write, but never for a whole sync pass.
pub(crate) fn save_main_placement_before_exit(handle: &AppHandle) {
    // A tray Quit waits here and then again on the exit it asks for: once is enough.
    static WAITED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if WAITED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    if let Some(done) = save_main_placement(handle) {
        let _ = done.recv_timeout(EXIT_SAVE_WAIT);
    }
}

/// One line for the log: the panic's message and where it happened.
fn panic_line(info: &std::panic::PanicHookInfo<'_>) -> String {
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
        .unwrap_or("no message");
    let message = strip_quoted(message);
    match info.location() {
        Some(at) => format!("panic: {message} at {}:{}", at.file(), at.line()),
        None => format!("panic: {message}"),
    }
}

/// `message` with whatever sits inside double quotes or backticks replaced by `…`. A panic's text can
/// quote a path or a line of a chat (`unwrap()` on an error that holds one), and the log gets shared
/// in bug reports. An unclosed quote hides the rest.
fn strip_quoted(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut chars = message.chars();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '"' || c == '`' {
            let mut escaped = false;
            for d in chars.by_ref() {
                match d {
                    _ if escaped => escaped = false,
                    '\\' if c == '"' => escaped = true,
                    _ if d == c => break,
                    _ => {}
                }
            }
            out.push('…');
            out.push(c);
        }
    }
    out
}

/// `path` with the home folder written as `~`, for the log: it is shared in bug reports, and the
/// folder is the user's name.
fn shorten_home(path: &std::path::Path, home: Option<&std::path::Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

/// `text` with the home folder written as `~` wherever it appears, for log lines that quote a path.
/// Whole folders only: `/Users/me` leaves `/Users/mellow` alone.
pub(crate) fn shorten_home_in(text: &str, home: Option<&Path>) -> String {
    let Some(home) = home.map(|h| h.display().to_string()).filter(|h| !h.is_empty()) else { return text.to_string() };
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(&home) {
        let after = &rest[at + home.len()..];
        let whole = after.chars().next().is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '-' || c == '.'));
        out.push_str(&rest[..at]);
        out.push_str(if whole { "~" } else { &home });
        rest = after;
    }
    out.push_str(rest);
    out
}

/// `path` for the log, with the home folder written as `~`.
pub(crate) fn home_short(path: &Path) -> String {
    shorten_home(path, std::env::home_dir().as_deref())
}

/// Writes every panic to the log, then runs the default hook so a panic still prints as before.
fn log_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("{}", panic_line(info));
        default(info);
    }));
}

/// Whether the launch asks for the CI self-check.
fn is_self_check(mut args: impl Iterator<Item = std::ffi::OsString>) -> bool {
    args.any(|a| a == SELF_CHECK_ARG)
}

/// The throwaway folder a self-check keeps its database in.
fn self_check_dir() -> PathBuf {
    std::env::temp_dir().join(format!("noactually-self-check-{}", std::process::id()))
}

/// What the dialog says when the database can't be opened at launch. Plain words: the person
/// has no window to look at, only this.
fn startup_failure_text(e: &store::Error, log_dir: Option<&Path>, db: Option<&Path>) -> String {
    let mut text = if matches!(e, store::Error::NewerDatabase { .. }) {
        "Your data was saved by a newer version of No, Actually... Download the latest version from \
         github.com/HTMLGuyLLC/NoActually/releases/latest to open it. Nothing was changed."
            .to_string()
    } else {
        let mut t = format!("No, Actually... couldn't open its database.\n\n{e}");
        if let Some(folder) = db.and_then(Path::parent) {
            t = format!(
                "{t}\n\nA daily backup, if one was made, is {BACKUP_FILE_NAME} in {}. \
                 Copy it somewhere safe before changing anything.",
                folder.display()
            );
        }
        t
    };
    if let Some(dir) = log_dir {
        text = format!("{text}\n\nThe log is in {}.", dir.display());
    }
    text
}

/// Tells the person why the app can't start, in a native dialog, then exits when it's closed.
/// Without this the app would stop with no window and no word.
fn startup_failed(handle: &AppHandle, db: Option<&Path>, e: &store::Error) {
    log::error!("couldn't open the database: {e}");
    let log_dir = handle.path().app_log_dir().ok();
    let text = startup_failure_text(e, log_dir.as_deref(), db);
    let exit = handle.clone();
    // Not a blocking dialog: this runs on the main thread, which has to keep running to draw it.
    handle
        .dialog()
        .message(text)
        .title("No, Actually... can't start")
        .kind(MessageDialogKind::Error)
        .show(move |_| exit.exit(1));
}

/// The database path and the two connections to it (writer, reader). On failure, the path if it
/// was known and the error.
type Opened = (PathBuf, Store, Store);
fn open_databases(self_check: bool) -> Result<Opened, (Option<PathBuf>, store::Error)> {
    // The same file the CLI opens. NOACTUALLY_DB points at another one (development, demos).
    let db = if self_check {
        self_check_dir().join(store::DB_FILE)
    } else {
        store::default_db_path().map_err(|e| (None, e))?
    };
    if store::legacy_db_env_in_use() {
        log::warn!("{} is deprecated; set {} instead", store::LEGACY_DB_ENV, store::DB_ENV);
    }
    log::info!("database: {}", home_short(&db));
    let open = || Store::open(&db).map_err(|e| (Some(db.clone()), e));
    let (writer, reader) = (open()?, open()?);
    Ok((db, writer, reader))
}

/// Opens the database, builds the tray, and starts the tracker and update threads.
fn setup(tauri_app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    log_panics();
    let handle = tauri_app.handle().clone();
    let self_check = is_self_check(std::env::args_os());
    log::info!(
        "No, Actually... {} on {} {}, scoring rules {}",
        tauri_app.package_info().version,
        std::env::consts::OS,
        std::env::consts::ARCH,
        engine::RULES_VERSION
    );
    let (db, writer, reader) = match open_databases(self_check) {
        Ok(opened) => opened,
        Err((_, e)) if self_check => return Err(e.into()),
        Err((db, e)) => {
            startup_failed(&handle, db.as_deref(), &e);
            return Ok(()); // no window opens; the dialog's button ends the app
        }
    };
    // The trail a rebuild or migration leaves: what the database was last scored under.
    log::info!(
        "database format {}, scored under rules {}",
        writer.schema_version()?,
        writer.get_setting("rules_version")?.as_deref().unwrap_or("none")
    );
    match report_cards::cli_path::publish(&db) {
        Ok(report_cards::cli_path::Published::Translocated) => {
            log::warn!("running from a temporary copy; not recording where the report-card helper is");
        }
        Ok(_) => {}
        Err(e) => log::warn!("couldn't record where the report-card helper is: {e}"),
    }
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
    let has_tray = tray_host_running() && tray_built(|| build_tray(tauri_app, &menu));
    tauri_app.manage(HasTray(has_tray));
    apply_menu_bar(&handle, &app, menu_bar);

    if self_check {
        // Everything above is what a launch builds. No window, no threads, no network: done.
        log::info!("self-check passed");
        handle.exit(0);
        return Ok(());
    }

    // A launch at login stays in the menu bar; a launch by the user opens the scorecard. With no
    // menu bar icon (turned off, or no tray on this system) there'd be nothing to click, so it
    // always opens.
    if starts_hidden(std::env::args_os().any(|a| a == HIDDEN_ARG), menu_bar && has_tray) {
        #[cfg(target_os = "macos")]
        let _ = handle.set_activation_policy(tauri::ActivationPolicy::Accessory);
    } else {
        show_main(&handle);
    }
    updater::spawn(handle.clone(), app.clone(), update_rx);
    // After an update, moves the Claude Code plugin to the new release in the background.
    report_cards::spawn_launch_check();
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
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // No arguments in the log: they can hold paths.
            log::info!("a second launch brought the scorecard forward");
            show_main(app);
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .max_file_size(2_000_000)
                // The log in use and one older one (about 4 MB in all), so a rotation never wipes the lead-up.
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(1))
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
                let handle = window.app_handle();
                if handle.try_state::<std::sync::Arc<App>>().is_some_and(|a| a.menu_bar())
                    && handle.try_state::<HasTray>().is_some_and(|t| t.0)
                {
                    // Hiding never waits: the placement is written in the background.
                    let _ = save_main_placement(handle);
                    api.prevent_close();
                    hide_main(window);
                } else {
                    save_main_placement_before_exit(handle);
                    handle.exit(0);
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
            commands::set_update_channel,
            commands::check_for_updates,
            commands::install_update,
            commands::log_error,
            commands::open_log_folder,
            commands::open_notices,
            commands::get_popups,
            commands::set_popups,
            commands::rescored_notice,
            commands::recent_report_cards,
            report_cards::report_cards_status,
            report_cards::report_cards_turn_on,
            report_cards::report_cards_turn_off,
            report_cards::report_cards_set_tips,
            report_cards::report_cards_set_reminders,
            report_cards::report_cards_delete_history,
        ])
        .build(tauri::generate_context!())
        .expect("error while building No, Actually...")
        .run(|handle, event| match event {
            // Cmd+Q and logout quit without closing the window first.
            tauri::RunEvent::ExitRequested { .. } => save_main_placement_before_exit(handle),
            tauri::RunEvent::Exit if is_self_check(std::env::args_os()) => {
                let _ = std::fs::remove_dir_all(self_check_dir());
            }
            _ => {}
        });
}

#[cfg(test)]
mod placement_tests {
    use super::{main_placement, Rect, MAIN_HEIGHT, MAIN_WIDTH};

    const LAPTOP: Rect = Rect { x: 0.0, y: 25.0, w: 1512.0, h: 920.0 };
    const SIDE: Rect = Rect { x: 1512.0, y: 0.0, w: 1920.0, h: 1055.0 };

    #[test]
    fn the_first_open_fills_the_screen_height_and_centers() {
        let r = main_placement(None, &[LAPTOP], Some(LAPTOP)).unwrap();
        assert_eq!(r, Rect { x: (1512.0 - MAIN_WIDTH) / 2.0, y: 25.0, w: MAIN_WIDTH, h: 920.0 });
    }

    #[test]
    fn a_big_screen_gets_the_full_default_size_centered() {
        let big = Rect { x: 0.0, y: 25.0, w: 2560.0, h: 1415.0 };
        let r = main_placement(None, &[big], Some(big)).unwrap();
        assert_eq!(r, Rect { x: (2560.0 - MAIN_WIDTH) / 2.0, y: 135.0, w: MAIN_WIDTH, h: MAIN_HEIGHT });
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

#[cfg(test)]
mod panic_tests {
    use super::{panic_line, shorten_home, strip_quoted};
    use std::path::Path;
    use std::sync::Mutex;

    #[test]
    fn a_panic_line_has_the_message_and_the_place() {
        static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());
        let before = std::panic::take_hook();
        std::panic::set_hook(Box::new(|info| SEEN.lock().unwrap().push(panic_line(info))));
        let _ = std::panic::catch_unwind(|| panic!("bad file {}", 7));
        std::panic::set_hook(before);
        let seen = SEEN.lock().unwrap();
        let line = seen.iter().find(|l| l.contains("bad file 7")).expect("hook saw the panic");
        assert!(line.starts_with("panic: bad file 7 at ") && line.contains("lib.rs:"), "{line}");
    }

    #[test]
    fn quoted_text_is_cut_from_a_logged_panic() {
        let message = r#"called `Result::unwrap()` on an `Err` value: Io { path: "/Users/me/chat.jsonl", note: "say \"hi\" ok" } done"#;
        assert_eq!(strip_quoted(message), r#"called `…` on an `…` value: Io { path: "…", note: "…" } done"#);
        assert_eq!(strip_quoted("index out of bounds: the len is 3"), "index out of bounds: the len is 3");
        assert_eq!(strip_quoted(r#"bad line "never closed"#), r#"bad line "…""#, "an open quote hides the rest");
        assert_eq!(strip_quoted("don't panic"), "don't panic", "apostrophes are not quotes");
    }

    #[test]
    fn the_home_folder_is_written_as_a_tilde_in_the_log() {
        let home = Some(Path::new("/Users/me"));
        let sep = std::path::MAIN_SEPARATOR;
        let db = Path::new("/Users/me").join("Library").join("noactually.db");
        assert_eq!(shorten_home(&db, home), format!("~{sep}Library{sep}noactually.db"));
        assert_eq!(shorten_home(Path::new("/Users/me"), home), "~");
        assert_eq!(shorten_home(Path::new("/Users/mellow/x.db"), home), "/Users/mellow/x.db", "whole folders only");
        assert_eq!(shorten_home(&db, None), db.display().to_string());
    }
}

#[cfg(test)]
mod launch_tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn the_self_check_flag_is_found_among_the_arguments() {
        let args = |v: &[&str]| v.iter().map(OsString::from).collect::<Vec<_>>().into_iter();
        assert!(is_self_check(args(&["--self-check"])));
        assert!(is_self_check(args(&["--hidden", "--self-check"])));
        assert!(!is_self_check(args(&["--hidden"])));
        assert!(!is_self_check(args(&[])));
    }

    #[test]
    fn the_self_check_database_is_in_its_own_temporary_folder() {
        let dir = self_check_dir();
        assert!(dir.starts_with(std::env::temp_dir()));
        assert!(dir.to_string_lossy().contains(&std::process::id().to_string()));
        let opened = open_databases(true).expect("a temporary database opens");
        assert!(dir.join(store::DB_FILE).exists());
        drop(opened);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failed_open_names_the_log_folder_and_the_backup() {
        let db = Path::new("/data/noactually.db");
        let logs = Path::new("/logs/app.noactually");
        let broken = store::Error::Invalid("disk is full");
        let text = startup_failure_text(&broken, Some(logs), Some(db));
        assert!(text.contains("disk is full"), "{text}");
        assert!(text.contains(BACKUP_FILE_NAME) && text.contains("/data"), "{text}");
        assert!(text.contains("/logs/app.noactually"), "{text}");
    }

    #[test]
    fn a_newer_database_says_to_update_the_app() {
        let newer = store::Error::NewerDatabase { found: 13, known: 12 };
        let text = startup_failure_text(&newer, Some(Path::new("/logs")), Some(Path::new("/data/noactually.db")));
        assert!(text.contains("releases/latest"), "{text}");
        assert!(!text.contains(BACKUP_FILE_NAME), "a newer file's backup isn't an answer: {text}");
        assert!(text.contains("/logs"), "{text}");
        // With no known paths it still says something useful.
        assert!(startup_failure_text(&store::Error::NoDataDir, None, None).contains("couldn't open"));
    }

    #[test]
    fn the_backup_name_matches_the_stores() {
        let store_src = include_str!("../../../crates/store/src/lib.rs");
        assert!(store_src.contains(&format!("const BACKUP_FILE: &str = \"{BACKUP_FILE_NAME}\";")));
    }

    #[test]
    fn popups_are_on_unless_turned_off() {
        assert!(popups_enabled(None));
        assert!(popups_enabled(Some("on")));
        assert!(popups_enabled(Some("anything")));
        assert!(!popups_enabled(Some("off")));
    }

    #[test]
    fn the_placement_is_saved_in_points() {
        assert_eq!(placement_string((200, 100), (2400, 1800), 2.0), "100,50,1200,900");
        let saved = placement_string((-30, 50), (1085, 900), 1.0);
        assert_eq!(Rect::parse(&saved), Some(Rect { x: -30.0, y: 50.0, w: 1085.0, h: 900.0 }));
    }

    #[test]
    fn the_home_folder_is_cut_from_any_text_that_quotes_a_path() {
        let home = Some(Path::new("/Users/me"));
        assert_eq!(shorten_home_in("can't read /Users/me/chat.jsonl: denied", home), "can't read ~/chat.jsonl: denied");
        assert_eq!(shorten_home_in("/Users/me and /Users/me/x", home), "~ and ~/x");
        assert_eq!(shorten_home_in("/Users/mellow/x /Users/me-too", home), "/Users/mellow/x /Users/me-too");
        assert_eq!(shorten_home_in("/Users/me/a", None), "/Users/me/a");
    }
}
