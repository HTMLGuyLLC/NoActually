//! Background tracking: one thread owns syncing. File events and a timer only ever ask it
//! to sync; the timer is the safety net for anything the OS watcher misses.

use crate::{lock, milestones, App};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use sources::Kind;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub enum Msg {
    SyncNow,
    FilesChanged,
    SourcesChanged,
}

const SAFETY_NET: Duration = Duration::from_secs(5 * 60);
const DEBOUNCE: Duration = Duration::from_secs(2);
/// More new messages than a person writes between two checks means history was just read.
const BACKFILL_MESSAGES: usize = 40;

/// Folders to watch: only the roots of sources the user has allowed.
fn watched_roots(app: &App) -> Vec<PathBuf> {
    let Ok(states) = lock(&app.reader).source_states() else { return vec![] };
    let mut roots = Vec::new();
    for st in states.iter().filter(|s| s.allowed && !s.paused) {
        if let Some(src) = app.sources.iter().find(|s| s.id() == st.id) {
            if src.kind() == Kind::Live {
                roots.extend(src.roots().into_iter().filter(|r| r.exists()));
            }
        }
    }
    roots
}

fn watch(app: &App, tx: &mpsc::Sender<Msg>) -> Option<RecommendedWatcher> {
    let tx = tx.clone();
    let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if res.is_ok_and(|e| e.kind.is_create() || e.kind.is_modify()) {
            let _ = tx.send(Msg::FilesChanged);
        }
    })
    .map_err(|e| log::warn!("file watcher unavailable, relying on the timer: {e}"))
    .ok()?;
    for root in watched_roots(app) {
        if let Err(e) = w.watch(&root, RecursiveMode::Recursive) {
            log::warn!("can't watch {}: {e}", root.display());
        }
    }
    Some(w)
}

/// `quiet`: record milestones without announcing them (a backfill, or a source just allowed).
fn sync_once(handle: &AppHandle, app: &App, quiet: bool) {
    lock(&app.status).syncing = true;
    let _ = handle.emit("sync-started", ());
    let started = std::time::Instant::now();
    let report = {
        let mut store = lock(&app.writer);
        store::sync(&mut store, &app.sources)
    };
    let mut status = lock(&app.status);
    status.syncing = false;
    status.last_sync = Some(chrono::Utc::now());
    match report {
        Ok(r) => {
            if r.files_read > 0 {
                log::info!(
                    "synced {} files: {} messages, {} catches in {:?}",
                    r.files_read,
                    r.messages,
                    r.catches,
                    started.elapsed()
                );
            }
            for e in &r.errors {
                log::warn!("skipped file: {e}");
            }
            status.last_error = None;
            status.files_with_errors = r.errors.len();
            drop(status);
            let _ = handle.emit("stats-changed", &r);
            // A big pass is history arriving, not new catches: don't celebrate old milestones.
            let backfill = quiet || r.rescored || r.messages > BACKFILL_MESSAGES;
            milestones::check(handle, app, backfill);
            update_tray(handle, app);
        }
        Err(e) => {
            log::error!("sync failed: {e:#}");
            status.last_error = Some(e.to_string());
            drop(status);
            let _ = handle.emit("sync-failed", e.to_string());
        }
    }
}

/// Hovering the menu bar icon shows the score without opening anything.
fn update_tray(handle: &AppHandle, app: &App) {
    let Ok(s) = lock(&app.reader).stats(&Default::default(), chrono::Local::now().date_naive()) else { return };
    if let Some(tray) = handle.tray_by_id("tray") {
        let tip = format!("No, Actually... · {} catches this week · {}", s.catches_last_7d, s.tier);
        let _ = tray.set_tooltip(Some(tip));
    }
}

pub fn spawn(handle: AppHandle, app: Arc<App>, tx: mpsc::Sender<Msg>, rx: mpsc::Receiver<Msg>) {
    std::thread::Builder::new()
        .name("tracker".into())
        .spawn(move || {
            let mut watcher = watch(&app, &tx);
            // A panic in one pass (a malformed file, say) must not stop tracking for good.
            let guarded = |quiet: bool| {
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sync_once(&handle, &app, quiet))).is_err() {
                    log::error!("sync pass panicked; will retry on the next change or timer");
                    lock(&app.status).syncing = false;
                }
            };
            guarded(false); // catch up on anything written while the app was closed
            loop {
                let mut sources_changed = false;
                match rx.recv_timeout(SAFETY_NET) {
                    Ok(Msg::SourcesChanged) => sources_changed = true,
                    Ok(Msg::FilesChanged) => {
                        // Coalesce bursts: an AI reply streams many writes. Keep any consent change
                        // that arrives meanwhile, or a newly allowed source would go unwatched.
                        std::thread::sleep(DEBOUNCE);
                        while let Ok(m) = rx.try_recv() {
                            sources_changed |= matches!(m, Msg::SourcesChanged);
                        }
                    }
                    Ok(Msg::SyncNow) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
                if sources_changed {
                    watcher = watch(&app, &tx);
                }
                guarded(sources_changed);
            }
            drop(watcher);
        })
        .expect("start tracker thread");
}
