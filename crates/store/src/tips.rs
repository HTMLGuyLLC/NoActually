//! Tips in the database. When a follow-up could have been in the first message, the AI says so in
//! a line of its reply. Only what the line's bracket says is kept, never the words, and tips are
//! counted against the tasks in chats that had the rules. The design is in `docs/ARCHITECTURE.md`.

use crate::cards::{ts_str, CLEARED_KEY};
use crate::stats::{monday_of, View};
use crate::{hash, Filter, Result, SourceState, Store, NO_MODEL};
use chrono::{DateTime, Days, Local, NaiveDate, Utc};
use engine::{habit_bits, normalize_model};
use rusqlite::{params, Connection};
use serde::Serialize;
use sources::{FileScan, TipSeen};
use std::collections::BTreeMap;

/// The latest tips "your most common tip" looks at.
const RECENT_TIPS: i64 = 20;
/// Fewer tips than this and no habit is really the most common.
const MIN_TOP_TIP: i64 = 5;

/// The source whose chats get the rules.
const CLAUDE_CODE: &str = "claude-code";
/// Tasks a week needs, in chats that had the rules, before two weeks are compared.
const MIN_TIP_WEEK: i64 = 10;
/// Chats that started since tips were turned on, none with the rules, before Settings says something's
/// wrong. One or two could be scripts, which never get the rules.
const MIN_CHATS_WITHOUT_RULES: i64 = 3;
/// The same, after chats did get the rules: they stopping is a bigger claim, so it takes more.
const MIN_CHATS_AFTER_RULES: i64 = 5;

/// One week of tips.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TipWeek {
    /// Tips your AI gave.
    pub tips: i64,
    /// Tasks started in chats that had the rules: what `tips` is out of.
    pub tasks: i64,
}

/// The tip you get most.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TopTip {
    /// The habit, by name: target, evidence, finish, guardrails, purpose or context.
    pub habit: String,
    /// How many of your latest tips were about it.
    pub tips: i64,
    /// How many tips that's out of.
    pub of: i64,
}

/// The last full week of tips against the week before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TipTrend {
    /// The last full week.
    pub latest: TipWeek,
    /// The week before it.
    pub before: TipWeek,
}

/// Whether tips are reaching your chats.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct TipActivity {
    /// Chats with the rules that you used this week.
    pub active: i64,
    /// Tips kept so far.
    pub tips: i64,
    /// Chats have started without the rules, since tips were turned on or since the last chat that
    /// got them.
    pub blocked: bool,
}

fn local_day(ts: DateTime<Utc>) -> String {
    ts.with_timezone(&Local).date_naive().to_string()
}

/// Writes what `scan` found, in the caller's transaction. Reading a reply again writes nothing new,
/// and neither does a forked chat's copy of one, which keeps its id. Anything this source's consent
/// skips, or from before the history was last deleted (`cleared`), stays out.
pub(crate) fn save(conn: &Connection, state: &SourceState, scan: &FileScan, cleared: Option<&str>) -> Result<()> {
    let counts = |ts: DateTime<Utc>| state.wants(ts) && cleared.is_none_or(|t| ts_str(ts).as_str() > t);
    for seen in scan.tips.iter().filter(|t| counts(t.ts)) {
        save_tip(conn, &state.id, seen)?;
        // A chat can get here without its rules being seen: a scan that began after them, or a tool
        // that gives no record of them. Its tips still have to count against its tasks.
        save_chat(conn, &state.id, &seen.session, seen.ts)?;
    }
    // Every time, not just the first: a chat's early rules can be from before the history was deleted.
    for rules in scan.rules.iter().filter(|r| counts(r.ts)) {
        save_chat(conn, &state.id, &rules.session, rules.ts)?;
    }
    Ok(())
}

fn save_tip(conn: &Connection, source: &str, seen: &TipSeen) -> Result<()> {
    let key = match &seen.key {
        Some(key) => hash(&[source, key]),
        None => hash(&[source, &seen.session, &seen.pos.to_string()]),
    };
    let model = seen.model.as_deref().and_then(normalize_model);
    let (provider, family, version) = model
        .as_ref()
        .map_or((NO_MODEL, NO_MODEL, ""), |m| (m.provider.as_str(), m.family.as_str(), m.version.as_str()));
    conn.execute(
        "INSERT OR IGNORE INTO tips (key, source, session, day, ts, provider, family, version, habit, redid)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            key,
            source,
            hash(&[source, &seen.session]),
            local_day(seen.ts),
            ts_str(seen.ts),
            provider,
            family,
            version,
            seen.tip.habit,
            seen.tip.redid
        ],
    )?;
    Ok(())
}

fn save_chat(conn: &Connection, source: &str, session: &str, ts: DateTime<Utc>) -> Result<()> {
    conn.execute(
        "INSERT INTO tip_chats (session, source, day) VALUES (?1,?2,?3)
         ON CONFLICT(session) DO UPDATE SET day = min(day, excluded.day)",
        params![hash(&[source, session]), source, local_day(ts)],
    )?;
    Ok(())
}

/// Tips per week, and the tasks they're out of: those from the day a chat had the rules on.
pub(crate) fn weeks(v: &View) -> Result<BTreeMap<NaiveDate, TipWeek>> {
    let mut weeks: BTreeMap<NaiveDate, TipWeek> = BTreeMap::new();
    let by_day = |sql: &str| v.rows(sql, &[], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)));
    let tips = format!("SELECT day, COUNT(*) FROM tips WHERE 1=1 {} GROUP BY day", v.clause);
    for (day, n) in by_day(&tips)? {
        if let Ok(day) = day.parse::<NaiveDate>() {
            weeks.entry(monday_of(day)).or_default().tips += n;
        }
    }
    let tasks = format!(
        "SELECT day, COUNT(*) FROM tasks
         WHERE EXISTS (SELECT 1 FROM tip_chats c WHERE c.session = tasks.session AND c.day <= tasks.day) {}
         GROUP BY day",
        v.clause
    );
    for (day, n) in by_day(&tasks)? {
        if let Ok(day) = day.parse::<NaiveDate>() {
            weeks.entry(monday_of(day)).or_default().tasks += n;
        }
    }
    Ok(weeks)
}

/// Last full week against the week before, once both have enough tasks to say.
pub(crate) fn trend(v: &View, today: NaiveDate) -> Result<Option<TipTrend>> {
    let weeks = weeks(v)?;
    let week = |back: u64| {
        let monday = monday_of(today).checked_sub_days(Days::new(7 * back))?;
        let w = weeks.get(&monday)?;
        // A task can take two tips, but "7 of 5 tasks" can't be read.
        (w.tasks >= MIN_TIP_WEEK).then(|| TipWeek { tips: w.tips.min(w.tasks), tasks: w.tasks })
    };
    Ok(week(1).zip(week(2)).map(|(latest, before)| TipTrend { latest, before }))
}

/// The habit your latest tips were about most, once there are enough of them to say.
pub(crate) fn top_tip(v: &View) -> Result<Option<TopTip>> {
    let sql = format!("SELECT habit FROM tips WHERE 1=1 {} ORDER BY ts DESC, key DESC LIMIT {RECENT_TIPS}", v.clause);
    let recent: Vec<i64> = v.rows(&sql, &[], |r| r.get(0))?;
    let of = recent.len() as i64;
    if of < MIN_TOP_TIP {
        return Ok(None);
    }
    let count = |bit: u8| recent.iter().filter(|h| **h == i64::from(bit)).count() as i64;
    let top = habit_bits()
        .into_iter()
        .map(|(name, bit)| (name, count(bit)))
        // Reversed, because the last of equal counts wins and the habit on the card first should.
        .rev()
        .max_by_key(|(_, tips)| *tips);
    Ok(top.map(|(name, tips)| TopTip { habit: name.to_string(), tips, of }))
}

impl Store {
    /// Whether tips are reaching your chats. `on_since` is the day they were turned on, if they're on.
    pub fn tip_activity(&self, today: NaiveDate, on_since: Option<NaiveDate>) -> Result<TipActivity> {
        let conn = self.conn();
        let active = conn.query_row(
            "SELECT COUNT(*) FROM tip_chats c JOIN sessions s ON s.id = c.session WHERE s.last_day >= ?1",
            [monday_of(today).to_string()],
            |r| r.get(0),
        )?;
        let tips = conn.query_row("SELECT COUNT(*) FROM tips", [], |r| r.get(0))?;
        let Some(on_since) = on_since else { return Ok(TipActivity { active, tips, blocked: false }) };
        // Deleting the history also forgets which chats had the rules, so only what's after it can say.
        let cleared = self.get_setting(CLEARED_KEY)?.and_then(|ts| ts.parse::<DateTime<Utc>>().ok());
        let since = cleared.map_or(on_since, |ts| on_since.max(ts.with_timezone(&Local).date_naive()));
        let last_told: Option<String> = conn.query_row(
            "SELECT MAX(s.first_day) FROM sessions s JOIN tip_chats c ON c.session = s.id
             WHERE s.source = ?1 AND s.first_day >= ?2",
            params![CLAUDE_CODE, since.to_string()],
            |r| r.get(0),
        )?;
        // From the day after: a chat from that day may have started before it.
        let floor = last_told.clone().unwrap_or_else(|| since.to_string());
        let started: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE source = ?1 AND first_day > ?2",
            params![CLAUDE_CODE, floor],
            |r| r.get(0),
        )?;
        let needed = if last_told.is_some() { MIN_CHATS_AFTER_RULES } else { MIN_CHATS_WITHOUT_RULES };
        Ok(TipActivity { active, tips, blocked: started >= needed })
    }

    /// The tip lines `stats --report-cards` prints, under `f`'s models and over all your history.
    pub fn tip_summary(&self, f: &Filter, today: NaiveDate) -> Result<(Option<TipTrend>, Option<TopTip>)> {
        let (clause, args) = Filter { since: None, ..f.clone() }.sql();
        let v = View { conn: self.conn(), clause, args };
        Ok((trend(&v, today)?, top_tip(&v)?))
    }
}
