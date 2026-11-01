//! Everything the windows can ask the Rust side to do.
//!
//! Commands marked `#[tauri::command(async)]` write through `App::writer`, which a sync pass
//! holds for its whole length. They run off the main thread so waiting for it never freezes
//! the window. Commands that only read use `App::reader` and return at once.

use crate::updater::{self, UpdateStatus, Updates};
use crate::{lock, tracker::Msg, App, SyncStatus};
use chrono::{Local, NaiveDate};
use serde::Serialize;
use sources::{Kind, Source};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use store::{Filter, Goal, GoalKind, Meta, Progress, RecentReportCard, Retention, SourceState, Stats};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_dialog::{DialogExt, FileDialogBuilder, FilePath};
use tauri_plugin_notification::{NotificationExt, PermissionState};
use tauri_plugin_opener::OpenerExt;

type Res<T> = Result<T, String>;

/// Errors a command returned lately, so [`log_error`] doesn't write the same one again when the
/// window reports what it got back.
static RECENT_ERRORS: Mutex<Vec<(Instant, String)>> = Mutex::new(Vec::new());
/// How long a returned error counts as already logged.
const RECENT_ERROR_WINDOW: Duration = Duration::from_secs(30);
/// How many returned errors are remembered.
const RECENT_ERRORS_KEPT: usize = 16;

/// Remembers `message` as returned to a window at `now`, dropping old ones.
fn remember_error(recent: &mut Vec<(Instant, String)>, message: &str, now: Instant) {
    recent.retain(|(at, _)| now.saturating_duration_since(*at) < RECENT_ERROR_WINDOW);
    if recent.len() >= RECENT_ERRORS_KEPT {
        recent.remove(0);
    }
    recent.push((now, message.to_string()));
}

/// Whether a window's report is (or contains) an error a command returned a moment ago.
fn already_logged(recent: &[(Instant, String)], report: &str, now: Instant) -> bool {
    recent.iter().any(|(at, m)| {
        now.saturating_duration_since(*at) < RECENT_ERROR_WINDOW && !m.is_empty() && report.contains(m.as_str())
    })
}

/// A real failure, logged once as a warning and passed on. The window may report it again through
/// [`log_error`], which then stays quiet.
fn err(e: impl std::fmt::Display) -> String {
    let message = e.to_string();
    log::warn!("command failed: {message}");
    remember_error(&mut crate::lock(&RECENT_ERRORS), &message, Instant::now());
    message
}

/// An expected "no" (a wrong window, a file that isn't an export): passed on for the window to show,
/// and not logged at all, as neither warning nor error.
fn refused(message: &str) -> String {
    remember_error(&mut crate::lock(&RECENT_ERRORS), message, Instant::now());
    message.to_string()
}

/// Fails unless `label` is the scorecard window. What changes consent or deletes data is done only
/// there: the small window under the menu bar icon never does.
fn only_main(label: &str) -> Res<()> {
    if label == "main" {
        Ok(())
    } else {
        Err(refused("That can only be done from the main window."))
    }
}

fn changed(app: &App) {
    let _ = app.tracker.send(Msg::SourcesChanged);
}

/// The adapter for `id`, or an error the window can show.
fn source<'a>(app: &'a App, id: &str) -> Res<&'a dyn Source> {
    sources::find(&app.sources, id).map_err(err)
}

/// What the consent screen shows for one source.
#[derive(Serialize)]
pub struct SourceListing {
    id: &'static str,
    name: &'static str,
    kind: &'static str,
    /// Folders a live source would read, and whether each exists on this machine.
    roots: Vec<(String, bool)>,
    state: Option<SourceState>,
}

/// Tiers, categories and sources, so the UI never hard-codes Rust's tables.
#[tauri::command]
pub fn meta(app: State<Arc<App>>) -> Meta {
    store::meta(&app.sources)
}

#[tauri::command(async)]
pub fn stats(app: State<Arc<App>>, filter: Filter) -> Res<Stats> {
    lock(&app.reader).stats(&filter, Local::now().date_naive()).map_err(err)
}

/// The Improve tab: history for the filter, plus your goal, trophies and bests. Off the main
/// thread: it reads every week of history.
#[tauri::command(async)]
pub fn progress(app: State<Arc<App>>, filter: Filter) -> Res<Progress> {
    lock(&app.reader).progress(&filter, Local::now().date_naive()).map_err(err)
}

/// The latest report cards under the model filter, newest first, for Improve's recent list.
#[tauri::command(async)]
pub fn recent_report_cards(app: State<Arc<App>>, filter: Filter) -> Res<Vec<RecentReportCard>> {
    lock(&app.reader).recent_report_cards(&filter, store::RECENT_REPORT_CARDS).map_err(err)
}

#[tauri::command(async)]
pub fn set_goal(app: State<Arc<App>>, kind: GoalKind, target: i64) -> Res<Goal> {
    let now = chrono::Local::now();
    lock(&app.writer).set_goal(kind, target, now.date_naive(), now.timestamp_millis()).map_err(err)
}

#[tauri::command(async)]
pub fn clear_goal(app: State<Arc<App>>) -> Res<()> {
    lock(&app.writer).clear_goal().map_err(err)
}

/// Longest draft the prompt checker grades. The engine reads the first 4,000 characters anyway;
/// this just keeps a giant paste from crossing the bridge.
const CHECK_MAX_BYTES: usize = 64 * 1024;

/// Grades a draft prompt with the same rules scoring uses. Nothing is stored or logged.
#[tauri::command]
pub fn check_prompt(text: String) -> engine::Check {
    let mut end = text.len().min(CHECK_MAX_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    engine::check(&text[..end])
}

#[tauri::command]
pub fn learned(app: State<Arc<App>>) -> Res<Vec<String>> {
    lock(&app.reader).learned().map_err(err)
}

#[tauri::command(async)]
pub fn set_learned(app: State<Arc<App>>, id: String, done: bool) -> Res<Vec<String>> {
    lock(&app.writer).set_learned(&id, done).map_err(err)
}

/// The menu bar setting, and whether this system could build the icon at all.
#[derive(Serialize)]
pub struct MenuBar {
    /// The saved choice: keep running in the menu bar (macOS) or system tray after the window
    /// closes. False means an ordinary app that quits.
    enabled: bool,
    /// False when the tray failed to build, so the choice has no effect here.
    available: bool,
}

#[tauri::command]
pub fn get_menu_bar(app: State<Arc<App>>, tray: State<crate::HasTray>) -> MenuBar {
    MenuBar { enabled: app.menu_bar(), available: tray.0 }
}

#[tauri::command(async)]
pub fn set_menu_bar(handle: AppHandle, app: State<Arc<App>>, tray: State<crate::HasTray>, enabled: bool) -> Res<()> {
    if !tray.0 {
        return Ok(()); // no icon to show or hide; closing the window quits either way
    }
    lock(&app.writer).set_setting(crate::MENU_BAR_KEY, if enabled { "on" } else { "off" }).map_err(err)?;
    crate::apply_menu_bar(&handle, &app, enabled);
    Ok(())
}

#[tauri::command(async)]
pub fn list_sources(app: State<Arc<App>>) -> Res<Vec<SourceListing>> {
    let states = lock(&app.reader).source_states().map_err(err)?;
    Ok(app
        .sources
        .iter()
        .map(|s| SourceListing {
            id: s.id(),
            name: s.name(),
            kind: if s.kind() == Kind::Live { "live" } else { "import" },
            roots: s.roots().iter().map(|r| (r.display().to_string(), r.exists())).collect(),
            state: states.iter().find(|x| x.id == s.id()).cloned(),
        })
        .collect())
}

#[tauri::command(async)]
pub fn allow_source(
    window: tauri::Window,
    handle: AppHandle,
    app: State<Arc<App>>,
    id: String,
    since: Option<NaiveDate>,
    from_now: Option<bool>,
) -> Res<()> {
    only_main(window.label())?;
    let src = source(&app, &id)?;
    {
        let mut store = lock(&app.writer);
        store.allow(&id, since).map_err(err)?;
        // "From now on": everything already on disk is treated as read.
        if from_now == Some(true) {
            store.start_from_now(src).map_err(err)?;
        }
    }
    log::info!("allowed source {id} (since {since:?})");
    changed(&app);
    // Ask for notifications now, in context, rather than out of nowhere at the first milestone.
    if handle.notification().permission_state().is_ok_and(|s| s != PermissionState::Granted) {
        let _ = handle.notification().request_permission();
    }
    Ok(())
}

#[tauri::command(async)]
pub fn revoke_source(window: tauri::Window, app: State<Arc<App>>, id: String) -> Res<()> {
    only_main(window.label())?;
    source(&app, &id)?;
    lock(&app.writer).revoke(&id).map_err(err)?;
    changed(&app);
    Ok(())
}

#[tauri::command(async)]
pub fn forget_source(window: tauri::Window, app: State<Arc<App>>, id: String) -> Res<()> {
    only_main(window.label())?;
    source(&app, &id)?;
    lock(&app.writer).forget(&id).map_err(err)?;
    log::info!("forgot source {id}");
    changed(&app);
    Ok(())
}

#[tauri::command(async)]
pub fn pause_source(window: tauri::Window, app: State<Arc<App>>, id: String, paused: bool) -> Res<()> {
    only_main(window.label())?;
    source(&app, &id)?;
    lock(&app.writer).set_paused(&id, paused).map_err(err)?;
    changed(&app);
    Ok(())
}

/// Shows a native file dialog without blocking the async runtime; `None` if it was cancelled.
async fn pick(dialog: FileDialogBuilder<tauri::Wry>, save: bool) -> Option<FilePath> {
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    let done = move |p: Option<FilePath>| {
        let _ = tx.try_send(p);
    };
    if save {
        dialog.save_file(done);
    } else {
        dialog.pick_file(done);
    }
    rx.recv().await.flatten()
}

/// Checks that `path` reads as an export of `src`, the way a sync would read it, so a wrong file is
/// refused now and not skipped silently on every pass. An error the window can show.
fn check_export(src: &dyn Source, path: &Path) -> Res<()> {
    match src.scan(path, 0, true) {
        Ok(_) => Ok(()),
        Err(e) => {
            log::info!("import refused: {}", crate::shorten_home_in(&e.to_string(), std::env::home_dir().as_deref()));
            Err(refused(&if matches!(e, sources::Error::TooBig { .. }) {
                "That file is over the 1 GB limit for an export.".to_string()
            } else {
                format!("That doesn't look like a {} export. Choose its conversations.json.", src.name())
            }))
        }
    }
}

/// Opens a native file picker for an export (ChatGPT / Claude.ai `conversations.json`), then
/// imports it: the path is added and the source allowed, same as `noactually import`. A file the
/// source can't read as an export is refused and not added.
#[tauri::command]
pub async fn import_export(
    window: tauri::Window,
    handle: AppHandle,
    app: State<'_, Arc<App>>,
    id: String,
) -> Res<Option<String>> {
    only_main(window.label())?;
    source(&app, &id)?;
    let dialog = handle.dialog().file().set_title("Choose your conversations.json").add_filter("Export", &["json"]);
    let Some(file) = pick(dialog, false).await else { return Ok(None) };
    let path = file.into_path().map_err(err)?.to_string_lossy().to_string();
    // Reading the file and waiting for the writer (a sync may hold it) both happen off the async runtime.
    let (writer, import_path) = (Arc::clone(&app), path.clone());
    tauri::async_runtime::spawn_blocking(move || -> Res<()> {
        let src = sources::find(&writer.sources, &id).map_err(err)?;
        check_export(src, Path::new(&import_path))?;
        lock(&writer.writer).import(&id, &import_path).map_err(err)
    })
    .await
    .map_err(err)??;
    changed(&app);
    Ok(Some(path))
}

#[tauri::command]
pub fn get_retention(app: State<Arc<App>>) -> Res<Retention> {
    lock(&app.reader).retention().map_err(err)
}

#[tauri::command(async)]
pub fn set_retention(window: tauri::Window, app: State<Arc<App>>, retention: Retention) -> Res<()> {
    only_main(window.label())?;
    retention.validate().map_err(|e| refused(&e.to_string()))?;
    lock(&app.writer).set_retention(retention).map_err(err)
}

#[tauri::command(async)]
pub fn pin(app: State<Arc<App>>, id: i64, pinned: bool) -> Res<()> {
    lock(&app.writer).pin(id, pinned).map_err(err)
}

#[tauri::command]
pub fn sync_now(app: State<Arc<App>>) {
    let _ = app.tracker.send(Msg::SyncNow);
}

#[tauri::command]
pub fn sync_status(app: State<Arc<App>>) -> SyncStatus {
    lock(&app.status).clone()
}

/// Shows the scorecard window, on `tab` when given (the main window checks it's a real tab).
#[tauri::command]
pub fn open_main(handle: AppHandle, tab: Option<String>) {
    crate::show_main(&handle);
    if let Some(tab) = tab {
        let _ = handle.emit_to("main", "open-tab", tab);
    }
}

#[tauri::command]
pub fn get_autostart(handle: AppHandle) -> Res<bool> {
    handle.autolaunch().is_enabled().map_err(err)
}

#[tauri::command]
pub fn set_autostart(handle: AppHandle, enabled: bool) -> Res<()> {
    let al = handle.autolaunch();
    if enabled { al.enable() } else { al.disable() }.map_err(err)
}

/// Every PNG file starts with these 8 bytes.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];

/// True if `bytes` start like a PNG file.
pub fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(&PNG_SIGNATURE)
}

/// Biggest share card accepted. A real one is a few hundred KB.
const SHARE_CARD_MAX_BYTES: usize = 10 * 1024 * 1024;

/// Saves the share card the window drew. The user picks where; nothing is uploaded.
#[tauri::command]
pub async fn save_share_card(handle: AppHandle, png: Vec<u8>) -> Res<Option<String>> {
    if !is_png(&png) {
        return Err(refused("That isn't a PNG image."));
    }
    if png.len() > SHARE_CARD_MAX_BYTES {
        return Err(refused("That image is too big to be a scorecard."));
    }
    let dialog = handle
        .dialog()
        .file()
        .set_title("Save your scorecard")
        .set_file_name("no-actually-scorecard.png")
        .add_filter("PNG image", &["png"]);
    let Some(dest) = pick(dialog, true).await else { return Ok(None) };
    let path = dest.into_path().map_err(err)?;
    std::fs::write(&path, png).map_err(err)?;
    Ok(Some(path.to_string_lossy().to_string()))
}

/// Sites a share button may open. Keep in sync with `SHARE_HOSTS` in app/src/share/share.ts.
const SHARE_HOSTS: &[&str] = &[
    "x.com",
    "www.linkedin.com",
    "www.facebook.com",
    "www.threads.net",
    "bsky.app",
    "www.reddit.com",
    "wa.me",
    "github.com",
];

/// True for an https link to one of the share sites, and nothing else.
pub fn is_share_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    SHARE_HOSTS.contains(&host)
}

/// Opens a network's share page in the person's browser. The share text is in the URL, so it reaches
/// the site as soon as the page opens (numbers and the project link, and for a correction its quote
/// too), before anything is posted. Nothing is posted until they do it themselves.
#[tauri::command]
pub fn open_share(handle: AppHandle, url: String) -> Res<()> {
    if !is_share_url(&url) {
        return Err(refused("That link isn't a share link."));
    }
    handle.opener().open_url(url, None::<&str>).map_err(err)
}

/// Longest window error kept in the log, so one runaway message can't fill it.
const LOG_MESSAGE_MAX_CHARS: usize = 2000;

/// The first [`LOG_MESSAGE_MAX_CHARS`] characters of `message`.
fn cap_log_message(message: &str) -> String {
    message.chars().take(LOG_MESSAGE_MAX_CHARS).collect()
}

/// Records an error from a window in the app log, where a release build would otherwise lose it.
#[tauri::command]
pub fn log_error(message: String) {
    // A command that failed already logged its error; the window reporting it back isn't a second one.
    if already_logged(&crate::lock(&RECENT_ERRORS), &message, Instant::now()) {
        return;
    }
    log::error!("window: {}", cap_log_message(&message));
}

/// The notices file's name in the bundle's resources (see `bundle.resources` in `tauri.conf.json`).
const NOTICES_FILE: &str = "THIRD_PARTY_NOTICES.md";

/// Opens the folder the app log lives in, so someone can attach it to a bug report.
#[tauri::command]
pub fn open_log_folder(window: tauri::Window, handle: AppHandle) -> Res<()> {
    only_main(window.label())?;
    let dir = handle.path().app_log_dir().map_err(err)?;
    std::fs::create_dir_all(&dir).map_err(err)?;
    handle.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(err)
}

/// Opens the third-party notices file that ships with the app, so a person can read the licenses.
#[tauri::command]
pub fn open_notices(window: tauri::Window, handle: AppHandle) -> Res<()> {
    only_main(window.label())?;
    let file = handle.path().resolve(NOTICES_FILE, tauri::path::BaseDirectory::Resource).map_err(err)?;
    if !file.exists() {
        // A development run has no bundle to read it from.
        return Err(refused("The notices file isn't part of this build."));
    }
    handle.opener().open_path(file.to_string_lossy(), None::<&str>).map_err(err)
}

/// Whether popups (milestones and the rescored notice) are on.
#[tauri::command]
pub fn get_popups(app: State<Arc<App>>) -> bool {
    crate::popups_enabled(lock(&app.reader).get_setting(crate::POPUPS_KEY).ok().flatten().as_deref())
}

#[tauri::command(async)]
pub fn set_popups(app: State<Arc<App>>, enabled: bool) -> Res<()> {
    lock(&app.writer).set_setting(crate::POPUPS_KEY, if enabled { "on" } else { "off" }).map_err(err)
}

/// When scores were last rebuilt under new rules (an RFC 3339 time), or `None` if they never were.
/// Kept even with popups off, so the window can show the notice.
#[tauri::command]
pub fn rescored_notice(app: State<Arc<App>>) -> Res<Option<String>> {
    lock(&app.reader).get_setting(crate::RESCORED_NOTICE_KEY).map_err(err)
}

#[tauri::command]
pub fn update_status(app: State<Arc<App>>, updates: State<Updates>) -> UpdateStatus {
    updates.status(&app)
}

#[tauri::command(async)]
pub fn set_auto_update(app: State<Arc<App>>, updates: State<Updates>, enabled: bool) -> Res<()> {
    lock(&app.writer).set_setting(updater::AUTO_KEY, if enabled { "on" } else { "off" }).map_err(err)?;
    if enabled {
        let _ = updates.tx.send(updater::Msg::CheckNow);
    }
    Ok(())
}

/// Switches between stable and beta, and looks on the new channel straight away.
#[tauri::command(async)]
pub fn set_update_channel(
    handle: AppHandle,
    app: State<Arc<App>>,
    updates: State<Updates>,
    channel: String,
) -> Res<()> {
    if channel != updater::STABLE && channel != updater::BETA {
        return Err(refused("That isn't an update channel."));
    }
    lock(&app.writer).set_setting(updater::CHANNEL_KEY, &channel).map_err(err)?;
    updates.forget_offer(&handle);
    // With automatic checks off, only "Check for app updates" may reach the network.
    if updates.status(&app).auto {
        let _ = updates.tx.send(updater::Msg::CheckNow);
    }
    Ok(())
}

#[tauri::command]
pub fn check_for_updates(updates: State<Updates>) {
    let _ = updates.tx.send(updater::Msg::CheckNow);
}

/// Downloads and installs the waiting update, then restarts the app.
#[tauri::command]
pub async fn install_update(handle: AppHandle) -> Res<()> {
    updater::install(handle).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_share_sites_open() {
        assert!(is_share_url("https://x.com/intent/post?text=hi"));
        assert!(is_share_url("https://wa.me/?text=hi"));
        assert!(!is_share_url("http://x.com/intent/post"), "https only");
        assert!(!is_share_url("https://x.com.evil.test/"), "exact host");
        assert!(!is_share_url("https://evil.test/?x.com"));
        assert!(!is_share_url("file:///etc/passwd"));
    }

    #[test]
    fn the_share_hosts_here_and_in_the_window_are_the_same_list() {
        let ts = include_str!("../../src/share/share.ts");
        let list = ts.split("export const SHARE_HOSTS = [").nth(1).and_then(|rest| rest.split(']').next()).unwrap();
        let hosts: Vec<&str> = list.split('"').skip(1).step_by(2).collect();
        assert_eq!(hosts, SHARE_HOSTS, "update both lists together");
    }

    #[test]
    fn only_the_scorecard_window_changes_consent() {
        assert!(only_main("main").is_ok());
        assert!(only_main("glance").is_err());
        assert!(only_main("").is_err());
    }

    #[test]
    fn the_checker_cuts_giant_drafts_on_a_char_boundary() {
        let huge = "é".repeat(CHECK_MAX_BYTES); // two bytes each: the cut lands mid-character
        assert_eq!(check_prompt(huge).grade, engine::Grade::Thin);
        assert_eq!(check_prompt("Fix `a.ts`. Done when tests pass. Don't touch b.".into()).grade, engine::Grade::Sharp);
    }

    #[test]
    fn window_errors_are_capped_in_the_log() {
        assert_eq!(cap_log_message("short"), "short");
        let long = "é".repeat(LOG_MESSAGE_MAX_CHARS + 50);
        assert_eq!(cap_log_message(&long).chars().count(), LOG_MESSAGE_MAX_CHARS);
    }

    #[test]
    fn a_returned_error_is_not_logged_a_second_time() {
        let t = Instant::now();
        let mut recent = Vec::new();
        remember_error(&mut recent, "That link isn't a share link.", t);
        let report = "invoke failed: That link isn't a share link.";
        assert!(already_logged(&recent, report, t + Duration::from_secs(1)));
        assert!(!already_logged(&recent, "TypeError: x is undefined", t));
        assert!(!already_logged(&recent, report, t + RECENT_ERROR_WINDOW), "old ones are forgotten");
        assert!(!already_logged(&[(t, String::new())], "anything", t), "an empty message matches nothing");
    }

    #[test]
    fn remembered_errors_are_capped() {
        let t = Instant::now();
        let mut recent = Vec::new();
        for i in 0..RECENT_ERRORS_KEPT + 5 {
            remember_error(&mut recent, &format!("error {i}"), t);
        }
        assert_eq!(recent.len(), RECENT_ERRORS_KEPT);
        assert_eq!(recent[0].1, "error 5");
    }

    fn export_source(id: &str) -> &'static dyn Source {
        static ALL: std::sync::OnceLock<Vec<Box<dyn Source>>> = std::sync::OnceLock::new();
        sources::find(ALL.get_or_init(sources::all), id).unwrap()
    }

    #[test]
    fn a_file_that_is_not_an_export_is_refused_with_a_plain_error() {
        let dir = tempfile::tempdir().unwrap();
        for id in ["chatgpt-export", "claude-ai-export"] {
            let src = export_source(id);
            let write = |name: &str, text: &str| {
                let p = dir.path().join(name);
                std::fs::write(&p, text).unwrap();
                p
            };
            assert!(check_export(src, &write("ok.json", "[]")).is_ok(), "{id}: an empty export is still an export");
            for bad in ["not json at all", "{\"conversations\": []}", ""] {
                let err = check_export(src, &write("bad.json", bad)).unwrap_err();
                assert!(
                    err.starts_with("That doesn't look like a ") && err.contains("conversations.json"),
                    "{id}: {err}"
                );
                assert!(!err.contains(dir.path().to_str().unwrap()), "no path in the message: {err}");
            }
            let missing = check_export(src, &dir.path().join("gone.json")).unwrap_err();
            assert!(missing.starts_with("That doesn't look like a "), "{missing}");
        }
    }

    #[test]
    fn the_notices_and_license_ship_in_the_bundle() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let resources = &conf["bundle"]["resources"];
        assert_eq!(resources["../../THIRD_PARTY_NOTICES.md"], NOTICES_FILE);
        assert_eq!(resources["../../LICENSE"], "LICENSE");
        assert!(include_str!("../../../THIRD_PARTY_NOTICES.md").contains("Third-party notices"));
        assert!(include_str!("../../../LICENSE").contains("MIT"));
    }

    #[test]
    fn the_long_description_tells_the_truth_about_what_reaches_github() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let text = conf["bundle"]["longDescription"].as_str().unwrap();
        assert!(text.contains("checks GitHub for updates"), "{text}");
        assert!(text.contains("report cards") && text.contains("plugin version"), "{text}");
    }

    #[test]
    fn png_needs_the_whole_signature() {
        assert!(is_png(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"));
        assert!(!is_png(b"xPNG\r\n\x1a\n"), "first byte matters");
        assert!(!is_png(b"\x89PNG"), "too short");
        assert!(!is_png(b"<svg>PNG</svg>"));
    }

    #[test]
    fn bundle_id_matches_the_shared_database_folder() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["identifier"], store::APP_ID, "the CLI and app must open the same database");
    }

    /// The browser demo (app/public/demo.json) stands in for these commands, so it must have
    /// their exact shape. This fails if Rust's output changes and the demo isn't updated.
    #[test]
    fn demo_data_matches_what_rust_sends() {
        let demo: serde_json::Value = serde_json::from_str(include_str!("../../public/demo.json")).unwrap();
        let meta = serde_json::to_value(store::meta(&sources::all())).unwrap();
        assert_eq!(demo["meta"], meta, "demo.json `meta` differs from the `meta` command");

        let db = store::Store::open_in_memory().unwrap();
        let stats = serde_json::to_value(db.stats(&Filter::default(), Local::now().date_naive()).unwrap()).unwrap();
        let keys = |v: &serde_json::Value| -> Vec<String> {
            let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
            k.sort();
            k
        };
        assert_eq!(keys(&demo["stats"]), keys(&stats), "demo.json `stats` keys differ from `Stats`");
        let rows = [
            ("hall_of_fame", serde_json::to_value(store::HallEntry::default()).unwrap()),
            ("models", serde_json::to_value(store::ModelRow::default()).unwrap()),
            ("weekly", serde_json::to_value(store::WeekRow::default()).unwrap()),
        ];
        let prompts =
            serde_json::to_value(store::Prompts { tip: Some(store::Tip::default()), ..Default::default() }).unwrap();
        assert_eq!(keys(&demo["stats"]["prompts"]), keys(&prompts), "demo.json `prompts` keys differ from `Prompts`");
        assert_eq!(
            keys(&demo["stats"]["prompts"]["tip"]),
            keys(&prompts["tip"]),
            "demo.json `prompts.tip` keys differ"
        );
        assert_eq!(keys(&demo["stats"]["overall"]), keys(&stats["overall"]), "demo.json `overall` keys differ");
        let progress =
            serde_json::to_value(db.progress(&Filter::default(), Local::now().date_naive()).unwrap()).unwrap();
        assert_eq!(keys(&demo["progress"]), keys(&progress), "demo.json `progress` keys differ from `Progress`");
        let week = serde_json::to_value(store::HistoryWeek::default()).unwrap();
        for w in demo["progress"]["weeks"].as_array().unwrap() {
            assert_eq!(keys(w), keys(&week), "a demo.json `progress.weeks` row has different keys");
            assert_eq!(
                keys(&w["report_cards"]),
                keys(&week["report_cards"]),
                "a demo.json week's `report_cards` differ"
            );
        }
        assert!(demo["progress"]["goal"].is_object(), "the demo shows a goal under way");
        assert_eq!(keys(&demo["progress"]["bests"]), keys(&progress["bests"]), "demo.json `bests` keys differ");
        assert_eq!(
            keys(&demo["progress"]["insights"]),
            keys(&progress["insights"]),
            "demo.json `insights` keys differ"
        );
        // The report card pieces show in the demo: some weeks graded (one unpriced), the best week, the trend and the recent list.
        let cards: Vec<_> = demo["progress"]["weeks"].as_array().unwrap().iter().map(|w| &w["report_cards"]).collect();
        assert!(cards.iter().any(|c| c["graded"].as_i64() > Some(0)), "the demo has report cards");
        assert!(
            cards.iter().any(|c| c["graded"].as_i64() > Some(0) && c["extra_cost"].is_null()),
            "and an unpriced week"
        );
        assert!(demo["progress"]["bests"]["report_card"].is_array(), "the demo shows a best report card week");
        let trend = serde_json::to_value(store::CardTrend {
            first_ab: 0,
            last_ab: 0,
            habit: Some(store::HabitDrop { habit: String::new(), first: 0, last: 0 }),
        })
        .unwrap();
        let shown = &demo["progress"]["insights"]["report_cards"];
        assert_eq!(keys(shown), keys(&trend), "demo.json `insights.report_cards` keys differ");
        assert_eq!(keys(&shown["habit"]), keys(&trend["habit"]), "demo.json `insights.report_cards.habit` keys differ");
        let recent = serde_json::to_value(store::RecentReportCard {
            day: String::new(),
            graded_day: chrono::NaiveDate::default(),
            source: String::new(),
            grade: String::new(),
            missing: Some(String::new()),
            extra_cost: Some(0.0),
            extra_tokens: 0,
            extra_minutes: 0,
        })
        .unwrap();
        let recents = demo["recent_report_cards"].as_array().unwrap();
        assert!(!recents.is_empty(), "the demo lists recent report cards");
        for r in recents {
            assert_eq!(keys(r), keys(&recent), "a demo.json `recent_report_cards` row has different keys");
        }
        for (field, row) in rows {
            let demo_rows = demo["stats"][field].as_array().unwrap();
            assert!(!demo_rows.is_empty(), "demo.json `{field}` is empty, so nothing is checked");
            for r in demo_rows {
                assert_eq!(keys(r), keys(&row), "a demo.json `{field}` row has different keys");
            }
        }
    }
}
