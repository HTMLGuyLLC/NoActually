//! What the person decided about each source: allowed, paused, how far back, or forgotten. The
//! markers beside the database say the same thing to the hooks, which can't open it.

use crate::cards::clear_left_out;
use crate::ingest::{source_files, upsert_file, FileMeta, Pass};
use crate::rebuild::{KEPT_TABLES, SCORE_TABLES};
use crate::{files, Result, Store};
use chrono::{DateTime, Local, NaiveDate, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use sources::Source;
use std::path::{Path, PathBuf};

/// The file beside the database that says a source is off limits.
fn stopped_file(db: &Path, source: &str) -> PathBuf {
    db.with_file_name(format!("source-stopped-{source}"))
}

/// The file beside the database that says a source is allowed.
fn allowed_file(db: &Path, source: &str) -> PathBuf {
    db.with_file_name(format!("source-allowed-{source}"))
}

/// True when nothing may read `source`'s chats: the person stopped, paused or forgot it, or never
/// allowed it. `db` is the database's path. With no database the app was never set up, so there's
/// no consent to go by: the person installed the plugin on its own and asked for it.
pub fn source_stopped(db: &Path, source: &str) -> bool {
    stopped_file(db, source).exists() || (db.exists() && !allowed_file(db, source).exists())
}

/// What the user decided about one source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceState {
    /// The source's id.
    pub id: String,
    /// Read only while allowed and not paused.
    pub allowed: bool,
    /// Allowed, but not read for now.
    pub paused: bool,
    /// Ignore history before this day. `None` means everything.
    pub backfill_since: Option<NaiveDate>,
    /// When the user allowed it.
    pub granted_at: Option<DateTime<Utc>>,
    /// "From now on": ignore every message written before this moment.
    #[serde(default)]
    pub start_after: Option<DateTime<Utc>>,
    /// Extra files or folders the user pointed this source at (exports live here).
    pub paths: Vec<String>,
}

impl SourceState {
    /// A source nobody has decided about yet: not allowed, no extra paths.
    pub fn new(id: &str) -> Self {
        Self {
            id: id.into(),
            allowed: false,
            paused: false,
            backfill_since: None,
            granted_at: None,
            start_after: None,
            paths: Vec::new(),
        }
    }

    /// False for a message this source's consent says to skip (before the backfill day, or
    /// before "From now on").
    pub(crate) fn wants(&self, ts: DateTime<Utc>) -> bool {
        let day = ts.with_timezone(&Local).date_naive();
        !(self.backfill_since.is_some_and(|s| day < s) || self.start_after.is_some_and(|t| ts < t))
    }
}

impl Store {
    /// Every source the user has made a decision about.
    pub fn source_states(&self) -> Result<Vec<SourceState>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, allowed, paused, backfill_since, granted_at, paths, start_after FROM sources")?;
        let rows = stmt.query_map([], |r| {
            Ok(SourceState {
                id: r.get(0)?,
                allowed: r.get(1)?,
                paused: r.get(2)?,
                backfill_since: r.get::<_, Option<String>>(3)?.and_then(|s| s.parse().ok()),
                granted_at: r.get::<_, Option<String>>(4)?.and_then(|s| s.parse().ok()),
                paths: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default(),
                start_after: r.get::<_, Option<String>>(6)?.and_then(|s| s.parse().ok()),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// One source's state, if the user has made a decision about it.
    pub fn source_state(&self, id: &str) -> Result<Option<SourceState>> {
        Ok(self.source_states()?.into_iter().find(|s| s.id == id))
    }

    /// The user allowed reading this source. Nothing is read before this.
    pub fn allow(&self, id: &str, backfill_since: Option<NaiveDate>) -> Result<()> {
        // Reaching further back than before (say "All history" after "From now on") can't use
        // the old checkpoints: they sit at the end of files whose older messages were skipped.
        // The `seen` keys keep re-read messages from counting twice.
        if let Some(prev) = self.source_state(id)? {
            let before = prev.start_after.map(|t| t.date_naive()).or(prev.backfill_since);
            let reaches_back = match (backfill_since, before) {
                (None, Some(_)) => true,
                (Some(new), Some(old)) => new < old,
                _ => false,
            };
            if reaches_back {
                self.conn.execute("DELETE FROM files WHERE source = ?1", [id])?;
            }
        }
        self.conn.execute(
            "INSERT INTO sources (id, allowed, paused, backfill_since, granted_at, paths) VALUES (?1, 1, 0, ?2, ?3, '[]')
             ON CONFLICT(id) DO UPDATE SET allowed = 1, paused = 0, backfill_since = excluded.backfill_since,
               granted_at = excluded.granted_at, start_after = NULL",
            params![id, backfill_since.map(|d| d.to_string()), Utc::now().to_rfc3339()],
        )?;
        self.mark_stopped(id, false);
        Ok(())
    }

    /// Says beside the database whether `id` is off limits, for the hooks: they run outside the app,
    /// on every reply, and have to know without opening it. A failure is logged, not fatal, so it
    /// can't block turning a source off.
    /// The two markers disagree only if a write failed, and either one alone keeps the hooks out.
    fn mark_stopped(&self, id: &str, stopped: bool) {
        let Some(db) = self.conn.path().filter(|p| !p.is_empty()) else { return };
        let (db, make, remove) = (Path::new(db), stopped, !stopped);
        for (file, wanted) in [(stopped_file(db, id), make), (allowed_file(db, id), remove)] {
            let done = if wanted {
                match files::write_new(&file, b"", 0o644) {
                    Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => Err(e),
                    _ => Ok(()),
                }
            } else {
                match std::fs::remove_file(&file) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                    _ => Ok(()),
                }
            };
            if let Err(e) = done {
                log::warn!("couldn't update the consent marker for {id}: {e}");
            }
        }
    }

    /// Writes the consent markers for every known source, so hooks agree with the database even for
    /// a source allowed before the allowed marker existed.
    pub(crate) fn refresh_consent_markers(&self) -> Result<()> {
        for state in self.source_states()? {
            self.mark_stopped(&state.id, !state.allowed || state.paused);
        }
        Ok(())
    }

    /// "From now on": marks every file this source has today as already read, so only messages
    /// written after this moment count. Returns how many files were marked.
    pub fn start_from_now(&mut self, src: &dyn Source) -> Result<usize> {
        // The cutoff is what guarantees it; the checkpoints just save re-reading old bytes.
        self.conn
            .execute("UPDATE sources SET start_after = ?2 WHERE id = ?1", params![src.id(), Utc::now().to_rfc3339()])?;
        let extra = self.source_state(src.id())?.map(|s| s.paths).unwrap_or_default();
        let tx = self.conn.transaction()?;
        let mut marked = 0;
        for path in source_files(src, &extra) {
            let Ok((meta, _)) = FileMeta::read(&path) else { continue };
            upsert_file(&tx, &path.to_string_lossy(), src.id(), meta.size, Pass::new(meta, false))?;
            marked += 1;
        }
        tx.commit()?;
        Ok(marked)
    }

    /// Points a source at one more file or folder (an export). Doesn't change consent.
    pub fn add_path(&self, id: &str, path: &str) -> Result<()> {
        let mut paths = self.source_state(id)?.map(|s| s.paths).unwrap_or_default();
        if !paths.iter().any(|p| p == path) {
            paths.push(path.into());
        }
        self.conn.execute(
            "INSERT INTO sources (id, paths) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET paths = excluded.paths",
            params![id, serde_json::to_string(&paths)?],
        )?;
        Ok(())
    }

    /// Imports an export: adds the path and allows the source (all history), so the next sync
    /// reads it. Picking a file to import is the consent. The CLI and the app both use this.
    pub fn import(&self, id: &str, path: &str) -> Result<()> {
        self.add_path(id, path)?;
        self.allow(id, None)
    }

    /// Pauses or resumes an allowed source.
    pub fn set_paused(&self, id: &str, paused: bool) -> Result<()> {
        self.conn.execute("UPDATE sources SET paused = ?2 WHERE id = ?1", params![id, paused])?;
        self.mark_stopped(id, paused || !self.source_state(id)?.is_some_and(|s| s.allowed));
        Ok(())
    }

    /// Stop reading. Keeps the scores already counted.
    pub fn revoke(&self, id: &str) -> Result<()> {
        self.conn.execute("UPDATE sources SET allowed = 0 WHERE id = ?1", [id])?;
        self.mark_stopped(id, true);
        Ok(())
    }

    /// Whether `id` is still allowed. `noactually forget` from another process doesn't wait for a
    /// running pass, which holds what it read in memory: this stops the pass writing it back.
    pub(crate) fn still_allowed(&self, id: &str) -> Result<bool> {
        Ok(self.source_state(id)?.is_some_and(|s| s.allowed))
    }

    /// Stop reading and delete what this source produced. Finished goal weeks and trophies stay:
    /// they hold totals, with no source on them.
    pub fn forget(&mut self, id: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        for table in SCORE_TABLES {
            tx.execute(&format!("DELETE FROM {table} WHERE source = ?1"), [id])?;
        }
        for table in KEPT_TABLES {
            tx.execute(&format!("DELETE FROM kept_{table} WHERE source = ?1"), [id])?;
        }
        tx.execute("DELETE FROM rebuild_read WHERE source = ?1", [id])?;
        tx.execute("DELETE FROM report_cards WHERE source = ?1", [id])?;
        tx.execute("DELETE FROM sources WHERE id = ?1", [id])?;
        clear_left_out(&tx, Some(id))?;
        tx.commit()?;
        self.mark_stopped(id, true);
        self.flush_deleted()
    }
}
