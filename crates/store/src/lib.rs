//! The local database. Everything the app knows lives here, and only here.
//!
//! [`Store`] owns one SQLite connection: consent and settings, per-file checkpoints, daily
//! counts, and correction events whose text obeys the [`Retention`] level. [`sync`] reads whatever
//! changed in the allowed sources; [`Store::stats`] and [`Store::progress`] answer every view; [`milestones`] decides
//! which moments deserve a popup.
//!
//! Tests: small pure helpers keep their tests inline in `mod tests`; the store's behavior suite
//! (ingest, sync, retention, stats) lives in `src/tests.rs` because it drives the whole crate.

#![warn(missing_docs)]

mod cards;
mod consent;
mod error;
mod files;
mod goals;
mod ingest;
mod insights;
mod meta;
pub mod milestones;
mod paths;
mod progress;
mod rebuild;
mod retention;
mod schema;
mod stats;
mod sync;
mod tips;

pub use consent::{source_stopped, SourceState};
pub use error::{Error, Result};
pub use files::{write_atomic, write_new};
pub use goals::{Goal, GoalKind, GoalNews, GoalStatus, GoalWeek, Trophy, WeekState, GOAL_WEEKS};
pub use insights::{Bests, CardTrend, HabitDrop, Insights, ModelLanded};
pub use meta::{meta, CategoryInfo, HabitInfo, Meta, SignalInfo, SourceInfo, TierInfo, Titled, VerdictTierInfo};
pub use paths::{
    card_secret, default_db_path, legacy_db_env_in_use, APP_ID, CARD_KEY_FILE, DB_ENV, DB_FILE, LEGACY_DB_ENV,
};
pub use progress::{
    floor_pct, HistoryWeek, Progress, RecentReportCard, ReportCardWeek, MIN_CARD_BEST, MIN_CARD_HALF, MIN_CARD_WEEK,
    MIN_WEEK, RECENT_REPORT_CARDS,
};
pub use rebuild::RULES_KEY;
pub use retention::{Level, Retention, BACKUP_FILE};
pub use stats::{Filter, HallEntry, ModelRow, Overall, Prompts, Stats, Tip, WeekRow};
pub use sync::{sync, SyncReport};
pub use tips::{TipActivity, TipTrend, TipWeek, TopTip};

pub(crate) use engine::key_hash as hash;
pub(crate) use ingest::{source_files, FileMeta, Pass};

use paths::protect;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

/// Shown when a message's chat never recorded a model (a session with no real reply).
pub const NO_MODEL: &str = "No model recorded";

/// The local database.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens (or creates) the database at `path`, creating its folder and migrating it.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            files::create_private_dir(dir).map_err(|source| Error::Io { path: dir.to_path_buf(), source })?;
        }
        // Made owner-only from the start; SQLite gives its -wal and -shm files the same mode.
        #[cfg(unix)]
        match write_new(path, b"", 0o600) {
            Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => {
                log::warn!("couldn't create the database as owner-only: {e}");
            }
            _ => {}
        }
        let mut conn = Connection::open(path)?;
        // The app keeps a reader and a writer on the same file; wait instead of failing on a lock.
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        schema::migrate(&mut conn)?;
        protect(path);
        let store = Self { conn };
        store.refresh_consent_markers()?;
        Ok(store)
    }

    /// A throwaway database, for tests and demos.
    pub fn open_in_memory() -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        schema::migrate(&mut conn)?;
        Ok(Self { conn })
    }

    /// Closes the connection, and says so if SQLite couldn't. Dropping a [`Store`] closes it too, but
    /// silently. The last connection to close removes the `-wal` and `-shm` files.
    pub fn close(self) -> Result<()> {
        self.conn.close().map_err(|(_, e)| e.into())
    }

    /// Small app-owned values (milestones already announced, onboarding done, …).
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    /// Sets an app-owned value.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        put_setting(&self.conn, key, value)
    }

    /// The database's format version, for the log.
    pub fn schema_version(&self) -> Result<usize> {
        Ok(self.conn.pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }
}

/// Saves one setting on `conn`, which may be a transaction.
fn put_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

#[cfg(test)]
mod card_tests;
#[cfg(test)]
mod modes_tests;
#[cfg(test)]
mod tests;
