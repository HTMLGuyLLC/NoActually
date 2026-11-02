//! How long correction text is kept, and making sure it's gone once it isn't: aging it down,
//! cleaning it with newer redaction, and clearing it from the write-ahead log and the backup.

use crate::{files, put_setting, Error, Result, Store};
use chrono::{DateTime, Duration, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// How much correction text to keep. Levels only ever step down as events age.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// Your whole message and the start of the AI's reply (still redacted).
    Full,
    /// The matched sentence from each side, at most 140 chars, redacted.
    Snippet,
    /// No text at all.
    Category,
}

impl Level {
    /// The one of the two that keeps less text.
    #[must_use]
    pub fn at_most(self, other: Level) -> Level {
        let rank = |l| [Level::Category, Level::Snippet, Level::Full].iter().position(|x| *x == l);
        if rank(self) <= rank(other) {
            self
        } else {
            other
        }
    }

    /// Stable id, as stored and serialized.
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Full => "full",
            Level::Snippet => "snippet",
            Level::Category => "category",
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Level {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        [Level::Full, Level::Snippet, Level::Category]
            .into_iter()
            .find(|l| l.as_str() == s)
            .ok_or(Error::Invalid("Pick full, snippet or category."))
    }
}

/// The longest a retention period can be (100 years). Keeps the cutoff dates in range.
const MAX_RETENTION_DAYS: u32 = 36_500;

/// How long correction text is kept, and at what level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Retention {
    /// The level new corrections start at.
    pub level: Level,
    /// Days full text is kept before it drops to a snippet.
    pub full_days: u32,
    /// Days snippets are kept before only the category remains.
    pub snippet_days: u32,
}

impl Default for Retention {
    fn default() -> Self {
        Self { level: Level::Snippet, full_days: 30, snippet_days: 365 }
    }
}

impl Retention {
    /// Both periods must be between a day and 100 years.
    pub fn validate(&self) -> Result<()> {
        if self.full_days == 0 || self.snippet_days == 0 {
            return Err(Error::Invalid("Keep text for at least 1 day."));
        }
        if self.full_days > MAX_RETENTION_DAYS || self.snippet_days > MAX_RETENTION_DAYS {
            return Err(Error::Invalid("Keep text for at most 36500 days."));
        }
        Ok(())
    }

    /// Whether saving this keeps less than `saved` does, so older text is deleted for good.
    #[must_use]
    pub fn keeps_less_than(&self, saved: &Retention) -> bool {
        let both = |uses: fn(Level) -> bool| uses(self.level) && uses(saved.level);
        self.level.at_most(saved.level) != saved.level
            || (both(|l| l == Level::Full) && self.full_days < saved.full_days)
            || (both(|l| l != Level::Category) && self.snippet_days < saved.snippet_days)
    }
}

/// How often to retry a busy log truncation, and how long to wait between tries.
const CHECKPOINT_TRIES: u32 = 5;
const CHECKPOINT_WAIT: std::time::Duration = std::time::Duration::from_millis(100);

/// The [`engine::REDACTION_VERSION`] stored text was last cleaned with.
pub(crate) const REDACTION_KEY: &str = "redaction_version";

/// Kept beside the database. Deleting text removes it, and the next pass writes a new one.
pub const BACKUP_FILE: &str = "noactually-backup.db";
const BACKUP_EVERY: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

impl Store {
    /// The retention settings, or the defaults if never set. A saved value that can't be read
    /// (say, written by a newer version) means new corrections keep no text, never the defaults,
    /// which might keep more than the user chose.
    pub fn retention(&self) -> Result<Retention> {
        Ok(self.saved_retention()?.unwrap_or(Retention { level: Level::Category, ..Retention::default() }))
    }

    /// The retention settings, or `None` if the saved value can't be read.
    fn saved_retention(&self) -> Result<Option<Retention>> {
        let Some(raw) = self.get_setting("retention")? else { return Ok(Some(Retention::default())) };
        match serde_json::from_str::<Retention>(&raw).map_err(Error::from).and_then(|r| r.validate().map(|()| r)) {
            Ok(r) => Ok(Some(r)),
            Err(e) => {
                log::error!(
                    "the saved retention setting is unreadable ({e}); new corrections keep no text until it is set again"
                );
                Ok(None)
            }
        }
    }

    /// Saves retention settings and applies them to what's already stored.
    pub fn set_retention(&self, r: Retention) -> Result<()> {
        r.validate()?;
        self.set_setting("retention", &serde_json::to_string(&r)?)?;
        self.sweep(Utc::now())?;
        Ok(())
    }

    /// Runs newer redaction over text already stored, once. Text whose chat can't be read again would
    /// otherwise keep whatever the older rules missed, and pinned text is the likeliest to be shared.
    pub fn redact_stored_text(&mut self) -> Result<()> {
        let current = engine::REDACTION_VERSION;
        let stored = self.get_setting(REDACTION_KEY)?.and_then(|v| v.parse::<u32>().ok());
        if stored.is_some_and(|v| v >= current) {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        let mut changed = false;
        for table in ["events", "kept_events"] {
            let rows = {
                let mut q = tx.prepare(&format!(
                    "SELECT rowid, human_full, ai_full, human_snip, ai_snip FROM {table}
                     WHERE human_full IS NOT NULL OR ai_full IS NOT NULL OR human_snip IS NOT NULL OR ai_snip IS NOT NULL"
                ))?;
                let rows = q.query_map([], |r| {
                    let text = |i| r.get::<_, Option<String>>(i);
                    Ok((r.get::<_, i64>(0)?, [text(1)?, text(2)?, text(3)?, text(4)?]))
                })?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            };
            for (id, text) in rows {
                let clean = text.clone().map(|t| t.map(|t| engine::redact(&t)));
                if clean != text {
                    tx.execute(
                        &format!(
                            "UPDATE {table} SET human_full = ?2, ai_full = ?3, human_snip = ?4, ai_snip = ?5
                             WHERE rowid = ?1"
                        ),
                        params![id, clean[0], clean[1], clean[2], clean[3]],
                    )?;
                    changed = true;
                }
            }
        }
        put_setting(&tx, REDACTION_KEY, &current.to_string())?;
        tx.commit()?;
        if changed {
            self.flush_deleted()?;
        }
        Ok(())
    }

    /// Writes a fresh copy of the database beside it, at most once a day and before a rebuild.
    /// A failure is only logged: a backup must never stop a sync.
    pub fn backup_if_due(&self) -> Result<()> {
        let Some(path) = self.backup_path() else { return Ok(()) };
        let age = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
        if age.is_some_and(|a| a < BACKUP_EVERY) && !self.rebuild_due()? {
            return Ok(());
        }
        let temp = path.with_extension("db.tmp");
        let _ = std::fs::remove_file(&temp);
        if let Err(e) = self.conn.execute("VACUUM INTO ?1", [temp.to_string_lossy()]) {
            log::warn!("couldn't back up the database: {e}");
            return Ok(());
        }
        files::restrict(&temp, 0o600);
        if let Err(e) = std::fs::rename(&temp, &path) {
            log::warn!("couldn't save the database backup: {e}");
        }
        Ok(())
    }

    fn backup_path(&self) -> Option<PathBuf> {
        let db = self.conn.path().filter(|p| !p.is_empty())?;
        Some(Path::new(db).with_file_name(BACKUP_FILE))
    }

    /// Steps aging events down: full → snippet → category. Pinned events keep their text.
    /// An unreadable retention setting skips the sweep: deleting text can't be undone, so it
    /// waits until the user's choice is known again.
    pub fn sweep(&self, now: DateTime<Utc>) -> Result<usize> {
        let Some(r) = self.saved_retention()? else { return Ok(0) };
        // A stored value out of range (an older build allowed any number) means nothing is that old.
        let cutoff = |days: u32| {
            now.checked_sub_signed(Duration::days(i64::from(days))).unwrap_or(DateTime::<Utc>::MIN_UTC).to_rfc3339()
        };
        let (full_cut, snip_cut) = (cutoff(r.full_days), cutoff(r.snippet_days));
        let mut changed = self.conn.execute(
            "UPDATE events SET human_full = NULL, ai_full = NULL, level = 'snippet'
             WHERE pinned = 0 AND level = 'full' AND (ts < ?1 OR ?2 != 'full')",
            params![full_cut, r.level.as_str()],
        )?;
        changed += self.conn.execute(
            "UPDATE events SET human_snip = NULL, ai_snip = NULL, human_full = NULL, ai_full = NULL, level = 'category'
             WHERE pinned = 0 AND level != 'category' AND (ts < ?1 OR ?2 = 'category')",
            params![snip_cut, r.level.as_str()],
        )?;
        if changed > 0 {
            self.flush_deleted()?;
        }
        Ok(changed)
    }

    /// Folds the write-ahead log back into the database and truncates it, so text that was just
    /// deleted doesn't linger in `noactually.db-wal`.
    pub fn flush_deleted(&self) -> Result<()> {
        // Returns (busy, log frames, checkpointed frames). busy = 1 means a reader blocked it.
        for attempt in 0..CHECKPOINT_TRIES {
            let busy: i64 = self.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))?;
            if attempt == 0 {
                if let Some(path) = self.backup_path() {
                    let _ = std::fs::remove_file(path); // it still holds what was just deleted
                }
            }
            if busy == 0 {
                return Ok(());
            }
            if attempt + 1 < CHECKPOINT_TRIES {
                std::thread::sleep(CHECKPOINT_WAIT);
            }
        }
        log::warn!(
            "couldn't truncate the write-ahead log (database busy); deleted text may linger until the next checkpoint"
        );
        Ok(())
    }

    /// Pins (or unpins) a correction, keeping its text past the retention period.
    pub fn pin(&self, event_id: i64, pinned: bool) -> Result<()> {
        self.conn.execute("UPDATE events SET pinned = ?2 WHERE id = ?1", params![event_id, pinned])?;
        Ok(())
    }
}
