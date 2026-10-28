use crate::Result;
use rusqlite::Connection;

/// Each entry runs once, in order. Never edit a shipped migration; append a new one.
const MIGRATIONS: &[&str] = &[
    r"
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
",
    // 2: "From now on" keeps an exact cutoff, so even a rules rebuild never reads older history.
    r"
ALTER TABLE sources ADD COLUMN start_after TEXT;
",
    // 3: Pins are remembered by the event's hashed key, so they survive a rules rebuild.
    r"
CREATE TABLE pins (key TEXT PRIMARY KEY, source TEXT NOT NULL) WITHOUT ROWID;
INSERT OR IGNORE INTO pins (key, source) SELECT seen_key, source FROM events WHERE pinned = 1;
",
];

/// Sets connection pragmas and applies any migrations the database hasn't had. Each migration
/// and its `user_version` bump commit together, so a failure leaves the previous version intact.
pub fn migrate(conn: &mut Connection) -> Result<()> {
    // Deleted text is overwritten, not just unlinked, so Forget and the retention step-down
    // leave nothing readable in the file's free pages.
    conn.pragma_update(None, "secure_delete", "ON")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    let version: usize = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i + 1)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_version(conn: &Connection) -> usize {
        conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap()
    }

    #[test]
    fn migrates_once_and_reopens_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let mut conn = Connection::open(&path).unwrap();
        migrate(&mut conn).unwrap();
        assert_eq!(user_version(&conn), MIGRATIONS.len());
        migrate(&mut conn).unwrap(); // a second run is a no-op, not a duplicate-column error
        drop(conn);
        let mut again = Connection::open(&path).unwrap();
        migrate(&mut again).unwrap();
        assert_eq!(user_version(&again), MIGRATIONS.len());
    }

    #[test]
    fn a_failed_migration_rolls_back() {
        let mut conn = Connection::open_in_memory().unwrap();
        // A clashing table makes migration 1 fail part-way, after it created `settings`.
        conn.execute_batch("CREATE TABLE events (x)").unwrap();
        assert!(migrate(&mut conn).is_err());
        assert_eq!(user_version(&conn), 0, "the version bump rolled back");
        let settings: bool = conn
            .query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = 'settings')", [], |r| r.get(0))
            .unwrap();
        assert!(!settings, "so did the tables it created");
    }
}
