use crate::{Error, Result};
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
    // 8: Report cards. One row per (card, graded task): numbers only, never chat text. Not a score
    // table: a rules rebuild re-reads files and writes the same rows again, and Claude Code deletes
    // old transcripts, so these rows are the history. `first` is the task's first prompt key: a
    // resumed chat is a new file but keeps its prompts, so a re-grade there finds the same task. The
    // view is the rows that still count: not replaced by a newer card (graded_at, then rounds, then
    // key) that grades the same first prompt, or, in the same chat, a task sharing more than an
    // endpoint with it. `engine::tasks_overlap` is the same rule in Rust. A file also remembers the
    // last card position it read (`cards_to`, NULL until one), so the chat isn't read whole again to
    // see the same card. `tokens` is JSON: the tokens the task redid by the raw id of the model that
    // answered each round ({"claude-opus-5-5":{"input":..,"cache_write":..,"cache_read":..,"output":..}}),
    // priced when read.
    r"
ALTER TABLE files ADD COLUMN cards_to INTEGER;
CREATE TABLE report_cards (
    key TEXT PRIMARY KEY,
    card TEXT NOT NULL,
    source TEXT NOT NULL, session TEXT NOT NULL,
    first TEXT NOT NULL,
    day TEXT NOT NULL,
    start_ts TEXT NOT NULL, end_ts TEXT NOT NULL,
    provider TEXT NOT NULL, family TEXT NOT NULL, version TEXT NOT NULL,
    grade TEXT NOT NULL, rounds INTEGER NOT NULL, avoidable INTEGER NOT NULL,
    missing INTEGER NOT NULL,
    auto INTEGER NOT NULL,
    tokens TEXT NOT NULL,
    minutes INTEGER NOT NULL,
    graded_at TEXT NOT NULL
) WITHOUT ROWID;
CREATE INDEX report_cards_session ON report_cards (session);
CREATE INDEX report_cards_first ON report_cards (source, first);
CREATE INDEX report_cards_day ON report_cards (day);
CREATE VIEW current_report_cards AS
SELECT * FROM report_cards a WHERE NOT EXISTS (
    SELECT 1 FROM report_cards b
    WHERE b.card <> a.card
      AND (b.graded_at, b.rounds, b.key) > (a.graded_at, a.rounds, a.key)
      AND ((b.source = a.source AND b.first = a.first)
        OR (b.session = a.session AND b.start_ts < a.end_ts AND a.start_ts < b.end_ts))
);
",
    // 9: Tips an AI gave, and the chats that had the rules. Numbers only: the words stay in the chat.
    // Score tables, so a rebuild reads them again and they stay in step with `tasks`.
    r"
CREATE TABLE tips (
    key TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    session TEXT NOT NULL,
    day TEXT NOT NULL,
    ts TEXT NOT NULL,
    provider TEXT NOT NULL,
    family TEXT NOT NULL,
    version TEXT NOT NULL,
    habit INTEGER NOT NULL,
    redid INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX tips_day ON tips (day);
CREATE TABLE tip_chats (
    session TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    day TEXT NOT NULL
) WITHOUT ROWID;
",
    // 10: Where a rules rebuild sets scores aside while it reads history again. Days it couldn't read
    // come back from here, so history whose files are gone isn't lost. Empty between rebuilds.
    r"
CREATE TABLE kept_events AS SELECT * FROM events WHERE 0;
CREATE TABLE kept_daily AS SELECT * FROM daily WHERE 0;
CREATE TABLE kept_sessions AS SELECT * FROM sessions WHERE 0;
CREATE TABLE kept_tasks AS SELECT * FROM tasks WHERE 0;
CREATE TABLE kept_seen AS SELECT * FROM seen WHERE 0;
CREATE TABLE kept_tips AS SELECT * FROM tips WHERE 0;
CREATE TABLE kept_tip_chats AS SELECT * FROM tip_chats WHERE 0;
",
    // 11: `report_cards.auto` was never used. The view selects `*`, so it goes first and comes back
    // after, with the same rule as migration 8.
    r"
DROP VIEW current_report_cards;
ALTER TABLE report_cards DROP COLUMN auto;
CREATE VIEW current_report_cards AS
SELECT * FROM report_cards a WHERE NOT EXISTS (
    SELECT 1 FROM report_cards b
    WHERE b.card <> a.card
      AND (b.graded_at, b.rounds, b.key) > (a.graded_at, a.rounds, a.key)
      AND ((b.source = a.source AND b.first = a.first)
        OR (b.session = a.session AND b.start_ts < a.end_ts AND a.start_ts < b.end_ts))
);
",
    // 12: A rebuild records every chat it read, because a fork can claim all of a chat's messages and
    // leave it with no `sessions` row. `seen` learns its chat so a rebuild can restore only the keys
    // of chats it didn't read. Rows from before this are NULL.
    r"
CREATE TABLE rebuild_read (id TEXT PRIMARY KEY, source TEXT NOT NULL) WITHOUT ROWID;
ALTER TABLE seen ADD COLUMN session TEXT;
ALTER TABLE kept_seen ADD COLUMN session TEXT;
CREATE INDEX kept_events_seen_key ON kept_events (seen_key);
",
    // 13: `events.model_raw` and `events.rules` were written but never read. `kept_events` holds the
    // same columns in the same order, because a rebuild copies rows across with `SELECT *`, so both
    // lose them together, and a rebuild that was cut short still finishes.
    r"
ALTER TABLE events DROP COLUMN model_raw;
ALTER TABLE events DROP COLUMN rules;
ALTER TABLE kept_events DROP COLUMN model_raw;
ALTER TABLE kept_events DROP COLUMN rules;
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
    if version > MIGRATIONS.len() {
        return Err(Error::NewerDatabase { found: version, known: MIGRATIONS.len() });
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version) {
        // Immediate, and checked again inside: the app and a command can open an old file together.
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let now: usize = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if now > i {
            continue;
        }
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
    fn a_database_from_a_newer_version_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let mut conn = Connection::open(&path).unwrap();
        migrate(&mut conn).unwrap();
        conn.pragma_update(None, "user_version", MIGRATIONS.len() + 1).unwrap();
        let err = migrate(&mut conn).unwrap_err();
        assert!(matches!(err, Error::NewerDatabase { .. }), "{err}");
        assert!(err.to_string().contains("Update the app"), "{err}");
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
    fn version_7_gains_the_report_card_table_and_the_view_of_what_counts() {
        let mut conn = Connection::open_in_memory().unwrap();
        for sql in &MIGRATIONS[..7] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 7).unwrap();
        conn.execute("INSERT INTO settings VALUES ('keep', 'me')", []).unwrap();
        migrate(&mut conn).unwrap();
        assert_eq!(user_version(&conn), MIGRATIONS.len());
        let count = |table: &str| -> i64 {
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap()
        };
        assert_eq!((count("report_cards"), count("current_report_cards"), count("settings")), (0, 0, 1));
    }

    #[test]
    fn version_10_loses_the_unused_auto_column_and_keeps_its_cards() {
        let mut conn = Connection::open_in_memory().unwrap();
        for sql in &MIGRATIONS[..10] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 10).unwrap();
        conn.execute(
            "INSERT INTO report_cards VALUES ('k','c','claude-code','s','f','2026-10-05','2026-10-05T10:00:00.000Z',
               '2026-10-05T10:30:00.000Z','Anthropic','Claude Opus','5.5','B',3,1,0,0,'{}',2,'2026-10-05T11:00:00.000Z')",
            [],
        )
        .unwrap();
        migrate(&mut conn).unwrap();
        assert_eq!(user_version(&conn), MIGRATIONS.len());
        let columns: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('report_cards')")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(!columns.iter().any(|c| c == "auto"), "{columns:?}");
        let (grade, minutes): (String, i64) = conn
            .query_row("SELECT grade, minutes FROM current_report_cards WHERE key = 'k'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((grade.as_str(), minutes), ("B", 2), "the view works again, on the same rows");
    }

    /// One row as version 12 stored it, for `events` or `kept_events`.
    fn old_event(conn: &Connection, table: &str, id: i64, session: &str, level: &str, pinned: i64, seen_key: &str) {
        conn.execute(
            &format!(
                "INSERT INTO {table} VALUES (?1, '2026-10-05T10:00:00+00:00', '2026-10-05', 'claude-code', ?2,
                   'Anthropic', 'Claude Opus', '5.5', 'claude-opus-5-5', 'confirmed', 'made_it_up', 'ai.youre_right',
                   ?3, 'all of it', 'the reply', 'part of it', 'part of the reply', ?4, ?5)"
            ),
            rusqlite::params![id, session, level, pinned, seen_key],
        )
        .unwrap();
    }

    /// What `events` holds for one key: id, label, level, full text and whether it's pinned.
    fn row(store: &crate::Store, key: &str) -> (i64, String, String, Option<String>, i64) {
        let sql = "SELECT id, label, level, human_full, pinned FROM events WHERE seen_key = ?1";
        let read = |r: &rusqlite::Row| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?));
        store.conn().query_row(sql, [key], read).unwrap()
    }

    #[test]
    fn version_12_loses_the_unread_event_columns_and_keeps_every_row() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(crate::DB_FILE);
        let conn = Connection::open(&path).unwrap();
        for sql in &MIGRATIONS[..12] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 12).unwrap();
        old_event(&conn, "events", 7, "kept-chat", "full", 1, "pinned");
        old_event(&conn, "events", 8, "kept-chat", "snippet", 0, "plain");
        // A rebuild cut short: one row set aside, from a chat that won't be read again.
        old_event(&conn, "kept_events", 9, "gone-chat", "full", 0, "set-aside");
        conn.execute("INSERT INTO settings VALUES ('rules_rebuild.pending', '1')", []).unwrap();
        drop(conn);

        let mut store = crate::Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), MIGRATIONS.len());
        for table in ["events", "kept_events"] {
            let columns: Vec<String> = store
                .conn()
                .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            assert_eq!(
                columns.join(" "),
                "id ts day source session provider family version label category level human_full ai_full \
                 human_snip ai_snip pinned seen_key",
                "{table}"
            );
        }
        assert_eq!(row(&store, "pinned"), (7, "confirmed".into(), "full".into(), Some("all of it".into()), 1));
        assert_eq!(row(&store, "plain").0, 8, "ids stay, so a pin still finds its row");

        // The rebuild that was cut short still finishes, on the rows it set aside.
        store.finish_rebuild().unwrap();
        assert_eq!(row(&store, "set-aside").3.as_deref(), Some("all of it"));
        let count = |table: &str| -> i64 {
            store.conn().query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap()
        };
        assert_eq!((count("events"), count("kept_events")), (3, 0));
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
