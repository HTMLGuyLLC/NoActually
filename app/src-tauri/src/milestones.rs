//! Popups for moments worth sharing. What counts as a milestone is decided in
//! `store::milestones`; this module only shows the notifications.
//! Bulk reads (first launch, a newly allowed source, a rules rebuild) only move the baseline,
//! so backfilling history never fires a pile of old milestones.

use crate::{lock, App};
use chrono::Local;
use store::milestones::{crossed, Snapshot};
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

/// `quiet` moves the baseline without notifying: used when history was just read in bulk.
pub fn check(handle: &AppHandle, app: &App, quiet: bool) {
    let store = lock(&app.writer);
    let Ok(stats) = store.stats(&store::Filter::default(), Local::now().date_naive()) else { return };
    let now = Snapshot::of(&stats);
    let prev = store.advance_milestones(&now);
    drop(store);
    let prev = match prev {
        Ok(Some(prev)) if !quiet => prev,
        Ok(_) => return,
        Err(e) => {
            log::warn!("couldn't save the milestone baseline: {e}");
            return;
        }
    };
    for m in crossed(&prev, &now) {
        let _ = handle.notification().builder().title(m.title).body(m.body).show();
    }
}
