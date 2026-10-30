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

-- Corrections and humbled moments. Text columns obey the retention level.
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
    // 3: Pinned corrections survive a rules rebuild; re-reading one looks it up by key.
    r"
CREATE INDEX events_seen_key ON events (seen_key);
",
    // 4: Prompts. One row per task (a chat's opening request, or a "next, ..."): which signals its
    // prompt carried and how often it was steered, per cause. Numbers only, never text.
    r"
CREATE TABLE tasks (
    id INTEGER PRIMARY KEY,
    day TEXT NOT NULL, source TEXT NOT NULL, session TEXT NOT NULL,
    provider TEXT NOT NULL, family TEXT NOT NULL, version TEXT NOT NULL,
    signals INTEGER NOT NULL,
    messages INTEGER NOT NULL DEFAULT 1,
    ai_wrong INTEGER NOT NULL DEFAULT 0,
    added INTEGER NOT NULL DEFAULT 0,
    clarified INTEGER NOT NULL DEFAULT 0,
    redo INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX tasks_session ON tasks (session);
CREATE INDEX tasks_day ON tasks (day);
",
    // 5: Goals. A goal's closed weeks, frozen as they were, and finished goals. Numbers only.
    // Not score tables: a rules rebuild or Forget doesn't take back a week you already earned.
    r"
CREATE TABLE goal_weeks (
    goal INTEGER NOT NULL, week TEXT NOT NULL,
    count INTEGER NOT NULL, value INTEGER NOT NULL, state TEXT NOT NULL,
    PRIMARY KEY (goal, week)
) WITHOUT ROWID;
CREATE TABLE trophies (id INTEGER PRIMARY KEY, goal TEXT NOT NULL, done TEXT NOT NULL);
",
    // 6: The words changed: catches are corrections, briefs are prompts. Rename the column and
    // move the saved tier baseline to its new settings key.
    r"
ALTER TABLE sessions RENAME COLUMN catches TO corrections;
UPDATE settings SET key = 'milestone.prompt_tier' WHERE key = 'milestone.brief_tier';
",
    // 7: Remember which checkpoints came from a final pass, so a finished chat isn't re-read.
    r"
ALTER TABLE files ADD COLUMN finalized INTEGER NOT NULL DEFAULT 0;
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
    fn version_5_migrates_to_corrections_and_prompt_tier_key() {
        let mut conn = Connection::open_in_memory().unwrap();
        for sql in &MIGRATIONS[..5] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 5).unwrap();
        conn.execute("INSERT INTO sessions VALUES ('s', 'x', '2026-01-01', '2026-01-01', 4, 3)", []).unwrap();
        conn.execute("INSERT INTO settings VALUES ('milestone.brief_tier', 'hawk_eyed')", []).unwrap();
        migrate(&mut conn).unwrap();
        assert_eq!(user_version(&conn), MIGRATIONS.len());
        let n: i64 = conn.query_row("SELECT corrections FROM sessions WHERE id = 's'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 3);
        let v: String =
            conn.query_row("SELECT value FROM settings WHERE key = 'milestone.prompt_tier'", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "hawk_eyed");
        let old: i64 = conn
            .query_row("SELECT COUNT(*) FROM settings WHERE key = 'milestone.brief_tier'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(old, 0);
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
