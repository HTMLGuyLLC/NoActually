//! The local database. Everything the app knows lives here, and only here.

mod schema;
mod stats;
mod sync;

pub use stats::{Filter, HallEntry, ModelRow, Stats, WeekRow};
pub use sync::{sync, SyncReport};

use anyhow::Result;
use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use engine::{classify, normalize_model, snippet, Label};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

/// How much catch text to keep. Levels only ever step down as events age.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Full,
    Snippet,
    Category,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Level::Full => "full",
            Level::Snippet => "snippet",
            Level::Category => "category",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Retention {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceState {
    pub id: String,
    pub allowed: bool,
    pub paused: bool,
    /// Ignore history before this day. `None` means everything.
    pub backfill_since: Option<NaiveDate>,
    pub granted_at: Option<DateTime<Utc>>,
    /// "From now on": ignore every message written before this moment.
    #[serde(default)]
    pub start_after: Option<DateTime<Utc>>,
    /// Extra files or folders the user pointed this source at (exports live here).
    pub paths: Vec<String>,
}

/// Shown when a message's chat never recorded a model (a session with no real reply).
pub const NO_MODEL: &str = "No model recorded";

pub struct Store {
    conn: Connection,
}

pub(crate) fn hash(parts: &[&str]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p.as_bytes());
        h.update([0]);
    }
    h.finalize().iter().take(12).map(|b| format!("{b:02x}")).collect()
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        // The app keeps a reader and a writer on the same file; wait instead of failing on a lock.
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        schema::migrate(&conn)?;
        Ok(Self { conn })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        schema::migrate(&conn)?;
        Ok(Self { conn })
    }

    // ---- settings ----------------------------------------------------------------------

    pub fn retention(&self) -> Result<Retention> {
        let v: Option<String> =
            self.conn.query_row("SELECT value FROM settings WHERE key = 'retention'", [], |r| r.get(0)).optional()?;
        Ok(v.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default())
    }

    pub fn set_retention(&self, r: Retention) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES ('retention', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [serde_json::to_string(&r)?],
        )?;
        self.sweep(Utc::now())?;
        Ok(())
    }

    /// Small app-owned values (milestones already announced, onboarding done, …).
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    // ---- consent -----------------------------------------------------------------------

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

    pub fn source_state(&self, id: &str) -> Result<Option<SourceState>> {
        Ok(self.source_states()?.into_iter().find(|s| s.id == id))
    }

    /// The user allowed reading this source. Nothing is read before this.
    pub fn allow(&self, id: &str, backfill_since: Option<NaiveDate>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sources (id, allowed, paused, backfill_since, granted_at, paths) VALUES (?1, 1, 0, ?2, ?3, '[]')
             ON CONFLICT(id) DO UPDATE SET allowed = 1, paused = 0, backfill_since = excluded.backfill_since,
               granted_at = excluded.granted_at, start_after = NULL",
            params![id, backfill_since.map(|d| d.to_string()), Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// "From now on": marks every file this source has today as already read, so only messages
    /// written after this moment count. Returns how many files were marked.
    pub fn start_from_now(&mut self, src: &dyn sources::Source) -> Result<usize> {
        // The cutoff is what guarantees it; the checkpoints just save re-reading old bytes.
        self.conn
            .execute("UPDATE sources SET start_after = ?2 WHERE id = ?1", params![src.id(), Utc::now().to_rfc3339()])?;
        let extra = self.source_state(src.id())?.map(|s| s.paths).unwrap_or_default();
        let mut roots = src.roots();
        roots.extend(extra.iter().map(std::path::PathBuf::from));
        let tx = self.conn.transaction()?;
        let mut marked = 0;
        for root in roots {
            let files = if root.is_file() { vec![root] } else { src.files(&root) };
            for path in files {
                let Ok(meta) = std::fs::metadata(&path) else { continue };
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs() as i64);
                tx.execute(
                    "INSERT INTO files (path, source, checkpoint, size, mtime) VALUES (?1,?2,?3,?3,?4)
                     ON CONFLICT(path) DO UPDATE SET checkpoint = excluded.checkpoint, size = excluded.size, mtime = excluded.mtime",
                    params![path.to_string_lossy(), src.id(), meta.len() as i64, mtime],
                )?;
                marked += 1;
            }
        }
        tx.commit()?;
        Ok(marked)
    }

    pub fn add_path(&self, id: &str, path: &str) -> Result<()> {
        let mut st = self.source_state(id)?.unwrap_or(SourceState {
            id: id.into(),
            allowed: false,
            paused: false,
            backfill_since: None,
            granted_at: None,
            start_after: None,
            paths: vec![],
        });
        if !st.paths.iter().any(|p| p == path) {
            st.paths.push(path.into());
        }
        self.conn.execute(
            "INSERT INTO sources (id, allowed, paused, paths) VALUES (?1, ?2, 0, ?3)
             ON CONFLICT(id) DO UPDATE SET paths = excluded.paths",
            params![id, st.allowed, serde_json::to_string(&st.paths)?],
        )?;
        Ok(())
    }

    pub fn set_paused(&self, id: &str, paused: bool) -> Result<()> {
        self.conn.execute("UPDATE sources SET paused = ?2 WHERE id = ?1", params![id, paused])?;
        Ok(())
    }

    /// Stop reading. Keeps the scores already counted.
    pub fn revoke(&self, id: &str) -> Result<()> {
        self.conn.execute("UPDATE sources SET allowed = 0 WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Clears every score and checkpoint so history is re-read under new rules. Consent and
    /// settings stay. Scores only come back for history files that still exist.
    pub fn reset_scores(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        for table in ["events", "daily", "sessions", "files", "seen"] {
            tx.execute(&format!("DELETE FROM {table}"), [])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Rebuilds scores once if they were counted under older rules. Returns true if it did.
    pub fn ensure_rules_current(&mut self) -> Result<bool> {
        let current = engine::RULES_VERSION.to_string();
        let stored = self.get_setting("rules_version")?;
        if stored.as_deref() == Some(current.as_str()) {
            return Ok(false);
        }
        // Only counted scores need rebuilding. A store with checkpoints but no counts yet (say,
        // a source allowed "from now on") must keep its checkpoints.
        let had_scores: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM daily)", [], |r| r.get(0))?;
        if had_scores {
            self.reset_scores()?;
        }
        self.set_setting("rules_version", &current)?;
        Ok(had_scores)
    }

    /// Stop reading and delete everything this source ever produced.
    pub fn forget(&mut self, id: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        for table in ["events", "daily", "sessions", "files", "seen"] {
            tx.execute(&format!("DELETE FROM {table} WHERE source = ?1"), [id])?;
        }
        tx.execute("DELETE FROM sources WHERE id = ?1", [id])?;
        tx.commit()?;
        self.flush_deleted()
    }

    // ---- ingestion ---------------------------------------------------------------------

    pub(crate) fn file_checkpoint(&self, path: &str) -> Result<Option<(u64, u64, i64)>> {
        Ok(self
            .conn
            .query_row("SELECT checkpoint, size, mtime FROM files WHERE path = ?1", [path], |r| {
                Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64, r.get(2)?))
            })
            .optional()?)
    }

    /// Writes one file's turns and its new checkpoint in a single transaction, so a crash
    /// can never count a turn without moving the checkpoint (or the reverse).
    #[allow(clippy::too_many_arguments)] // one internal caller (sync); a struct would only rename them
    pub(crate) fn ingest(
        &mut self,
        source: &str,
        path: &str,
        scan: &sources::FileScan,
        size: u64,
        mtime: i64,
        since: Option<NaiveDate>,
        start_after: Option<DateTime<Utc>>,
    ) -> Result<(usize, usize)> {
        let retention = self.retention()?;
        let tx = self.conn.transaction()?;
        let (mut messages, mut catches) = (0, 0);
        for st in &scan.turns {
            let human_head: String = st.turn.human.chars().take(200).collect();
            let key = match &st.key {
                Some(k) => hash(&[source, k]),
                None => hash(&[source, &st.session, &st.ts.to_rfc3339(), &human_head]),
            };
            if tx.execute("INSERT OR IGNORE INTO seen (key, source) VALUES (?1, ?2)", params![key, source])? == 0 {
                continue; // a copy we've already counted (resumed or forked session, re-import)
            }
            let day = st.ts.with_timezone(&Local).date_naive();
            if since.is_some_and(|s| day < s) || start_after.is_some_and(|t| st.ts < t) {
                continue;
            }
            let v = classify(&st.turn);
            let model = v.blamed_model(&st.turn).and_then(normalize_model);
            let (provider, family, version) = model
                .as_ref()
                .map_or((NO_MODEL, NO_MODEL, ""), |m| (m.provider.as_str(), m.family.as_str(), m.version.as_str()));
            let day_s = day.to_string();
            let bump = |kind: &str, category: &str| {
                tx.execute(
                    "INSERT INTO daily (day, source, provider, family, version, kind, category, n) VALUES (?1,?2,?3,?4,?5,?6,?7,1)
                     ON CONFLICT DO UPDATE SET n = n + 1",
                    params![day_s, source, provider, family, version, kind, category],
                )
            };
            bump("message", "")?;
            messages += 1;
            let session = hash(&[source, &st.session]);
            let is_catch = v.label.is_some_and(Label::is_catch);
            tx.execute(
                "INSERT INTO sessions (id, source, first_day, last_day, messages, catches) VALUES (?1,?2,?3,?3,1,?4)
                 ON CONFLICT(id) DO UPDATE SET messages = messages + 1, catches = catches + excluded.catches,
                   first_day = min(first_day, excluded.first_day), last_day = max(last_day, excluded.last_day)",
                params![session, source, day_s, is_catch as i64],
            )?;
            let Some(label) = v.label else { continue };
            let category = v.category.map_or("", |c| c.as_str());
            bump(label.as_str(), category)?;
            catches += is_catch as usize;

            // Text for the hall of fame, per the user's retention level. Always redacted.
            // The next reply holds the admission (catch) or the correction (humbled).
            let ai = st.turn.next_ai.as_ref();
            let ai_text = engine::normalize_quotes(ai.map_or("", |m| m.text.as_str()));
            let (human_full, ai_full, human_snip, ai_snip) = match retention.level {
                Level::Category => (None, None, None, None),
                level => {
                    let hs = snippet(&engine::normalize_quotes(st.turn.human.trim()), v.human_span);
                    let as_ = (!ai_text.is_empty()).then(|| snippet(&ai_text, v.ai_span));
                    if level == Level::Full {
                        (Some(engine::redact(st.turn.human.trim())), ai.map(|m| engine::redact(&m.text)), Some(hs), as_)
                    } else {
                        (None, None, Some(hs), as_)
                    }
                }
            };
            tx.execute(
                "INSERT INTO events (ts, day, source, session, provider, family, version, model_raw, label, category, rules,
                    level, human_full, ai_full, human_snip, ai_snip, seen_key)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
                params![
                    st.ts.to_rfc3339(),
                    day_s,
                    source,
                    session,
                    provider,
                    family,
                    version,
                    model.as_ref().map(|m| m.raw.as_str()),
                    label.as_str(),
                    category,
                    v.rules.join(","),
                    retention.level.as_str(),
                    human_full,
                    ai_full,
                    human_snip,
                    ai_snip,
                    key,
                ],
            )?;
        }
        tx.execute(
            "INSERT INTO files (path, source, checkpoint, size, mtime) VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(path) DO UPDATE SET checkpoint = excluded.checkpoint, size = excluded.size, mtime = excluded.mtime",
            params![path, source, scan.checkpoint as i64, size as i64, mtime],
        )?;
        tx.commit()?;
        Ok((messages, catches))
    }

    // ---- retention ---------------------------------------------------------------------

    /// Steps aging events down: full → snippet → category. Pinned events keep their text.
    pub fn sweep(&self, now: DateTime<Utc>) -> Result<usize> {
        let r = self.retention()?;
        let full_cut = (now - Duration::days(r.full_days as i64)).to_rfc3339();
        let snip_cut = (now - Duration::days(r.snippet_days as i64)).to_rfc3339();
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
        self.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        Ok(())
    }

    pub fn pin(&self, event_id: i64, pinned: bool) -> Result<()> {
        self.conn.execute("UPDATE events SET pinned = ?2 WHERE id = ?1", params![event_id, pinned])?;
        Ok(())
    }

    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }
}

#[cfg(test)]
mod tests;
