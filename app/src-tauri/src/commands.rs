//! Everything the windows can ask the Rust side to do.

use crate::{lock, tracker::Msg, App, SyncStatus};
use chrono::{Local, NaiveDate};
use serde::Serialize;
use sources::Kind;
use std::sync::Arc;
use store::{Filter, Retention, SourceState, Stats};
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::{NotificationExt, PermissionState};
use tauri_plugin_opener::OpenerExt;

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    log::warn!("command failed: {e}");
    e.to_string()
}

fn changed(app: &App) {
    let _ = app.tracker.send(Msg::SourcesChanged);
}

#[derive(Serialize)]
pub struct SourceInfo {
    id: &'static str,
    name: &'static str,
    kind: &'static str,
    /// Folders a live source would read, and whether each exists on this machine.
    roots: Vec<(String, bool)>,
    state: Option<SourceState>,
}

#[tauri::command]
pub fn stats(app: State<Arc<App>>, filter: Filter) -> Res<Stats> {
    lock(&app.reader).stats(&filter, Local::now().date_naive()).map_err(err)
}

#[tauri::command]
pub fn list_sources(app: State<Arc<App>>) -> Res<Vec<SourceInfo>> {
    let states = lock(&app.reader).source_states().map_err(err)?;
    Ok(app
        .sources
        .iter()
        .map(|s| SourceInfo {
            id: s.id(),
            name: s.name(),
            kind: if s.kind() == Kind::Live { "live" } else { "import" },
            roots: s.roots().iter().map(|r| (r.display().to_string(), r.exists())).collect(),
            state: states.iter().find(|x| x.id == s.id()).cloned(),
        })
        .collect())
}

/// Async: it may wait for a sync to finish, and must not freeze the window meanwhile.
#[tauri::command(async)]
pub fn allow_source(
    handle: AppHandle,
    app: State<Arc<App>>,
    id: String,
    since: Option<NaiveDate>,
    from_now: Option<bool>,
) -> Res<()> {
    {
        let mut store = lock(&app.writer);
        store.allow(&id, since).map_err(err)?;
        // "From now on": everything already on disk is treated as read.
        if from_now == Some(true) {
            if let Some(src) = app.sources.iter().find(|s| s.id() == id) {
                store.start_from_now(src.as_ref()).map_err(err)?;
            }
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

/// Async: it may wait for a sync to finish, and must not freeze the window meanwhile.
#[tauri::command(async)]
pub fn revoke_source(app: State<Arc<App>>, id: String) -> Res<()> {
    lock(&app.writer).revoke(&id).map_err(err)?;
    changed(&app);
    Ok(())
}

/// Async: it may wait for a sync to finish, and must not freeze the window meanwhile.
#[tauri::command(async)]
pub fn forget_source(app: State<Arc<App>>, id: String) -> Res<()> {
    lock(&app.writer).forget(&id).map_err(err)?;
    log::info!("forgot source {id}");
    changed(&app);
    Ok(())
}

/// Async: it may wait for a sync to finish, and must not freeze the window meanwhile.
#[tauri::command(async)]
pub fn pause_source(app: State<Arc<App>>, id: String, paused: bool) -> Res<()> {
    lock(&app.writer).set_paused(&id, paused).map_err(err)?;
    changed(&app);
    Ok(())
}

/// Opens a native file picker for an export (ChatGPT / Claude.ai `conversations.json`).
#[tauri::command]
pub async fn import_export(handle: AppHandle, app: State<'_, Arc<App>>, id: String) -> Res<Option<String>> {
    let Some(file) = handle
        .dialog()
        .file()
        .set_title("Choose your conversations.json")
        .add_filter("Export", &["json"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let path = file.into_path().map_err(err)?.to_string_lossy().to_string();
    {
        let store = lock(&app.writer);
        store.add_path(&id, &path).map_err(err)?;
        store.allow(&id, None).map_err(err)?;
    }
    changed(&app);
    Ok(Some(path))
}

#[tauri::command]
pub fn get_retention(app: State<Arc<App>>) -> Res<Retention> {
    lock(&app.reader).retention().map_err(err)
}

/// Async: it may wait for a sync to finish, and must not freeze the window meanwhile.
#[tauri::command(async)]
pub fn set_retention(app: State<Arc<App>>, retention: Retention) -> Res<()> {
    if retention.full_days == 0 || retention.snippet_days == 0 {
        return Err("Keep text for at least 1 day.".into());
    }
    lock(&app.writer).set_retention(retention).map_err(err)
}

/// Async: it may wait for a sync to finish, and must not freeze the window meanwhile.
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

#[tauri::command]
pub fn open_main(handle: AppHandle) {
    crate::show_main(&handle);
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

/// Saves the share card the window drew. The user picks where; nothing is uploaded.
#[tauri::command]
pub async fn save_share_card(handle: AppHandle, png: Vec<u8>) -> Res<Option<String>> {
    if png.len() < 8 || &png[1..4] != b"PNG" {
        return Err("That isn't a PNG image.".into());
    }
    let Some(dest) = handle
        .dialog()
        .file()
        .set_title("Save your scorecard")
        .set_file_name("no-actually-scorecard.png")
        .add_filter("PNG image", &["png"])
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let path = dest.into_path().map_err(err)?;
    std::fs::write(&path, png).map_err(err)?;
    Ok(Some(path.to_string_lossy().to_string()))
}

/// Sites a share button may open. Keep in sync with SHARE_HOSTS in app/src/share.ts.
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

/// Opens a network's share page in the person's browser. Only the share text (numbers and the
/// project link) is in the URL; nothing is sent until they post it themselves.
#[tauri::command]
pub fn open_share(handle: AppHandle, url: String) -> Res<()> {
    if !is_share_url(&url) {
        return Err("That link isn't a share link.".into());
    }
    handle.opener().open_url(url, None::<&str>).map_err(err)
}

#[cfg(test)]
mod tests {
    use super::is_share_url;

    #[test]
    fn only_share_sites_open() {
        assert!(is_share_url("https://x.com/intent/post?text=hi"));
        assert!(is_share_url("https://wa.me/?text=hi"));
        assert!(!is_share_url("http://x.com/intent/post"), "https only");
        assert!(!is_share_url("https://x.com.evil.test/"), "exact host");
        assert!(!is_share_url("https://evil.test/?x.com"));
        assert!(!is_share_url("file:///etc/passwd"));
    }
}
