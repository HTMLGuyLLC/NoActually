//! Counting history again under new rules. Scores are set aside first, and whatever can't be read
//! again comes back as it was counted, so a chat whose file is gone is never lost.

use crate::cards::clear_left_out;
use crate::{put_setting, Level, Result, Store};
use rusqlite::OptionalExtension;

/// Tables that hold scores (everything but consent and settings). Each has a `source` column.
/// `report_cards` is not one: a rebuild keeps it (Claude Code deletes old transcripts, so those rows
/// are the history), and reading a card again writes the same row.
pub(crate) const SCORE_TABLES: [&str; 8] =
    ["events", "daily", "sessions", "tasks", "files", "seen", "tips", "tip_chats"];
/// The score tables a rebuild sets aside in `kept_*`. `files` isn't one: its checkpoints have to go.
pub(crate) const KEPT_TABLES: [&str; 7] = ["events", "daily", "sessions", "tasks", "seen", "tips", "tip_chats"];
/// The setting that says a rebuild has set scores aside and hasn't put back what it couldn't read.
pub(crate) const REBUILDING_KEY: &str = "rules_rebuild.pending";

/// The setting that says which [`engine::RULES_VERSION`] the scores were counted under.
pub const RULES_KEY: &str = "rules_version";

impl Store {
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
}

/// A rebuild never keeps more text than the correction had: levels only step down.
pub(crate) fn level_after_rebuild(tx: &rusqlite::Transaction<'_>, key: &str, level: Level) -> Result<Level> {
    let kept: Option<String> =
        tx.query_row("SELECT level FROM kept_events WHERE seen_key = ?1", [key], |r| r.get(0)).optional()?;
    Ok(kept.and_then(|l| l.parse().ok()).map_or(level, |l: Level| l.at_most(level)))
}
