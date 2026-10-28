use anyhow::Result;
use rusqlite::Connection;

/// Each entry runs once, in order. Never edit a shipped migration; append a new one.
const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);

-- Consent: a source is read only while allowed = 1 and paused = 0.
CREATE TABLE sources (
    id TEXT PRIMARY KEY,
    allowed INTEGER NOT NULL DEFAULT 0,
    paused INTEGER NOT NULL DEFAULT 0,
    backfill_since TEXT,
    granted_at TEXT,
    paths TEXT NOT NULL DEFAULT '[]'
);

-- Where each file was last read up to. No content.
CREATE TABLE files (
    path TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    checkpoint INTEGER NOT NULL,
    size INTEGER NOT NULL,
    mtime INTEGER NOT NULL
);

-- Hashes of message ids already counted, so copies never count twice. No content.
CREATE TABLE seen (key TEXT PRIMARY KEY, source TEXT NOT NULL) WITHOUT ROWID;

-- Counts per local day. kind = 'message' | label. Numbers only.
CREATE TABLE daily (
    day TEXT NOT NULL, source TEXT NOT NULL,
    provider TEXT NOT NULL, family TEXT NOT NULL, version TEXT NOT NULL,
    kind TEXT NOT NULL, category TEXT NOT NULL, n INTEGER NOT NULL,
    PRIMARY KEY (day, source, provider, family, version, kind, category)
) WITHOUT ROWID;

-- One row per chat, id hashed. Numbers only.
CREATE TABLE sessions (
    id TEXT PRIMARY KEY, source TEXT NOT NULL,
    first_day TEXT NOT NULL, last_day TEXT NOT NULL,
    messages INTEGER NOT NULL, catches INTEGER NOT NULL
);

-- Catches and humbled moments. Text columns obey the retention level.
CREATE TABLE events (
    id INTEGER PRIMARY KEY,
    ts TEXT NOT NULL, day TEXT NOT NULL, source TEXT NOT NULL, session TEXT NOT NULL,
    provider TEXT NOT NULL, family TEXT NOT NULL, version TEXT NOT NULL, model_raw TEXT,
    label TEXT NOT NULL, category TEXT NOT NULL, rules TEXT NOT NULL,
    level TEXT NOT NULL,
    human_full TEXT, ai_full TEXT, human_snip TEXT, ai_snip TEXT,
    pinned INTEGER NOT NULL DEFAULT 0,
    seen_key TEXT NOT NULL
);
CREATE INDEX events_day ON events (day);
CREATE INDEX daily_day ON daily (day);
"#];

pub fn migrate(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        conn.execute_batch(&format!("BEGIN; {sql}; PRAGMA user_version = {}; COMMIT;", i + 1))?;
    }
    Ok(())
}
