//! Read side: everything the scorecard, quick glance and model breakdown show.

use crate::{Result, Store};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Timelike};
use engine::{
    Answer, BriefTier, Category, Grade, Label, Score, Signal, Signals, Steer, Tier, Window, MIN_BRIEFS, WINDOW_DAYS,
};
use rusqlite::{params_from_iter, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

/// A chat with at least this many messages counts as a real one.
const REAL_CHAT: i64 = 10;
/// Entries in the hall of fame.
const HALL_SIZE: i64 = 12;
/// A tip only quotes "with vs without" when both groups have this many continued tasks.
const TIP_MIN_GROUP: i64 = 10;
/// ...and briefs carrying the signal landed at least this many points more often.
const TIP_MIN_LIFT: f64 = 10.0;
/// No tip once even the least-carried signal is in at least this share of briefs.
const TIP_MAX_PCT: f64 = 50.0;

/// The labels that count as catches, as an SQL list: `('confirmed','admitted','claimed')`.
pub(crate) static CATCH_KINDS: LazyLock<String> = LazyLock::new(|| {
    let ids: Vec<String> = Label::ALL.iter().filter(|l| l.is_catch()).map(|l| format!("'{}'", l.as_str())).collect();
    format!("({})", ids.join(","))
});

/// Narrow any view to a source and/or a model at any level (provider → family → version).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filter {
    /// A source id.
    pub source: Option<String>,
    /// A provider ("Anthropic").
    pub provider: Option<String>,
    /// A family ("Claude Opus").
    pub family: Option<String>,
    /// A version ("5.5"). Usually set together with `family`.
    pub version: Option<String>,
    /// Only days from this one on: counts and the score both follow it. Without it, counts cover
    /// all of history and the score the last [`WINDOW_DAYS`] days. "This week" never moves.
    #[serde(default)]
    pub since: Option<NaiveDate>,
}

impl Filter {
    /// `AND col = ?` for each set field, and the values to bind, in order.
    pub(crate) fn sql(&self) -> (String, Vec<String>) {
        let mut sql = String::new();
        let mut args = Vec::new();
        for (col, v) in [
            ("source", &self.source),
            ("provider", &self.provider),
            ("family", &self.family),
            ("version", &self.version),
        ] {
            if let Some(v) = v {
                sql.push_str(" AND ");
                sql.push_str(col);
                sql.push_str(" = ?");
                args.push(v.clone());
            }
        }
        // Every table this filters (daily, events, tasks) has a local `day`.
        if let Some(since) = self.since {
            sql.push_str(" AND day >= ?");
            args.push(since.to_string());
        }
        (sql, args)
    }
}

/// One model's numbers.
#[derive(Debug, Default, Serialize)]
pub struct ModelRow {
    /// Provider.
    pub provider: String,
    /// Family.
    pub family: String,
    /// Version, or "".
    pub version: String,
    /// Messages sent to it.
    pub messages: i64,
    /// Times you caught it.
    pub catches: i64,
    /// Times it caught you.
    pub humbled: i64,
    /// Tasks given to it.
    pub tasks: i64,
    /// Of those, the ones that went past their first message.
    pub continued: i64,
    /// Continued tasks you never had to steer.
    pub landed: i64,
}

/// "Could AI replace you?": the overall score from the last [`WINDOW_DAYS`] days, and the two
/// parts it mixes. The score fields are `None` until there are enough tasks to rank.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Overall {
    /// 0 to 100.
    pub score: Option<i64>,
    /// Percent of continued tasks that landed.
    pub direction: Option<i64>,
    /// 0 to 100: how rarely it corrected you, once you've shown you check its work.
    pub judgment: Option<i64>,
    /// The tier the score reaches.
    pub tier: Option<Tier>,
    /// The tier above it; its floor is the target to show.
    pub next_tier: Option<Tier>,
    /// The headline answer.
    pub verdict: Answer,
    /// Times you caught it in the window.
    pub catches: i64,
    /// Times it caught you in the window.
    pub humbled: i64,
    /// More continued tasks before a score is given (0 once ranked).
    pub to_rank: i64,
}

/// One week's numbers.
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct WeekRow {
    /// The week's Monday.
    pub week: NaiveDate,
    /// Messages that week.
    pub messages: i64,
    /// Catches that week.
    pub catches: i64,
}

/// One catch (or humbled moment) shown in the hall of fame.
#[derive(Debug, Default, Serialize)]
pub struct HallEntry {
    /// Event id, for pinning.
    pub id: i64,
    /// Local day it happened.
    pub day: String,
    /// Source id.
    pub source: String,
    /// The source's display name.
    pub source_name: String,
    /// "Claude Opus 5.5".
    pub model: String,
    /// Label id.
    pub label: String,
    /// Category id, or "" for humbled moments.
    pub category: String,
    /// The category's display title, or "".
    pub category_title: String,
    /// Your side, per the retention level.
    pub human: Option<String>,
    /// The AI's side, per the retention level.
    pub ai: Option<String>,
    /// Pinned entries keep their text.
    pub pinned: bool,
}

/// The whole scorecard for one filter.
#[derive(Debug, Serialize)]
pub struct Stats {
    /// First day with any message.
    pub first_day: Option<String>,
    /// Last day with any message.
    pub last_day: Option<String>,
    /// Messages ever.
    pub messages: i64,
    /// Catches ever.
    pub catches: i64,
    /// Times the AI caught you.
    pub humbled: i64,
    /// Count per label id.
    pub labels: BTreeMap<String, i64>,
    /// Catches in the 7 days ending today.
    pub catches_last_7d: i64,
    /// "Could AI replace you?"
    pub overall: Overall,
    /// One catch every this many messages.
    pub messages_per_catch: Option<i64>,
    /// Chats with at least 10 messages.
    pub real_chats: i64,
    /// Share of real chats with at least one catch, one decimal.
    pub pct_real_chats_with_catch: Option<f64>,
    /// Longest run of days with a catch.
    pub best_streak_days: i64,
    /// The run ending today or yesterday.
    pub current_streak_days: i64,
    /// Local hour (0–23) with the most catches.
    pub peak_hour: Option<u32>,
    /// Catches per category id.
    pub categories: BTreeMap<String, i64>,
    /// Per week, oldest first.
    pub weekly: Vec<WeekRow>,
    /// Per model, most caught first.
    pub models: Vec<ModelRow>,
    /// Pinned first, then confirmed catches, newest first.
    pub hall_of_fame: Vec<HallEntry>,
    /// How you brief it.
    pub briefs: Briefs,
}

/// How you brief the AI: your tasks, what their briefs carried, and what it took to land them.
/// Only tasks that continued past their first message can need steering, so only they count
/// toward `landed` and the tier.
#[derive(Debug, Default, PartialEq, Serialize)]
pub struct Briefs {
    /// Tasks ever (questions and chatter aren't tasks).
    pub tasks: i64,
    /// Of those, the ones that went past their first message.
    pub continued: i64,
    /// Continued tasks you never had to add to, clarify or redo.
    pub landed: i64,
    /// Continued tasks in the score's window (the filter's `since`, or the last [`WINDOW_DAYS`] days).
    pub window_tasks: i64,
    /// Of those, how many landed.
    pub window_landed: i64,
    /// The tier those reach; `None` until there are enough of them.
    pub tier: Option<BriefTier>,
    /// The tier above it (its floor is the target to show).
    pub next_tier: Option<BriefTier>,
    /// More continued tasks before a tier is given (0 once ranked).
    pub to_rank: i64,
    /// Briefs per grade id.
    pub grades: BTreeMap<String, i64>,
    /// Briefs carrying each signal, by signal id.
    pub signals: BTreeMap<String, i64>,
    /// Steering messages per cause id.
    pub steering: BTreeMap<String, i64>,
    /// What to try next, once there are briefs.
    pub tip: Option<Tip>,
}

/// The signal your briefs carry least, and (when your own history backs it up) how much more
/// often continued tasks whose briefs carry it landed.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Tip {
    /// Signal id.
    pub signal: String,
    /// Percent of briefs carrying it, one decimal.
    pub pct_of_briefs: f64,
    /// Percent of briefs carrying it that landed, and of those that didn't; only when the gap
    /// is real (see `TIP_MIN_GROUP`, `TIP_MIN_LIFT`).
    pub landed_with_without: Option<(f64, f64)>,
}

/// The daily / events rows one filter selects.
pub(crate) struct View<'a> {
    pub(crate) conn: &'a Connection,
    /// `AND …` clause; works on `daily`, `events` and `tasks` (same column names).
    pub(crate) clause: String,
    pub(crate) args: Vec<String>,
}

impl View<'_> {
    /// Runs `sql` (with `{w}` already holding the filter clause) binding the filter's values
    /// and then `extra`, in that order.
    pub(crate) fn rows<T>(
        &self,
        sql: &str,
        extra: &[&str],
        f: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
    ) -> Result<Vec<T>> {
        let args: Vec<&str> = self.args.iter().map(String::as_str).chain(extra.iter().copied()).collect();
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params_from_iter(args), f)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Sum of `daily.n` over the filter plus `extra` SQL (binding `extra_args`).
    fn sum(&self, extra: &str, extra_args: &[&str]) -> Result<i64> {
        let sql = format!("SELECT COALESCE(SUM(n), 0) FROM daily WHERE 1=1 {} {extra}", self.clause);
        Ok(self.rows(&sql, extra_args, |r| r.get(0))?.pop().unwrap_or(0))
    }
}

/// Headline counts.
struct Totals {
    messages: i64,
    catches: i64,
    humbled: i64,
    catches_last_7d: i64,
    window_catches: i64,
    window_humbled: i64,
}

/// The first day the score looks at: `since` when the view has one, else [`WINDOW_DAYS`] back.
fn score_start(since: Option<NaiveDate>, today: NaiveDate) -> NaiveDate {
    since.unwrap_or(today - Duration::days(WINDOW_DAYS - 1))
}

/// Counts over `v`; "this week" and the score's window (from `start`) over `scope`, the same
/// filter without `since`.
fn totals(v: &View, scope: &View, today: NaiveDate, start: NaiveDate) -> Result<Totals> {
    let kinds = CATCH_KINDS.as_str();
    let week_ago = (today - Duration::days(6)).to_string();
    let window = start.to_string();
    Ok(Totals {
        messages: v.sum("AND kind = 'message'", &[])?,
        catches: v.sum(&format!("AND kind IN {kinds}"), &[])?,
        humbled: v.sum("AND kind = 'humbled'", &[])?,
        catches_last_7d: scope.sum(&format!("AND kind IN {kinds} AND day >= ?"), &[&week_ago])?,
        window_catches: scope.sum(&format!("AND kind IN {kinds} AND day >= ?"), &[&window])?,
        window_humbled: scope.sum("AND kind = 'humbled' AND day >= ?", &[&window])?,
    })
}

fn day_range(v: &View) -> Result<(Option<String>, Option<String>)> {
    let sql = format!("SELECT MIN(day), MAX(day) FROM daily WHERE 1=1 {}", v.clause);
    Ok(v.rows(&sql, &[], |r| Ok((r.get(0)?, r.get(1)?)))?.pop().unwrap_or_default())
}

/// Counts per label, and per category for catches.
fn labels_and_categories(v: &View) -> Result<(BTreeMap<String, i64>, BTreeMap<String, i64>)> {
    let sql = format!(
        "SELECT kind, category, SUM(n) FROM daily WHERE kind != 'message' {} GROUP BY kind, category",
        v.clause
    );
    let mut labels = BTreeMap::new();
    let mut categories = BTreeMap::new();
    for (kind, cat, n) in
        v.rows(&sql, &[], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))?
    {
        if kind != Label::Humbled.as_str() && !cat.is_empty() {
            *categories.entry(cat).or_insert(0) += n;
        }
        *labels.entry(kind).or_insert(0) += n;
    }
    Ok((labels, categories))
}

/// (real chats, real chats with a catch). Chats only know their source, not a model, so this
/// respects the source filter alone, plus `since` (chats still going on or after it).
fn real_chats(conn: &Connection, source: Option<&str>, since: Option<NaiveDate>) -> Result<(i64, i64)> {
    Ok(conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(catches > 0), 0) FROM sessions
         WHERE messages >= ?1 AND (?2 IS NULL OR source = ?2) AND (?3 IS NULL OR last_day >= ?3)",
        rusqlite::params![REAL_CHAT, source, since.map(|d| d.to_string())],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

/// Sorted days that had at least one catch.
fn catch_days(v: &View) -> Result<Vec<NaiveDate>> {
    let sql = format!("SELECT DISTINCT day FROM daily WHERE kind IN {} {} ORDER BY day", *CATCH_KINDS, v.clause);
    let mut days: Vec<NaiveDate> =
        v.rows(&sql, &[], |r| r.get::<_, String>(0))?.into_iter().filter_map(|d| d.parse().ok()).collect();
    days.dedup();
    Ok(days)
}

/// (best, current) runs of consecutive days in sorted `days`. The current run still counts if
/// its last day was yesterday: today isn't over.
fn streaks(days: &[NaiveDate], today: NaiveDate) -> (i64, i64) {
    let (mut best, mut run) = (0i64, 0i64);
    for (i, d) in days.iter().enumerate() {
        run = if i > 0 && *d - days[i - 1] == Duration::days(1) { run + 1 } else { 1 };
        best = best.max(run);
    }
    let current = match days.last() {
        Some(d) if today - *d <= Duration::days(1) => run,
        _ => 0,
    };
    (best, current)
}

/// Catches per local hour of day.
fn catch_hours(v: &View) -> Result<[i64; 24]> {
    let sql = format!("SELECT ts FROM events WHERE label IN {} {}", *CATCH_KINDS, v.clause);
    let mut hours = [0i64; 24];
    for ts in v.rows(&sql, &[], |r| r.get::<_, String>(0))? {
        if let Ok(t) = DateTime::parse_from_rfc3339(&ts) {
            hours[t.with_timezone(&Local).hour() as usize] += 1;
        }
    }
    Ok(hours)
}

/// The busiest hour; on a tie, the later one. `None` with no catches.
fn peak_hour(hours: &[i64; 24]) -> Option<u32> {
    hours.iter().any(|n| *n > 0).then(|| (0..24).max_by_key(|h| hours[*h as usize]).unwrap_or(0))
}

/// The Monday starting `d`'s week.
pub(crate) fn monday_of(d: NaiveDate) -> NaiveDate {
    d - Duration::days(i64::from(d.weekday().num_days_from_monday()))
}

fn weekly(v: &View) -> Result<Vec<WeekRow>> {
    let sql = format!(
        "SELECT day, SUM(CASE WHEN kind = 'message' THEN n ELSE 0 END), SUM(CASE WHEN kind IN {} THEN n ELSE 0 END)
         FROM daily WHERE 1=1 {} GROUP BY day",
        *CATCH_KINDS, v.clause
    );
    let mut weeks: BTreeMap<NaiveDate, (i64, i64)> = BTreeMap::new();
    for (day, m, k) in v.rows(&sql, &[], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))? {
        let Ok(d) = day.parse::<NaiveDate>() else { continue };
        let e = weeks.entry(monday_of(d)).or_default();
        e.0 += m;
        e.1 += k;
    }
    Ok(weeks.into_iter().map(|(week, (messages, catches))| WeekRow { week, messages, catches }).collect())
}

fn models(v: &View) -> Result<Vec<ModelRow>> {
    let mut rows = model_counts(v)?;
    let sql = format!(
        "SELECT provider, family, version, COUNT(*), COALESCE(SUM(messages >= 2), 0), COALESCE(SUM({}), 0)
         FROM tasks WHERE 1=1 {} GROUP BY provider, family, version",
        LANDED.as_str(),
        v.clause
    );
    for (key, tasks, continued, landed) in v.rows(&sql, &[], |r| {
        Ok(((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?), r.get(3)?, r.get(4)?, r.get(5)?))
    })? {
        let i = rows.iter().position(|m| (&m.provider, &m.family, &m.version) == (&key.0, &key.1, &key.2));
        let i = i.unwrap_or_else(|| {
            rows.push(ModelRow { provider: key.0, family: key.1, version: key.2, ..ModelRow::default() });
            rows.len() - 1
        });
        (rows[i].tasks, rows[i].continued, rows[i].landed) = (tasks, continued, landed);
    }
    Ok(rows)
}

fn model_counts(v: &View) -> Result<Vec<ModelRow>> {
    let sql = format!(
        "SELECT provider, family, version,
            SUM(CASE WHEN kind = 'message' THEN n ELSE 0 END),
            SUM(CASE WHEN kind IN {} THEN n ELSE 0 END),
            SUM(CASE WHEN kind = 'humbled' THEN n ELSE 0 END)
         FROM daily WHERE 1=1 {} GROUP BY provider, family, version ORDER BY 5 DESC, 4 DESC",
        *CATCH_KINDS, v.clause
    );
    v.rows(&sql, &[], |r| {
        Ok(ModelRow {
            provider: r.get(0)?,
            family: r.get(1)?,
            version: r.get(2)?,
            messages: r.get(3)?,
            catches: r.get(4)?,
            humbled: r.get(5)?,
            ..ModelRow::default()
        })
    })
}

fn hall_of_fame(v: &View) -> Result<Vec<HallEntry>> {
    let names: HashMap<&str, &str> = SOURCE_NAMES.iter().copied().collect();
    let sql = format!(
        "SELECT id, day, source, family || CASE WHEN version = '' THEN '' ELSE ' ' || version END, label, category,
            COALESCE(human_full, human_snip), COALESCE(ai_full, ai_snip), pinned
         FROM events WHERE level != 'category' {}
         ORDER BY pinned DESC, label = 'confirmed' DESC, ts DESC LIMIT {HALL_SIZE}",
        v.clause
    );
    v.rows(&sql, &[], |r| {
        let source: String = r.get(2)?;
        let category: String = r.get(5)?;
        Ok(HallEntry {
            id: r.get(0)?,
            day: r.get(1)?,
            source_name: names.get(source.as_str()).map_or_else(|| source.clone(), |n| (*n).to_string()),
            source,
            model: r.get(3)?,
            label: r.get(4)?,
            category_title: Category::from_id(&category).map_or("", Category::title).to_string(),
            category,
            human: r.get(6)?,
            ai: r.get(7)?,
            pinned: r.get(8)?,
        })
    })
}

/// One decimal percent of `n` in `total`.
pub(crate) fn pct(n: i64, total: i64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    (1000.0 * n as f64 / total as f64).round() / 10.0
}

/// `added + clarified + redo`: steering that was on you, not the AI.
static ON_YOU: LazyLock<String> = LazyLock::new(|| {
    let cols: Vec<&str> = Steer::ALL.iter().filter(|c| c.is_on_you()).map(|c| c.as_str()).collect();
    format!("({})", cols.join(" + "))
});

/// A continued task that nothing on you steered, as SQL over `tasks`.
pub(crate) static LANDED: LazyLock<String> = LazyLock::new(|| format!("(messages >= 2 AND {} = 0)", ON_YOU.as_str()));

/// One group of tasks: the signals their brief carried, whether they continued, whether they landed.
pub(crate) type Group = (Signals, bool, bool, i64);

/// Counts over `v`; the tier's window (from `start`) over `scope`, the same filter without `since`.
fn briefs(v: &View, scope: &View, start: NaiveDate) -> Result<Briefs> {
    let landed = LANDED.as_str();
    // At most 32 masks x 3 outcomes, so the per-signal math happens here, not in SQL.
    let sql =
        format!("SELECT signals, messages >= 2, {landed}, COUNT(*) FROM tasks WHERE 1=1 {} GROUP BY 1, 2, 3", v.clause);
    let groups: Vec<Group> =
        v.rows(&sql, &[], |r| Ok((Signals(r.get::<_, u8>(0)?), r.get(1)?, r.get(2)?, r.get(3)?)))?;
    let mut b = Briefs::default();
    for (signals, continued, landed, n) in &groups {
        b.tasks += n;
        if *continued {
            b.continued += n;
        }
        if *landed {
            b.landed += n;
        }
        *b.grades.entry(Grade::of(*signals).as_str().to_string()).or_insert(0) += n;
        for s in Signal::ALL.into_iter().filter(|s| signals.has(*s)) {
            *b.signals.entry(s.as_str().to_string()).or_insert(0) += n;
        }
    }
    let sums: Vec<String> = Steer::ALL.iter().map(|c| format!("COALESCE(SUM({}), 0)", c.as_str())).collect();
    let sql = format!("SELECT {} FROM tasks WHERE 1=1 {}", sums.join(", "), v.clause);
    let row =
        v.rows(&sql, &[], |r| (0..Steer::ALL.len()).map(|i| r.get::<_, i64>(i)).collect::<rusqlite::Result<Vec<_>>>())?;
    for (c, n) in Steer::ALL.iter().zip(row.into_iter().flatten()) {
        b.steering.insert(c.as_str().to_string(), n);
    }
    (b.window_tasks, b.window_landed) = window_counts(scope, start)?;
    b.tier = BriefTier::for_tasks(b.window_landed, b.window_tasks);
    b.next_tier = b.tier.and_then(BriefTier::next);
    b.to_rank = (MIN_BRIEFS - b.window_tasks).max(0);
    b.tip = tip(&groups, b.tasks);
    Ok(b)
}

/// Continued tasks from `start` on, and how many landed.
fn window_counts(v: &View, start: NaiveDate) -> Result<(i64, i64)> {
    let since = start.to_string();
    let sql = format!(
        "SELECT COUNT(*), COALESCE(SUM({}), 0) FROM tasks WHERE messages >= 2 {} AND day >= ?",
        LANDED.as_str(),
        v.clause
    );
    Ok(v.rows(&sql, &[&since], |r| Ok((r.get(0)?, r.get(1)?)))?.pop().unwrap_or_default())
}

/// The signal carried least (ties: display order), unless your briefs mostly carry it already.
/// Quotes its lift only when it's real.
fn tip(groups: &[Group], tasks: i64) -> Option<Tip> {
    if tasks == 0 {
        return None;
    }
    let carried = |s: Signal| -> i64 { groups.iter().filter(|g| g.0.has(s)).map(|g| g.3).sum() };
    let s = Signal::ALL.into_iter().min_by_key(|s| carried(*s))?;
    Some(tip_for(groups, s)).filter(|t| t.pct_of_briefs < TIP_MAX_PCT)
}

/// The tip for `s`: the share of all briefs carrying it, and how often continued tasks with and
/// without it landed, when the gap is real.
pub(crate) fn tip_for(groups: &[Group], s: Signal) -> Tip {
    let n = |has: bool, f: fn(&Group) -> bool| -> i64 {
        groups.iter().filter(|g| g.0.has(s) == has && f(g)).map(|g| g.3).sum()
    };
    let (with, without) = (n(true, |g| g.1), n(false, |g| g.1));
    let (rate_with, rate_without) = (pct(n(true, |g| g.2), with), pct(n(false, |g| g.2), without));
    let real = with >= TIP_MIN_GROUP && without >= TIP_MIN_GROUP && rate_with - rate_without >= TIP_MIN_LIFT;
    Tip {
        signal: s.as_str().to_string(),
        pct_of_briefs: pct(n(true, |_| true), n(true, |_| true) + n(false, |_| true)),
        landed_with_without: real.then_some((rate_with, rate_without)),
    }
}

/// (id, display name) of every source, for labeling stored rows.
static SOURCE_NAMES: LazyLock<Vec<(&'static str, &'static str)>> =
    LazyLock::new(|| sources::all().iter().map(|s| (s.id(), s.name())).collect());

/// The overall score from the window's tasks (already in `b`) and corrections.
fn overall(b: &Briefs, catches: i64, humbled: i64) -> Overall {
    overall_of(b.window_tasks, b.window_landed, catches, humbled)
}

fn overall_of(continued: i64, landed: i64, catches: i64, humbled: i64) -> Overall {
    let score = Score::of(Window { continued, landed, catches, humbled });
    let tier = score.map(|s| s.tier);
    Overall {
        score: score.map(|s| s.score),
        direction: score.map(|s| s.direction),
        judgment: score.map(|s| s.judgment),
        tier,
        next_tier: tier.and_then(Tier::next),
        verdict: Answer::for_tier(tier),
        catches,
        humbled,
        to_rank: (MIN_BRIEFS - continued).max(0),
    }
}

impl Store {
    /// Just the overall score across all AI: a few queries, for goals and the tray.
    pub fn overall(&self, today: NaiveDate) -> Result<Overall> {
        let v = View { conn: self.conn(), clause: String::new(), args: vec![] };
        let start = score_start(None, today);
        let (continued, landed) = window_counts(&v, start)?;
        let window = start.to_string();
        let catches = v.sum(&format!("AND kind IN {} AND day >= ?", *CATCH_KINDS), &[&window])?;
        let humbled = v.sum("AND kind = 'humbled' AND day >= ?", &[&window])?;
        Ok(overall_of(continued, landed, catches, humbled))
    }

    /// The scorecard for `f`, with "this week" ending on `today`.
    pub fn stats(&self, f: &Filter, today: NaiveDate) -> Result<Stats> {
        let (clause, args) = f.sql();
        let v = View { conn: self.conn(), clause, args };
        let (scope_clause, scope_args) = Filter { since: None, ..f.clone() }.sql();
        let scope = View { conn: self.conn(), clause: scope_clause, args: scope_args };
        let start = score_start(f.since, today);
        let t = totals(&v, &scope, today, start)?;
        let (first_day, last_day) = day_range(&v)?;
        let (labels, categories) = labels_and_categories(&v)?;
        let (real_chats, real_with_catch) = real_chats(self.conn(), f.source.as_deref(), f.since)?;
        let (best_streak_days, current_streak_days) = streaks(&catch_days(&v)?, today);
        let briefs = briefs(&v, &scope, start)?;
        Ok(Stats {
            first_day,
            last_day,
            messages: t.messages,
            catches: t.catches,
            humbled: t.humbled,
            labels,
            catches_last_7d: t.catches_last_7d,
            overall: overall(&briefs, t.window_catches, t.window_humbled),
            messages_per_catch: (t.catches > 0).then(|| (t.messages as f64 / t.catches as f64).round() as i64),
            real_chats,
            pct_real_chats_with_catch: (real_chats > 0).then(|| pct(real_with_catch, real_chats)),
            best_streak_days,
            current_streak_days,
            peak_hour: if t.catches > 0 { peak_hour(&catch_hours(&v)?) } else { None },
            categories,
            weekly: weekly(&v)?,
            models: models(&v)?,
            hall_of_fame: hall_of_fame(&v)?,
            briefs,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    #[test]
    fn catch_kinds_come_from_the_labels() {
        assert_eq!(CATCH_KINDS.as_str(), "('confirmed','admitted','claimed')");
    }

    #[test]
    fn streaks_break_on_gaps_and_yesterday_still_counts() {
        let days = [d("2026-10-01"), d("2026-10-02"), d("2026-10-03"), d("2026-10-05"), d("2026-10-06")];
        assert_eq!(streaks(&days, d("2026-10-06")), (3, 2));
        assert_eq!(streaks(&days, d("2026-10-07")), (3, 2), "yesterday keeps the streak alive");
        assert_eq!(streaks(&days, d("2026-10-08")), (3, 0));
        assert_eq!(streaks(&[], d("2026-10-08")), (0, 0));
    }

    #[test]
    fn peak_hour_prefers_the_later_tie_and_needs_a_catch() {
        let mut h = [0i64; 24];
        assert_eq!(peak_hour(&h), None);
        h[9] = 2;
        h[14] = 2;
        h[3] = 1;
        assert_eq!(peak_hour(&h), Some(14));
    }

    #[test]
    fn weeks_start_on_monday() {
        assert_eq!(monday_of(d("2026-10-25")), d("2026-10-19"), "Sunday belongs to the week before");
        assert_eq!(monday_of(d("2026-10-26")), d("2026-10-26"));
        assert_eq!(monday_of(d("2026-10-28")), d("2026-10-26"));
    }

    #[test]
    fn overall_mixes_the_window_and_waits_for_enough_tasks() {
        let b = |continued, landed| Briefs {
            window_tasks: continued,
            window_landed: landed,
            to_rank: (MIN_BRIEFS - continued).max(0),
            ..Briefs::default()
        };
        let o = overall(&b(150, 125), 41, 2);
        assert_eq!(
            (o.score, o.tier, o.next_tier, o.verdict),
            (Some(86), Some(Tier::SharpEyed), Some(Tier::Irreplaceable), Answer::No)
        );
        let o = overall(&b(4, 4), 9, 0);
        assert_eq!((o.score, o.tier, o.verdict, o.to_rank, o.catches), (None, None, Answer::Hmm, MIN_BRIEFS - 4, 9));
    }

    #[test]
    fn filter_binds_only_what_is_set() {
        let f = Filter { provider: Some("OpenAI".into()), version: Some("5.1".into()), ..Default::default() };
        assert_eq!(f.sql(), (" AND provider = ? AND version = ?".into(), vec!["OpenAI".into(), "5.1".into()]));
        assert_eq!(Filter::default().sql(), (String::new(), vec![]));
    }
}
