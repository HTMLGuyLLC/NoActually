//! Background tracking: one thread owns syncing. File events and a timer only ever ask it
//! to sync; the timer is the safety net for anything the OS watcher misses.

use crate::logging::{home_short, shorten_home_in};
use crate::{lock, milestones, App, SyncStatus};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use sources::Kind;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub enum Msg {
    SyncNow,
    FilesChanged,
    SourcesChanged,
    /// Ends tracking for good. Sent back on once no pass is running and none will start.
    Stop(mpsc::Sender<()>),
}

const SAFETY_NET: Duration = Duration::from_secs(5 * 60);
const DEBOUNCE: Duration = Duration::from_secs(2);
/// Live tracking checks at most this often while an AI tool is writing: a busy session rewrites
/// its files every few seconds, and scores don't need to move that fast. "Check now" and consent
/// changes don't wait.
const MIN_GAP: Duration = Duration::from_secs(60);
/// Shown once after the rules change, because scores can move with no new chats.
const RESCORED_TITLE: &str = "Your scores were updated";
const RESCORED_BODY: &str = "The scoring rules improved, so your history was counted again. Chats no longer on your computer keep their old counts.";

/// Folders to watch: only the roots of sources the user has allowed.
fn watched_roots(app: &App) -> Vec<PathBuf> {
    let Ok(states) = lock(&app.reader).source_states() else { return vec![] };
    let mut roots = Vec::new();
    for st in states.iter().filter(|s| s.allowed && !s.paused) {
        match sources::find(&app.sources, &st.id) {
            Ok(src) if src.kind() == Kind::Live => roots.extend(src.roots().into_iter().filter(|r| r.exists())),
            _ => {}
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
            log::warn!("can't watch {}: {e}", home_short(&root));
        }
    }
    Some(w)
}

/// How many skipped files get a line of their own in the log.
const LOGGED_SKIPS: usize = 5;

/// The log lines for the files a pass skipped: the first few, then the total. A bad folder can skip
/// thousands, and every pass would write them all again.
fn skipped_lines(errors: &[String], home: Option<&Path>) -> Vec<String> {
    let mut lines: Vec<String> =
        errors.iter().take(LOGGED_SKIPS).map(|e| format!("skipped file: {}", shorten_home_in(e, home))).collect();
    if errors.len() > LOGGED_SKIPS {
        lines.push(format!("{} files skipped in all; the first {LOGGED_SKIPS} are listed", errors.len()));
    }
    lines
}

/// The line a pass leaves in the log, if it did anything worth a line: it read files, or it rebuilt
/// scores (which can happen with no new files at all).
fn sync_line(files: usize, messages: usize, corrections: usize, rescored: bool, took: Duration) -> Option<String> {
    if files == 0 && !rescored {
        return None;
    }
    let rebuilt = if rescored {
        format!(" (scores rebuilt under scoring rules {})", engine::RULES_VERSION)
    } else {
        String::new()
    };
    Some(format!("synced {files} files{rebuilt}: {messages} messages, {corrections} corrections in {took:?}"))
}

/// Whether windows should redraw after a pass: numbers moved, or what the status line says did. A
/// pass that found nothing new only ends the check, since redrawing every window for it is wasted
/// work. A rebuild always counts: rows restored at its end change the numbers even if no file was read.
fn pass_changed(files_read: usize, rescored: bool, errors: usize, before: &SyncStatus, rebuilt: bool) -> bool {
    rebuilt || files_read > 0 || rescored || before.files_with_errors != errors || before.last_error.is_some()
}

/// A pass has started.
fn pass_started(status: &mut SyncStatus, rebuilding: bool) {
    status.syncing = true;
    status.rebuilding = rebuilding;
}

/// A pass ended well, with `errors` files skipped.
fn pass_succeeded(status: &mut SyncStatus, errors: usize, now: chrono::DateTime<chrono::Utc>) {
    status.syncing = false;
    status.rebuilding = false;
    status.last_sync = Some(now);
    status.last_error = None;
    status.files_with_errors = errors;
}

/// A pass failed: the last sync time still moves, so the window says when it last tried.
fn pass_failed(status: &mut SyncStatus, message: String, now: chrono::DateTime<chrono::Utc>) {
    status.syncing = false;
    status.rebuilding = false;
    status.last_sync = Some(now);
    status.last_error = Some(message);
}

/// A pass panicked: the window says so, and the next change or timer tries again.
fn pass_panicked(status: &mut SyncStatus) {
    status.syncing = false;
    status.rebuilding = false;
    status.last_error = Some(PANIC_MESSAGE.to_string());
}

/// `quiet`: record milestones without announcing them (a backfill, or a source just allowed).
fn sync_once(handle: &AppHandle, app: &App, quiet: bool) {
    // History is read again from zero when the rules changed, so the numbers are partial until the end.
    let rebuilding = lock(&app.reader).rebuild_due().unwrap_or_else(|e| {
        log::warn!("couldn't tell if a rebuild is due: {e}");
        false
    });
    if rebuilding {
        log::info!("rebuilding: history will be read again under scoring rules {}", engine::RULES_VERSION);
    }
    pass_started(&mut lock(&app.status), rebuilding);
    let _ = handle.emit("sync-started", ());
    let started = std::time::Instant::now();
    let report = {
        let mut store = lock(&app.writer);
        store::sync(&mut store, &app.sources)
    };
    let mut status = lock(&app.status);
    match report {
        Ok(r) => {
            if let Some(line) = sync_line(r.files_read, r.messages, r.corrections, r.rescored, started.elapsed()) {
                log::info!("{line}");
            }
            if r.skipped_lines > 0 {
                log::warn!("{} lines couldn't be read; a tool may have changed its format", r.skipped_lines);
            }
            for line in skipped_lines(&r.errors, std::env::home_dir().as_deref()) {
                log::warn!("{line}");
            }
            let changed = pass_changed(r.files_read, r.rescored, r.errors.len(), &status, rebuilding);
            pass_succeeded(&mut status, r.errors.len(), chrono::Utc::now());
            drop(status);
            if r.rescored {
                // Kept so the window can say so after the popup is gone, or if popups are off.
                let when = chrono::Utc::now().to_rfc3339();
                if let Err(e) = lock(&app.writer).set_setting(crate::RESCORED_NOTICE_KEY, &when) {
                    log::warn!("couldn't note that scores were rebuilt: {e}");
                }
            }
            let _ = handle.emit(if changed { "stats-changed" } else { "sync-idle" }, &r);
            if r.rescored {
                crate::notify(handle, app, RESCORED_TITLE, RESCORED_BODY);
            }
            // A big pass is history arriving, not new corrections: don't celebrate old milestones.
            milestones::check(handle, app, milestones::Quiet::for_pass(quiet, r.rescored, r.messages));
            update_tray(handle, app);
        }
        Err(e) => {
            log::error!("sync failed: {}", shorten_home_in(&format!("{e:#}"), std::env::home_dir().as_deref()));
            pass_failed(&mut status, e.to_string(), chrono::Utc::now());
            drop(status);
            let _ = handle.emit("sync-failed", e.to_string());
        }
    }
}

/// Hovering the menu bar icon shows the score without opening anything.
fn update_tray(handle: &AppHandle, app: &App) {
    let Ok(o) = lock(&app.reader).overall(chrono::Local::now().date_naive()) else { return };
    if let Some(tray) = handle.tray_by_id("tray") {
        let tip = match (o.score, o.tier) {
            (Some(score), Some(tier)) => format!("No, Actually... · {} · score {score}", tier.name()),
            _ => format!("No, Actually... · {} corrections in {} days", o.corrections, engine::WINDOW_DAYS),
        };
        let _ = tray.set_tooltip(Some(tip));
    }
}

/// What the window shows when a pass panics. The panic itself goes to the log.
const PANIC_MESSAGE: &str = "Couldn't finish checking your chats. It will try again.";

/// Asks the tracking thread to end and waits up to `wait` for it. A pass can't be cut short, so this
/// waits for the one running. True once it has stopped. A thread that's already gone counts: it can
/// never write again. When the wait runs out it still stops after its pass, so asking again is safe.
pub fn stop(tracker: &mpsc::Sender<Msg>, wait: Duration) -> bool {
    let (done, stopped) = mpsc::channel();
    if tracker.send(Msg::Stop(done)).is_err() {
        return true;
    }
    !matches!(stopped.recv_timeout(wait), Err(mpsc::RecvTimeoutError::Timeout))
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
                    {
                        let mut status = lock(&app.status);
                        pass_panicked(&mut status);
                    }
                    let _ = handle.emit("sync-failed", PANIC_MESSAGE);
                }
            };
            guarded(false); // catch up on anything written while the app was closed
            let mut last = std::time::Instant::now();
            loop {
                let mut sources_changed = false;
                match rx.recv_timeout(SAFETY_NET) {
                    Ok(Msg::SourcesChanged) => sources_changed = true,
                    Ok(Msg::FilesChanged) => {
                        // Coalesce bursts (an AI reply streams many writes) and wait out the gap
                        // since the last pass. A consent change or "Check now" meanwhile ends the
                        // wait, so a newly allowed source is never left unwatched.
                        let until = std::time::Instant::now() + DEBOUNCE.max(MIN_GAP.saturating_sub(last.elapsed()));
                        loop {
                            match rx.recv_timeout(until.saturating_duration_since(std::time::Instant::now())) {
                                Ok(Msg::FilesChanged) => {}
                                Ok(Msg::SourcesChanged) => {
                                    sources_changed = true;
                                    break;
                                }
                                Ok(Msg::SyncNow) | Err(mpsc::RecvTimeoutError::Timeout) => break,
                                Ok(Msg::Stop(done)) => {
                                    let _ = done.send(());
                                    return;
                                }
                                Err(mpsc::RecvTimeoutError::Disconnected) => return,
                            }
                        }
                    }
                    Ok(Msg::SyncNow) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Ok(Msg::Stop(done)) => {
                        let _ = done.send(());
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
                if sources_changed {
                    watcher = watch(&app, &tx);
                }
                guarded(sources_changed);
                last = std::time::Instant::now();
            }
            drop(watcher);
        })
        .expect("start tracker thread");
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn at() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 1, 12, 0, 0).unwrap()
    }

    /// A stand-in for the tracking thread, which answers `Stop` after `pass`.
    fn tracker_busy_for(pass: Duration) -> mpsc::Sender<Msg> {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(msg) = rx.recv() {
                if let Msg::Stop(done) = msg {
                    std::thread::sleep(pass);
                    let _ = done.send(());
                    return;
                }
            }
        });
        tx
    }

    #[test]
    fn stopping_waits_for_the_tracking_thread_and_says_if_it_ran_out_of_time() {
        assert!(stop(&tracker_busy_for(Duration::ZERO), Duration::from_secs(5)));
        let busy = tracker_busy_for(Duration::from_millis(300));
        assert!(!stop(&busy, Duration::from_millis(10)), "a pass still running means not stopped");
        let (gone, rx) = mpsc::channel();
        drop(rx);
        assert!(stop(&gone, Duration::ZERO), "a thread that's gone can't write");
    }

    #[test]
    fn a_quiet_pass_does_not_redraw() {
        let idle = SyncStatus::default();
        assert!(!pass_changed(0, false, 0, &idle, false));
    }

    #[test]
    fn a_pass_redraws_when_numbers_or_the_status_line_moved() {
        let idle = SyncStatus::default();
        assert!(pass_changed(1, false, 0, &idle, false), "files were read");
        assert!(pass_changed(0, true, 0, &idle, false), "scores were rebuilt");
        assert!(pass_changed(0, false, 2, &idle, false), "files newly skipped");
        assert!(pass_changed(0, false, 0, &idle, true), "a rebuild restores rows even if no file was read");
        let skipping = SyncStatus { files_with_errors: 2, ..SyncStatus::default() };
        assert!(pass_changed(0, false, 0, &skipping, false), "the skips went away");
        assert!(!pass_changed(0, false, 2, &skipping, false), "same skips, nothing new");
        let failed = SyncStatus { last_error: Some("disk full".into()), ..SyncStatus::default() };
        assert!(pass_changed(0, false, 0, &failed, false), "the error cleared");
    }

    #[test]
    fn a_pass_sets_and_clears_the_rebuilding_flag() {
        let mut s = SyncStatus::default();
        pass_started(&mut s, true);
        assert!(s.syncing && s.rebuilding);
        pass_succeeded(&mut s, 0, at());
        assert!(!s.syncing && !s.rebuilding);
        pass_started(&mut s, true);
        pass_failed(&mut s, "boom".into(), at());
        assert!(!s.syncing && !s.rebuilding, "a failure ends the rebuilding state too");
        pass_started(&mut s, true);
        pass_panicked(&mut s);
        assert!(!s.syncing && !s.rebuilding);
    }

    #[test]
    fn success_clears_the_error_and_records_the_skips() {
        let mut s = SyncStatus { last_error: Some("old".into()), ..SyncStatus::default() };
        pass_succeeded(&mut s, 3, at());
        assert_eq!((s.last_error, s.files_with_errors, s.last_sync), (None, 3, Some(at())));
    }

    #[test]
    fn failure_keeps_the_message_and_the_skip_count() {
        let mut s = SyncStatus { files_with_errors: 3, ..SyncStatus::default() };
        pass_failed(&mut s, "disk full".into(), at());
        assert_eq!((s.last_error.as_deref(), s.files_with_errors, s.last_sync), (Some("disk full"), 3, Some(at())));
        pass_panicked(&mut s);
        assert_eq!(s.last_error.as_deref(), Some(PANIC_MESSAGE));
    }

    #[test]
    fn only_the_first_skipped_files_are_logged_with_a_total() {
        let home = Some(Path::new("/Users/me"));
        let few = vec!["/Users/me/a.jsonl: bad".to_string()];
        assert_eq!(skipped_lines(&few, home), vec!["skipped file: ~/a.jsonl: bad"]);
        assert_eq!(skipped_lines(&[], home).len(), 0);
        let many: Vec<String> = (0..40).map(|i| format!("/Users/me/{i}.jsonl: bad")).collect();
        let lines = skipped_lines(&many, home);
        assert_eq!(lines.len(), LOGGED_SKIPS + 1);
        assert_eq!(lines[LOGGED_SKIPS], "40 files skipped in all; the first 5 are listed");
        assert!(lines.iter().all(|l| !l.contains("/Users/me")), "{lines:?}");
    }

    #[test]
    fn a_rebuild_is_logged_even_when_no_file_was_read() {
        let took = Duration::from_secs(1);
        assert_eq!(sync_line(0, 0, 0, false, took), None);
        let line = sync_line(0, 0, 0, true, took).expect("a rebuild leaves a line");
        assert!(line.contains("rebuilt under scoring rules"), "{line}");
        assert!(line.contains(&engine::RULES_VERSION.to_string()), "{line}");
        let line = sync_line(3, 10, 2, false, took).unwrap();
        assert!(line.starts_with("synced 3 files: 10 messages, 2 corrections"), "{line}");
    }
}
