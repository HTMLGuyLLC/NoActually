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
mod error;
mod files;
mod goals;
mod insights;
mod meta;
pub mod milestones;
mod progress;
mod schema;
mod stats;
mod sync;
mod tips;

pub use error::{Error, Result};
pub use files::{write_atomic, write_new};
pub use goals::{Goal, GoalKind, GoalNews, GoalStatus, GoalWeek, Trophy, WeekState, GOAL_WEEKS};
pub use insights::{Bests, CardTrend, HabitDrop, Insights, ModelLanded};
pub use meta::{meta, CategoryInfo, HabitInfo, Meta, SignalInfo, SourceInfo, TierInfo, Titled, VerdictTierInfo};
pub use progress::{
    floor_pct, HistoryWeek, Progress, RecentReportCard, ReportCardWeek, MIN_CARD_BEST, MIN_CARD_HALF, MIN_CARD_WEEK,
    MIN_WEEK, RECENT_REPORT_CARDS,
};
pub use stats::{Filter, HallEntry, ModelRow, Overall, Prompts, Stats, Tip, WeekRow};
pub use sync::{sync, SyncReport};
pub use tips::{TipActivity, TipTrend, TipWeek, TopTip};

/// Limits the database and its write-ahead log to their owner, and the app's own data folder too
/// (on Unix; elsewhere the user's profile already does it). A folder chosen with [`DB_ENV`] may
/// hold other things, so it isn't touched. Marker files stay world-readable: the hooks read them.
fn protect(db: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let mut name = db.as_os_str().to_owned();
        name.push(suffix);
        files::restrict(Path::new(&name), 0o600);
    }
    if let Some(dir) = db.parent().filter(|d| d.file_name().is_some_and(|n| n == APP_ID)) {
        files::restrict(dir, 0o700);
    }
}

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

/// Records how many report cards are left out of the chat file `path`. Each file keeps its own
/// count, replaced on every full check, so reading a file again (a rebuild, a pass from zero)
/// can't count the same cards twice, and a card that saves fine later no longer counts.
fn set_left_out(conn: &Connection, source: &str, path: &str, n: i64) -> Result<()> {
    let key = format!("{}.{}", left_out_key(source), hash(&[path]));
    if n > 0 {
        put_setting(conn, &key, &n.to_string())
    } else {
        conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
        Ok(())
    }
}

/// Clears every left-out count of `source`, or of all sources with `None`. Matches the key itself
/// (older versions kept one total there) and the per-file keys under it.
fn clear_left_out(conn: &Connection, source: Option<&str>) -> Result<()> {
    let prefix = left_out_key(source.unwrap_or_default());
    let sql = if source.is_some() {
        "DELETE FROM settings WHERE key = ?1 OR substr(key, 1, length(?1) + 1) = ?1 || '.'"
    } else {
        "DELETE FROM settings WHERE substr(key, 1, length(?1)) = ?1"
    };
    conn.execute(sql, [prefix])?;
    Ok(())
}

/// The settings key that counts the report cards left out for `source`. Its per-file counts are
/// under `<key>.<hash of the file>`.
fn left_out_key(source: &str) -> String {
    format!("report_cards.left_out.{source}")
}

use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use engine::{classify, normalize_model, read_prompt, snippet, Label, Prompt};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sources::{ChatView, Kind, Source};
use std::ffi::OsString;
use std::fmt;
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
/// The secret that seals report cards, kept next to the database.
pub const CARD_KEY_FILE: &str = "report-cards.key";

/// The database the CLI and the app share: `$NOACTUALLY_DB`, else `$WA_DB` (deprecated), else
/// `<app data>/app.noactually/noactually.db`.
pub fn default_db_path() -> Result<PathBuf> {
    resolve_db_path(|k| std::env::var_os(k), dirs::data_dir())
}

/// What [`CARD_KEY_FILE`] holds right now.
enum KeyFile {
    Missing,
    /// A file with nothing in it seals nothing, so it counts as unusable.
    Empty,
    Secret(Vec<u8>),
}

fn read_key(path: &Path) -> std::io::Result<KeyFile> {
    match std::fs::read(path) {
        Ok(secret) if secret.is_empty() => Ok(KeyFile::Empty),
        Ok(secret) => Ok(KeyFile::Secret(secret)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(KeyFile::Missing),
        Err(e) => Err(e),
    }
}

/// The secret that seals report cards for the database at `db`: [`CARD_KEY_FILE`] beside it. With
/// `create`, a missing file is made (32 random bytes, owner-only) and an existing one is never
/// overwritten; without it, a missing file is `None`. An empty file seals nothing: `None` without
/// `create`, and replaced with `create`. Only the helper creates it: the app just checks seals, and
/// a card can't be sealed before the helper has run.
///
/// Two first runs can't end with different secrets. The new secret is written whole to a temporary
/// file in the same folder and then hard-linked into place, which fails if the key is already there;
/// whoever loses reads the winner's. A file is never seen half written.
pub fn card_secret(db: &Path, create: bool) -> Result<Option<Vec<u8>>> {
    let path = db.with_file_name(CARD_KEY_FILE);
    let io = |source| Error::Io { path: path.clone(), source };
    let mut replace = match read_key(&path).map_err(io)? {
        KeyFile::Secret(secret) => return Ok(Some(secret)),
        _ if !create => return Ok(None),
        state => matches!(state, KeyFile::Empty),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let mut secret = vec![0u8; 32];
    getrandom::fill(&mut secret).map_err(|e| io(std::io::Error::other(e.to_string())))?;
    let tmp = files::temp_sibling(&path).map_err(io)?;
    write_new(&tmp, &secret, 0o600).map_err(io)?;
    let placed = (|| {
        // Twice at most: the second try is for an empty file that turned up between the read and the link.
        let mut tries = 0;
        loop {
            if let KeyFile::Secret(winner) = read_key(&path)? {
                return Ok(winner); // someone made it in the meantime: never replace a real key
            }
            let put = if replace { std::fs::rename(&tmp, &path) } else { std::fs::hard_link(&tmp, &path) };
            match put {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && !replace => {}
                Err(e) => return Err(e),
            }
            match read_key(&path)? {
                KeyFile::Secret(winner) => return Ok(winner),
                _ => replace = true,
            }
            tries += 1;
            if tries == 2 {
                return Err(std::io::Error::other("the report card key stayed empty"));
            }
        }
    })();
    let _ = std::fs::remove_file(&tmp); // gone already when it was renamed into place
    placed.map(Some).map_err(io)
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
/// `report_cards` is not one: a rebuild keeps it (Claude Code deletes old transcripts, so those rows
/// are the history), and reading a card again writes the same row.
const SCORE_TABLES: [&str; 8] = ["events", "daily", "sessions", "tasks", "files", "seen", "tips", "tip_chats"];
/// The score tables a rebuild sets aside in `kept_*`. `files` isn't one: its checkpoints have to go.
const KEPT_TABLES: [&str; 7] = ["events", "daily", "sessions", "tasks", "seen", "tips", "tip_chats"];
/// The setting that says a rebuild has set scores aside and hasn't put back what it couldn't read.
const REBUILDING_KEY: &str = "rules_rebuild.pending";
/// The [`engine::REDACTION_VERSION`] stored text was last cleaned with.
const REDACTION_KEY: &str = "redaction_version";
/// The setting that says which [`engine::RULES_VERSION`] the scores were counted under.
pub const RULES_KEY: &str = "rules_version";
/// Kept beside the database. Deleting text removes it, and the next pass writes a new one.
pub const BACKUP_FILE: &str = "noactually-backup.db";
const BACKUP_EVERY: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Shown when a message's chat never recorded a model (a session with no real reply).
pub const NO_MODEL: &str = "No model recorded";

/// The local database.
pub struct Store {
    conn: Connection,
}

pub(crate) use engine::key_hash as hash;

/// One read of a file: what it looked like, and what an earlier read already did with it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pass {
    pub meta: FileMeta,
    /// The pass that wrote this checkpoint already counted a trailing unanswered message, so an
    /// unchanged file needs no more reads even though its checkpoint is short of its end.
    pub finalized: bool,
    /// The last report card position in this file that was read and checked, or `None` if none was
    /// (also after a rewrite, which starts over). A card at or before it is already stored, so the
    /// whole chat isn't read again for it.
    pub cards_to: Option<u64>,
}

impl Pass {
    pub fn new(meta: FileMeta, finalized: bool) -> Self {
        Self { meta, finalized, cards_to: None }
    }
}

/// Where a file was read up to, and what it looked like then.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Checkpoint {
    /// Where the next pass resumes.
    pub at: u64,
    /// The file's size and mtime when it was read.
    pub meta: FileMeta,
    /// That pass was final (see [`Pass::finalized`]).
    pub finalized: bool,
    /// The last report card position read (see [`Pass::cards_to`]).
    pub cards_to: Option<u64>,
}

fn upsert_file(conn: &Connection, path: &str, source: &str, checkpoint: u64, pass: Pass) -> Result<()> {
    conn.execute(
        "INSERT INTO files (path, source, checkpoint, size, mtime, finalized, cards_to) VALUES (?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(path) DO UPDATE SET checkpoint = excluded.checkpoint, size = excluded.size,
           mtime = excluded.mtime, finalized = excluded.finalized, cards_to = excluded.cards_to",
        params![
            path,
            source,
            checkpoint as i64,
            pass.meta.size as i64,
            pass.meta.mtime,
            pass.finalized,
            pass.cards_to.map(|p| p as i64)
        ],
    )?;
    Ok(())
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

    // ---- settings ----------------------------------------------------------------------

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

    /// Small app-owned values (milestones already announced, onboarding done, …).
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    /// Sets an app-owned value.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        put_setting(&self.conn, key, value)
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
    fn refresh_consent_markers(&self) -> Result<()> {
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

    /// Sets every score aside and clears every checkpoint so history is re-read under new rules.
    /// Consent, settings and pinned corrections stay: a pin is often the only copy left once its
    /// history file is gone. [`Store::finish_rebuild`] then puts back the chats that couldn't be read.
    pub fn reset_scores(&mut self) -> Result<()> {
        self.finish_rebuild()?; // one that was cut short
        let tx = self.conn.transaction()?;
        for table in KEPT_TABLES {
            let keep = if table == "events" { " WHERE pinned = 0" } else { "" };
            tx.execute(&format!("INSERT INTO kept_{table} SELECT * FROM {table}{keep}"), [])?;
        }
        for table in SCORE_TABLES {
            let keep = if table == "events" { " WHERE pinned = 0" } else { "" };
            tx.execute(&format!("DELETE FROM {table}{keep}"), [])?;
        }
        // The rebuild reads every chat from zero and counts what it leaves out again.
        clear_left_out(&tx, None)?;
        put_setting(&tx, REBUILDING_KEY, "1")?;
        tx.commit()?;
        Ok(())
    }

    /// Remembers that the file as it stands now can't be read, so it isn't tried on every pass.
    pub(crate) fn remember_unreadable(&self, path: &str, source: &str, meta: FileMeta) {
        let pass = Pass { meta, finalized: true, cards_to: None };
        if let Err(e) = upsert_file(&self.conn, path, source, 0, pass) {
            log::warn!("couldn't note an unreadable file: {e}");
        }
    }

    /// Whether `id` is still allowed. `noactually forget` from another process doesn't wait for a
    /// running pass, which holds what it read in memory: this stops the pass writing it back.
    pub(crate) fn still_allowed(&self, id: &str) -> Result<bool> {
        Ok(self.source_state(id)?.is_some_and(|s| s.allowed))
    }

    /// The database's format version, for the log.
    pub fn schema_version(&self) -> Result<usize> {
        Ok(self.conn.pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    /// Whether a rebuild was cut short and hasn't finished yet.
    pub fn rebuild_pending(&self) -> Result<bool> {
        Ok(self.get_setting(REBUILDING_KEY)?.is_some())
    }

    /// Whether the next pass reads history again, so the numbers until then are partial.
    pub fn rebuild_due(&self) -> Result<bool> {
        let current = self.scores_current()?;
        let had_scores: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM daily)", [], |r| r.get(0))?;
        Ok(self.rebuild_pending()? || (!current && had_scores))
    }

    /// Whether the scores were counted under these rules, or newer ones.
    fn scores_current(&self) -> Result<bool> {
        let stored = self.get_setting(RULES_KEY)?;
        Ok(stored.as_deref().and_then(|v| v.parse::<u32>().ok()).is_some_and(|v| v >= engine::RULES_VERSION))
    }

    /// Ends a rebuild once history has been read again. A chat that wasn't read again comes back as
    /// it was counted: its file is gone, or its source is stopped. Counts per day have no chat on
    /// them, so a day with nothing read again comes back whole, and a day that a missing chat
    /// touched keeps the larger message count and adds back that chat's corrections. Other days take
    /// the new count. Message keys of missing chats come back too, so a file that returns can't
    /// count twice. A correction that a fork already brought back isn't restored a second time.
    pub(crate) fn finish_rebuild(&mut self) -> Result<()> {
        const UNREAD: &str = "k.session NOT IN (SELECT id FROM temp.read_chats)";
        const READ_DAY: &str = "EXISTS (SELECT 1 FROM temp.read_days r WHERE r.source = k.source AND r.day = k.day)";
        const MISSING_CHAT_DAY: &str = "EXISTS (SELECT 1 FROM kept_sessions s WHERE s.source = k.source
            AND k.day BETWEEN s.first_day AND s.last_day AND s.id NOT IN (SELECT id FROM temp.read_chats))";
        // `seen` holds only what this rebuild read until the kept keys come back below.
        const NOT_BACK: &str = "NOT EXISTS (SELECT 1 FROM seen s WHERE s.key = k.seen_key)";
        if self.get_setting(REBUILDING_KEY)?.is_none() {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        tx.execute_batch(
            "CREATE TEMP TABLE read_chats AS SELECT id FROM rebuild_read UNION SELECT id FROM sessions;
             CREATE TEMP TABLE read_days AS SELECT DISTINCT source, day FROM daily;",
        )?;
        tx.execute(
            &format!(
                "INSERT INTO daily SELECT k.day, k.source, k.provider, k.family, k.version, k.label, k.category, count(*)
                 FROM (SELECT * FROM kept_events UNION ALL SELECT * FROM events WHERE pinned = 1) k
                 WHERE {UNREAD} AND {READ_DAY} AND {NOT_BACK}
                 GROUP BY k.day, k.source, k.provider, k.family, k.version, k.label, k.category
                 ON CONFLICT (day, source, provider, family, version, kind, category) DO UPDATE SET n = n + excluded.n"
            ),
            [],
        )?;
        tx.execute(
            &format!(
                "INSERT INTO daily SELECT k.* FROM kept_daily k WHERE k.kind = 'message' AND {READ_DAY} AND {MISSING_CHAT_DAY}
                 ON CONFLICT (day, source, provider, family, version, kind, category) DO UPDATE SET n = max(n, excluded.n)"
            ),
            [],
        )?;
        tx.execute(&format!("INSERT INTO daily SELECT k.* FROM kept_daily k WHERE NOT {READ_DAY}"), [])?;
        tx.execute(
            &format!(
                "INSERT INTO events SELECT NULL, ts, day, source, session, provider, family, version,
                   label, category, level, human_full, ai_full, human_snip, ai_snip, pinned, seen_key
                 FROM kept_events k WHERE {UNREAD} AND {NOT_BACK}"
            ),
            [],
        )?;
        tx.execute(
            &format!(
                "INSERT INTO tasks SELECT NULL, day, source, session, provider, family, version, signals,
                   messages, ai_wrong, added, clarified, redo
                 FROM kept_tasks k WHERE {UNREAD}"
            ),
            [],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO sessions SELECT k.* FROM kept_sessions k
             WHERE k.id NOT IN (SELECT id FROM temp.read_chats)",
            [],
        )?;
        tx.execute(&format!("INSERT OR IGNORE INTO tips SELECT k.* FROM kept_tips k WHERE {UNREAD}"), [])?;
        tx.execute(&format!("INSERT OR IGNORE INTO tip_chats SELECT k.* FROM kept_tip_chats k WHERE {UNREAD}"), [])?;
        tx.execute(
            "INSERT OR IGNORE INTO seen SELECT k.* FROM kept_seen k
             WHERE k.session IS NULL OR k.session NOT IN (SELECT id FROM temp.read_chats)",
            [],
        )?;
        tx.execute_batch("DROP TABLE temp.read_chats; DROP TABLE temp.read_days;")?;
        for table in KEPT_TABLES {
            tx.execute(&format!("DELETE FROM kept_{table}"), [])?;
        }
        tx.execute("DELETE FROM rebuild_read", [])?;
        tx.execute("DELETE FROM settings WHERE key = ?1", [REBUILDING_KEY])?;
        tx.commit()?;
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

    /// Rebuilds scores once if they were counted under older rules. Returns true if it did. Scores
    /// from newer rules stay: an older build can't read them better.
    pub fn ensure_rules_current(&mut self) -> Result<bool> {
        if self.scores_current()? {
            return Ok(false);
        }
        // Only counted scores need rebuilding. A store with checkpoints but no counts yet (say,
        // a source allowed "from now on") must keep its checkpoints.
        let had_scores: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM daily)", [], |r| r.get(0))?;
        if had_scores {
            self.reset_scores()?;
        }
        self.set_setting(RULES_KEY, &engine::RULES_VERSION.to_string())?;
        Ok(had_scores)
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

    /// Report cards the app couldn't count, for one source: the person got a grade in chat, so they
    /// should hear why it isn't here.
    pub fn report_cards_left_out(&self, source: &str) -> Result<i64> {
        let key = left_out_key(source);
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(CAST(value AS INTEGER)), 0) FROM settings
                 WHERE key = ?1 OR substr(key, 1, length(?1) + 1) = ?1 || '.'",
            [key],
            |r| r.get(0),
        )?)
    }

    /// Deletes every report card and tip. They stay deleted: a rebuild that reads the same transcripts
    /// again won't bring back anything from before now. Returns how many report cards and tips went.
    pub fn delete_report_cards(&self) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let n = tx.execute("DELETE FROM report_cards", [])?;
        let tips = tx.execute("DELETE FROM tips", [])?;
        tx.execute("DELETE FROM tip_chats", [])?;
        tx.execute("DELETE FROM kept_tips", [])?;
        tx.execute("DELETE FROM kept_tip_chats", [])?;
        clear_left_out(&tx, None)?;
        put_setting(&tx, cards::CLEARED_KEY, &cards::ts_str(Utc::now()))?;
        tx.commit()?;
        self.flush_deleted()?;
        Ok(n + tips)
    }

    // ---- ingestion ---------------------------------------------------------------------

    /// Where `path` was last read: see [`Checkpoint`].
    pub(crate) fn file_checkpoint(&self, path: &str) -> Result<Option<Checkpoint>> {
        Ok(self
            .conn
            .query_row("SELECT checkpoint, size, mtime, finalized, cards_to FROM files WHERE path = ?1", [path], |r| {
                Ok(Checkpoint {
                    at: r.get::<_, i64>(0)? as u64,
                    meta: FileMeta { size: r.get::<_, i64>(1)? as u64, mtime: r.get(2)? },
                    finalized: r.get(3)?,
                    cards_to: r.get::<_, Option<i64>>(4)?.map(|p| p as u64),
                })
            })
            .optional()?)
    }

    /// Writes one file's turns and its new checkpoint in a single transaction, so a crash
    /// can never count a turn without moving the checkpoint (or the reverse). Turns the
    /// source's consent skips are left out entirely, so allowing more history later still
    /// counts them. Returns (messages, corrections) counted.
    ///
    /// `chat` is the whole file read again; it's only needed (and only given) when the scan found
    /// report cards past `pass.cards_to`, which are checked against it and saved in the same
    /// transaction.
    pub(crate) fn ingest(
        &mut self,
        state: &SourceState,
        path: &str,
        scan: &sources::FileScan,
        chat: Option<&ChatView>,
        mut pass: Pass,
    ) -> Result<(usize, usize)> {
        let source = state.id.as_str();
        let retention = self.retention()?;
        let graded = self.read_cards(state, scan, chat, pass.cards_to)?;
        let has_tips = !scan.tips.is_empty() || !scan.rules.is_empty();
        let cleared = if has_tips { self.get_setting(cards::CLEARED_KEY)? } else { None };
        let mut checkpoint = scan.checkpoint;
        if graded.unchecked {
            // Cards that couldn't be checked (no secret yet) are not remembered as read, and the file
            // resumes at the first of them, not past it: a following whole turn would otherwise move
            // the checkpoint beyond the card and no later pass would see it. Turns read twice are `seen`.
            if let Some(first) = cards::first_unread(&scan.cards, pass.cards_to) {
                checkpoint = checkpoint.min(first);
            }
        } else {
            pass.cards_to = pass.cards_to.max(scan.cards.iter().map(|c| c.pos).max());
        }
        let rebuilding = self.rebuild_pending()?;
        let tx = self.conn.transaction()?;
        let (mut messages, mut corrections) = (0, 0);
        for st in scan.turns.iter().filter(|st| state.wants(st.ts)) {
            let session = hash(&[source, &st.session]);
            if rebuilding {
                tx.execute(
                    "INSERT OR IGNORE INTO rebuild_read (id, source) VALUES (?1, ?2)",
                    params![session, source],
                )?;
            }
            let key = engine::message_key(source, &st.session, st.key.as_deref(), &st.ts.to_rfc3339(), &st.turn.human);
            let new = "INSERT OR IGNORE INTO seen (key, source, session) VALUES (?1, ?2, ?3)";
            if tx.execute(new, params![key, source, session])? == 0 {
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
            let is_correction = v.label.is_some_and(Label::is_correction);
            tx.execute(
                "INSERT INTO sessions (id, source, first_day, last_day, messages, corrections) VALUES (?1,?2,?3,?3,1,?4)
                 ON CONFLICT(id) DO UPDATE SET messages = messages + 1, corrections = corrections + excluded.corrections,
                   first_day = min(first_day, excluded.first_day), last_day = max(last_day, excluded.last_day)",
                params![session, source, day, i64::from(is_correction)],
            )?;
            count_prompt(&tx, &st.turn, &v, &session, [day.as_str(), source, provider, family, version])?;
            let Some(label) = v.label else { continue };
            let category = v.category.map_or("", engine::Category::as_str);
            bump(label.as_str(), category)?;
            corrections += usize::from(is_correction);

            let level = if rebuilding { level_after_rebuild(&tx, &key, retention.level)? } else { retention.level };
            let text = EventText::new(&st.turn, &v, level);
            // A pinned correction kept through a rules rebuild is already here; keep it as pinned.
            tx.execute(
                "INSERT INTO events (ts, day, source, session, provider, family, version, label, category,
                    level, human_full, ai_full, human_snip, ai_snip, seen_key)
                 SELECT ?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15
                 WHERE NOT EXISTS (SELECT 1 FROM events WHERE seen_key = ?15)",
                params![
                    st.ts.to_rfc3339(),
                    day,
                    source,
                    session,
                    provider,
                    family,
                    version,
                    label.as_str(),
                    category,
                    level.as_str(),
                    text.human_full,
                    text.ai_full,
                    text.human_snip,
                    text.ai_snip,
                    key,
                ],
            )?;
        }
        cards::save(&tx, &graded.rows)?;
        tips::save(&tx, state, scan, cleared.as_deref())?;
        // Only a full check knows the file's total. Cards that couldn't be checked yet are tried
        // again, so they aren't counted as left out.
        if graded.checked {
            set_left_out(&tx, source, path, graded.left_out())?;
        }
        upsert_file(&tx, path, source, checkpoint, pass)?;
        tx.commit()?;
        graded.log(source, path);
        Ok((messages, corrections))
    }

    /// The report cards in `scan` that check out against `chat`. The secret that seals them is read
    /// from its file each time (this is rare): the helper may have made it after the app started,
    /// and a missing, unreadable or empty one means no card can be trusted, never a failed sync.
    fn read_cards(
        &self,
        state: &SourceState,
        scan: &sources::FileScan,
        chat: Option<&ChatView>,
        cards_to: Option<u64>,
    ) -> Result<cards::Cards> {
        let mut out = cards::Cards::default();
        if !cards::unread(&scan.cards, cards_to) {
            return Ok(out);
        }
        let secret = match self.conn.path().filter(|p| !p.is_empty()) {
            Some(db) => card_secret(Path::new(db), false).unwrap_or_else(|e| {
                log::warn!("couldn't read the report card key: {e}");
                None
            }),
            None => None, // an in-memory database has no folder to keep one in
        };
        match (chat, secret) {
            (Some(chat), Some(secret)) => {
                let cleared = self.get_setting(cards::CLEARED_KEY)?;
                out = cards::read(state, chat, &secret, cleared.as_deref());
            }
            (None, _) => {
                out.skip("chat unreadable", scan.cards.len());
                out.unchecked = true;
            }
            (_, None) => {
                out.skip("no secret", scan.cards.len());
                out.unchecked = true;
            }
        }
        Ok(out)
    }

    // ---- retention ---------------------------------------------------------------------

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

/// The text kept for one event at a retention level. Always redacted. The next reply holds the
/// AI's admission (for a correction) or its pushback (humbled).
struct EventText {
    human_full: Option<String>,
    ai_full: Option<String>,
    human_snip: Option<String>,
    ai_snip: Option<String>,
}

/// Prompts: a new task gets a row; steering and other replies count toward the chat's latest.
/// `row` is the day, source, provider, family and version a new task is filed under.
fn count_prompt(
    tx: &rusqlite::Transaction<'_>,
    turn: &engine::Turn,
    v: &engine::Verdict,
    session: &str,
    row: [&str; 5],
) -> Result<()> {
    let latest = "id = (SELECT MAX(id) FROM tasks WHERE session = ?1)";
    let has_task: bool =
        tx.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE session = ?1)", [session], |r| r.get(0))?;
    match read_prompt(turn, v, has_task) {
        Prompt::Task { signals, .. } => {
            let [day, source, provider, family, version] = row;
            tx.execute(
                "INSERT INTO tasks (day, source, session, provider, family, version, signals) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![day, source, session, provider, family, version, signals.0],
            )?
        }
        Prompt::Steer { cause, .. } => tx.execute(
            &format!("UPDATE tasks SET messages = messages + 1, {c} = {c} + 1 WHERE {latest}", c = cause.as_str()),
            [session],
        )?,
        Prompt::Other => tx.execute(&format!("UPDATE tasks SET messages = messages + 1 WHERE {latest}"), [session])?,
    };
    Ok(())
}

/// A rebuild never keeps more text than the correction had: levels only step down.
fn level_after_rebuild(tx: &rusqlite::Transaction<'_>, key: &str, level: Level) -> Result<Level> {
    let kept: Option<String> =
        tx.query_row("SELECT level FROM kept_events WHERE seen_key = ?1", [key], |r| r.get(0)).optional()?;
    Ok(kept.and_then(|l| l.parse().ok()).map_or(level, |l: Level| l.at_most(level)))
}

impl EventText {
    fn new(turn: &engine::Turn, v: &engine::Verdict, level: Level) -> Self {
        let mut text = EventText { human_full: None, ai_full: None, human_snip: None, ai_snip: None };
        if level == Level::Category {
            return text;
        }
        // The span points into the message that held the admission.
        let ai = match v.ai_later {
            Some(i) => turn.later_ai.get(i).map(String::as_str),
            None => turn.next_ai.as_ref().map(|m| m.text.as_str()),
        };
        let ai_norm = engine::normalize_quotes(ai.unwrap_or(""));
        text.human_snip = Some(snippet(&engine::normalize_quotes(turn.human.trim()), v.human_span));
        text.ai_snip = (!ai_norm.is_empty()).then(|| snippet(&ai_norm, v.ai_span));
        if level == Level::Full {
            text.human_full = Some(engine::redact(turn.human.trim()));
            text.ai_full = ai.map(engine::redact);
        }
        text
    }
}

#[cfg(test)]
mod card_tests;
#[cfg(test)]
mod modes_tests;
#[cfg(test)]
mod tests;
