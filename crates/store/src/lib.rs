//! The local database. Everything the app knows lives here, and only here.
//!
//! [`Store`] owns one SQLite connection: consent and settings, per-file checkpoints, daily
//! counts, and catch events whose text obeys the [`Retention`] level. [`sync`] reads whatever
//! changed in the allowed sources; [`Store::stats`] answers every view; [`milestones`] decides
//! which moments deserve a popup.
//!
//! Tests: small pure helpers keep their tests inline in `mod tests`; the store's behavior suite
//! (ingest, sync, retention, stats) lives in `src/tests.rs` because it drives the whole crate.

#![warn(missing_docs)]

mod error;
mod meta;
pub mod milestones;
mod schema;
mod stats;
mod sync;

pub use error::{Error, Result};
pub use meta::{meta, BriefTierInfo, CategoryInfo, Meta, SignalInfo, SourceInfo, TierInfo, Titled};
pub use stats::{Briefs, Filter, HallEntry, ModelRow, Stats, Tip, WeekRow};
pub use sync::{sync, SyncReport};

use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use engine::{classify, normalize_model, read_prompt, snippet, Label, Prompt};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sources::{Kind, Source};
use std::ffi::OsString;
use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

/// The desktop app's bundle identifier; its data folder is named after it.
pub const APP_ID: &str = "app.noactually";
/// The database file inside the data folder.
pub const DB_FILE: &str = "noactually.db";
/// Environment variable that points the CLI and the app at another database.
pub const DB_ENV: &str = "NOACTUALLY_DB";
/// The variable's old name. Still read, after [`DB_ENV`]; deprecated.
pub const LEGACY_DB_ENV: &str = "WA_DB";

/// The database the CLI and the app share: `$NOACTUALLY_DB`, else `$WA_DB` (deprecated), else
/// `<app data>/app.noactually/noactually.db`.
pub fn default_db_path() -> Result<PathBuf> {
    resolve_db_path(|k| std::env::var_os(k), dirs::data_dir())
}

/// True when only the deprecated [`LEGACY_DB_ENV`] is set, so callers can say so.
pub fn legacy_db_env_in_use() -> bool {
    let set = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
    !set(DB_ENV) && set(LEGACY_DB_ENV)
}

fn resolve_db_path(env: impl Fn(&str) -> Option<OsString>, data_dir: Option<PathBuf>) -> Result<PathBuf> {
    // An empty variable counts as unset, so `NOACTUALLY_DB= cmd` can't silently mean "here".
    let var = |k: &str| env(k).filter(|v| !v.is_empty());
    if let Some(p) = var(DB_ENV).or_else(|| var(LEGACY_DB_ENV)) {
        return Ok(p.into());
    }
    // Never a relative path: that would put the database wherever the command happened to run.
    let dir = data_dir.filter(|d| d.is_absolute()).ok_or(Error::NoDataDir)?;
    Ok(dir.join(APP_ID).join(DB_FILE))
}

/// How much catch text to keep. Levels only ever step down as events age.
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

/// How long catch text is kept, and at what level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Retention {
    /// The level new catches start at.
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
    /// Both periods must be at least a day.
    pub fn validate(&self) -> Result<()> {
        if self.full_days == 0 || self.snippet_days == 0 {
            return Err(Error::Invalid("Keep text for at least 1 day."));
        }
        Ok(())
    }
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
    fn wants(&self, ts: DateTime<Utc>) -> bool {
        let day = ts.with_timezone(&Local).date_naive();
        !(self.backfill_since.is_some_and(|s| day < s) || self.start_after.is_some_and(|t| ts < t))
    }
}

/// A file's size and modified time, as last read. A change in either means it changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct FileMeta {
    pub size: u64,
    /// Nanoseconds since the Unix epoch, so two writes in the same second still differ. Older
    /// stores hold seconds here; those never match, so the first sync after upgrading re-checks
    /// each file once (append-only files resume at their checkpoint; whole files are re-read
    /// and the `seen` keys block double counts).
    pub mtime: i64,
}

impl FileMeta {
    /// The file's current size and mtime, plus the raw modified time.
    pub fn read(path: &Path) -> std::io::Result<(FileMeta, SystemTime)> {
        let meta = std::fs::metadata(path)?;
        let modified = meta.modified().unwrap_or(UNIX_EPOCH);
        let mtime = modified.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos() as i64);
        Ok((FileMeta { size: meta.len(), mtime }, modified))
    }
}

/// Every history file a source covers: its live roots, plus the files and folders the user
/// added (exports).
pub(crate) fn source_files(src: &dyn Source, extra: &[String]) -> Vec<PathBuf> {
    let mut roots = if src.kind() == Kind::Live { src.roots() } else { Vec::new() };
    roots.extend(extra.iter().map(PathBuf::from));
    roots.into_iter().flat_map(|root| if root.is_file() { vec![root] } else { src.files(&root) }).collect()
}

/// How often to retry a busy log truncation, and how long to wait between tries.
const CHECKPOINT_TRIES: u32 = 5;
const CHECKPOINT_WAIT: std::time::Duration = std::time::Duration::from_millis(100);

/// Tables that hold scores (everything but consent and settings). Each has a `source` column.
const SCORE_TABLES: [&str; 6] = ["events", "daily", "sessions", "tasks", "files", "seen"];

/// Shown when a message's chat never recorded a model (a session with no real reply).
pub const NO_MODEL: &str = "No model recorded";

/// The local database.
pub struct Store {
    conn: Connection,
}

pub(crate) fn hash(parts: &[&str]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p.as_bytes());
        h.update([0]);
    }
    h.finalize().iter().take(12).fold(String::with_capacity(24), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn upsert_file(conn: &Connection, path: &str, source: &str, checkpoint: u64, meta: FileMeta) -> Result<()> {
    conn.execute(
        "INSERT INTO files (path, source, checkpoint, size, mtime) VALUES (?1,?2,?3,?4,?5)
         ON CONFLICT(path) DO UPDATE SET checkpoint = excluded.checkpoint, size = excluded.size, mtime = excluded.mtime",
        params![path, source, checkpoint as i64, meta.size as i64, meta.mtime],
    )?;
    Ok(())
}

impl Store {
    /// Opens (or creates) the database at `path`, creating its folder and migrating it.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|source| Error::Io { path: dir.to_path_buf(), source })?;
        }
        let mut conn = Connection::open(path)?;
        // The app keeps a reader and a writer on the same file; wait instead of failing on a lock.
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        schema::migrate(&mut conn)?;
        Ok(Self { conn })
    }

    /// A throwaway database, for tests and demos.
    pub fn open_in_memory() -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        schema::migrate(&mut conn)?;
        Ok(Self { conn })
    }

    // ---- settings ----------------------------------------------------------------------

    /// The retention settings, or the defaults if never set. A saved value that can't be read
    /// (say, written by a newer version) means new catches keep no text, never the defaults,
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
                    "the saved retention setting is unreadable ({e}); new catches keep no text until it is set again"
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

    /// Small app-owned values (milestones already announced, onboarding done, …).
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    /// Sets an app-owned value.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    // ---- consent -----------------------------------------------------------------------

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
    pub fn start_from_now(&mut self, src: &dyn Source) -> Result<usize> {
        // The cutoff is what guarantees it; the checkpoints just save re-reading old bytes.
        self.conn
            .execute("UPDATE sources SET start_after = ?2 WHERE id = ?1", params![src.id(), Utc::now().to_rfc3339()])?;
        let extra = self.source_state(src.id())?.map(|s| s.paths).unwrap_or_default();
        let tx = self.conn.transaction()?;
        let mut marked = 0;
        for path in source_files(src, &extra) {
            let Ok((meta, _)) = FileMeta::read(&path) else { continue };
            upsert_file(&tx, &path.to_string_lossy(), src.id(), meta.size, meta)?;
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
        Ok(())
    }

    /// Stop reading. Keeps the scores already counted.
    pub fn revoke(&self, id: &str) -> Result<()> {
        self.conn.execute("UPDATE sources SET allowed = 0 WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Clears every score and checkpoint so history is re-read under new rules. Consent,
    /// settings and pinned catches stay: a pin is often the only copy left once its history file
    /// is gone. Other scores only come back for history files that still exist.
    pub fn reset_scores(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        for table in SCORE_TABLES {
            let keep = if table == "events" { " WHERE pinned = 0" } else { "" };
            tx.execute(&format!("DELETE FROM {table}{keep}"), [])?;
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
        for table in SCORE_TABLES {
            tx.execute(&format!("DELETE FROM {table} WHERE source = ?1"), [id])?;
        }
        tx.execute("DELETE FROM sources WHERE id = ?1", [id])?;
        tx.commit()?;
        self.flush_deleted()
    }

    // ---- ingestion ---------------------------------------------------------------------

    /// Where `path` was last read up to, and its size and mtime then.
    pub(crate) fn file_checkpoint(&self, path: &str) -> Result<Option<(u64, FileMeta)>> {
        Ok(self
            .conn
            .query_row("SELECT checkpoint, size, mtime FROM files WHERE path = ?1", [path], |r| {
                Ok((r.get::<_, i64>(0)? as u64, FileMeta { size: r.get::<_, i64>(1)? as u64, mtime: r.get(2)? }))
            })
            .optional()?)
    }

    /// Writes one file's turns and its new checkpoint in a single transaction, so a crash
    /// can never count a turn without moving the checkpoint (or the reverse). Turns the
    /// source's consent skips are left out entirely, so allowing more history later still
    /// counts them. Returns (messages, catches) counted.
    pub(crate) fn ingest(
        &mut self,
        state: &SourceState,
        path: &str,
        scan: &sources::FileScan,
        meta: FileMeta,
    ) -> Result<(usize, usize)> {
        let source = state.id.as_str();
        let retention = self.retention()?;
        let tx = self.conn.transaction()?;
        let (mut messages, mut catches) = (0, 0);
        for st in scan.turns.iter().filter(|st| state.wants(st.ts)) {
            let human_head = engine::head_chars(&st.turn.human, 200);
            let key = match &st.key {
                Some(k) => hash(&[source, k]),
                None => hash(&[source, &st.session, &st.ts.to_rfc3339(), human_head]),
            };
            if tx.execute("INSERT OR IGNORE INTO seen (key, source) VALUES (?1, ?2)", params![key, source])? == 0 {
                continue; // a copy we've already counted (resumed or forked session, re-import)
            }
            let v = classify(&st.turn);
            let model = v.blamed_model(&st.turn).and_then(normalize_model);
            let (provider, family, version) = model
                .as_ref()
                .map_or((NO_MODEL, NO_MODEL, ""), |m| (m.provider.as_str(), m.family.as_str(), m.version.as_str()));
            let day = st.ts.with_timezone(&Local).date_naive().to_string();
            let bump = |kind: &str, category: &str| {
                tx.execute(
                    "INSERT INTO daily (day, source, provider, family, version, kind, category, n) VALUES (?1,?2,?3,?4,?5,?6,?7,1)
                     ON CONFLICT DO UPDATE SET n = n + 1",
                    params![day, source, provider, family, version, kind, category],
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
                params![session, source, day, i64::from(is_catch)],
            )?;
            // Briefs: a new task gets a row; steering and other replies count toward the chat's latest.
            let latest = "id = (SELECT MAX(id) FROM tasks WHERE session = ?1)";
            let has_task: bool =
                tx.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE session = ?1)", [&session], |r| r.get(0))?;
            match read_prompt(&st.turn, &v, has_task) {
                Prompt::Brief { signals, .. } => tx.execute(
                    "INSERT INTO tasks (day, source, session, provider, family, version, signals) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    params![day, source, session, provider, family, version, signals.0],
                )?,
                Prompt::Steer { cause, .. } => tx.execute(
                    &format!("UPDATE tasks SET messages = messages + 1, {c} = {c} + 1 WHERE {latest}", c = cause.as_str()),
                    [&session],
                )?,
                Prompt::Other => tx.execute(&format!("UPDATE tasks SET messages = messages + 1 WHERE {latest}"), [&session])?,
            };
            let Some(label) = v.label else { continue };
            let category = v.category.map_or("", engine::Category::as_str);
            bump(label.as_str(), category)?;
            catches += usize::from(is_catch);

            let text = EventText::new(&st.turn, &v, retention.level);
            // A pinned catch kept through a rules rebuild is already here; keep it as pinned.
            tx.execute(
                "INSERT INTO events (ts, day, source, session, provider, family, version, model_raw, label, category, rules,
                    level, human_full, ai_full, human_snip, ai_snip, seen_key)
                 SELECT ?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17
                 WHERE NOT EXISTS (SELECT 1 FROM events WHERE seen_key = ?17)",
                params![
                    st.ts.to_rfc3339(),
                    day,
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
                    text.human_full,
                    text.ai_full,
                    text.human_snip,
                    text.ai_snip,
                    key,
                ],
            )?;
        }
        upsert_file(&tx, path, source, scan.checkpoint, meta)?;
        tx.commit()?;
        Ok((messages, catches))
    }

    // ---- retention ---------------------------------------------------------------------

    /// Steps aging events down: full → snippet → category. Pinned events keep their text.
    /// An unreadable retention setting skips the sweep: deleting text can't be undone, so it
    /// waits until the user's choice is known again.
    pub fn sweep(&self, now: DateTime<Utc>) -> Result<usize> {
        let Some(r) = self.saved_retention()? else { return Ok(0) };
        let full_cut = (now - Duration::days(i64::from(r.full_days))).to_rfc3339();
        let snip_cut = (now - Duration::days(i64::from(r.snippet_days))).to_rfc3339();
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

    /// Pins (or unpins) a catch, keeping its text past the retention period.
    pub fn pin(&self, event_id: i64, pinned: bool) -> Result<()> {
        self.conn.execute("UPDATE events SET pinned = ?2 WHERE id = ?1", params![event_id, pinned])?;
        Ok(())
    }

    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }
}

/// The text kept for one event at a retention level. Always redacted. The next reply holds the
/// admission (catch) or the correction (humbled).
struct EventText {
    human_full: Option<String>,
    ai_full: Option<String>,
    human_snip: Option<String>,
    ai_snip: Option<String>,
}

impl EventText {
    fn new(turn: &engine::Turn, v: &engine::Verdict, level: Level) -> Self {
        let mut text = EventText { human_full: None, ai_full: None, human_snip: None, ai_snip: None };
        if level == Level::Category {
            return text;
        }
        let ai = turn.next_ai.as_ref();
        let ai_norm = engine::normalize_quotes(ai.map_or("", |m| m.text.as_str()));
        text.human_snip = Some(snippet(&engine::normalize_quotes(turn.human.trim()), v.human_span));
        text.ai_snip = (!ai_norm.is_empty()).then(|| snippet(&ai_norm, v.ai_span));
        if level == Level::Full {
            text.human_full = Some(engine::redact(turn.human.trim()));
            text.ai_full = ai.map(|m| engine::redact(&m.text));
        }
        text
    }
}

#[cfg(test)]
mod tests;
