//! Read side: everything the scorecard, quick glance and model breakdown show.

use crate::{Result, Store};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Timelike};
use engine::{Answer, Category, Label, Tier};
use rusqlite::{params_from_iter, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

/// A chat with at least this many messages counts as a real one.
const REAL_CHAT: i64 = 10;
/// Entries in the hall of fame.
const HALL_SIZE: i64 = 12;

/// The labels that count as catches, as an SQL list: `('confirmed','admitted','claimed')`.
static CATCH_KINDS: LazyLock<String> = LazyLock::new(|| {
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
}

impl Filter {
    /// `AND col = ?` for each set field, and the values to bind, in order.
    fn sql(&self) -> (String, Vec<String>) {
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
        (sql, args)
    }
}

/// One model's numbers.
#[derive(Debug, Serialize)]
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
}

/// One week's numbers.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct WeekRow {
    /// The week's Monday.
    pub week: NaiveDate,
    /// Messages that week.
    pub messages: i64,
    /// Catches that week.
    pub catches: i64,
}

/// One catch (or humbled moment) shown in the hall of fame.
#[derive(Debug, Serialize)]
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
    /// This week's tier.
    pub tier: Tier,
    /// The next tier and how many more catches this week reach it.
    pub next_tier: Option<(Tier, i64)>,
    /// "Could AI replace you?"
    pub verdict: Answer,
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
}

/// The daily / events rows one filter selects.
struct View<'a> {
    conn: &'a Connection,
    /// `AND …` clause; works on `daily` and `events` (same column names).
    clause: String,
    args: Vec<String>,
}

impl View<'_> {
    /// Runs `sql` (with `{w}` already holding the filter clause) binding the filter's values
    /// and then `extra`, in that order.
    fn rows<T>(
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
}

fn totals(v: &View, today: NaiveDate) -> Result<Totals> {
    let kinds = CATCH_KINDS.as_str();
    let week_ago = (today - Duration::days(6)).to_string();
    Ok(Totals {
        messages: v.sum("AND kind = 'message'", &[])?,
        catches: v.sum(&format!("AND kind IN {kinds}"), &[])?,
        humbled: v.sum("AND kind = 'humbled'", &[])?,
        catches_last_7d: v.sum(&format!("AND kind IN {kinds} AND day >= ?"), &[&week_ago])?,
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
/// respects the source filter alone.
fn real_chats(conn: &Connection, source: Option<&str>) -> Result<(i64, i64)> {
    Ok(conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(catches > 0), 0) FROM sessions WHERE messages >= ?1 AND (?2 IS NULL OR source = ?2)",
        rusqlite::params![REAL_CHAT, source],
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
fn monday_of(d: NaiveDate) -> NaiveDate {
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

/// (id, display name) of every source, for labeling stored rows.
static SOURCE_NAMES: LazyLock<Vec<(&'static str, &'static str)>> =
    LazyLock::new(|| sources::all().iter().map(|s| (s.id(), s.name())).collect());

/// The tier `catches_last_7d` reaches, and how far the next one is.
fn tier_progress(catches_last_7d: i64) -> (Tier, Option<(Tier, i64)>) {
    let tier = Tier::for_catches(catches_last_7d);
    (tier, tier.next().map(|next| (next, next.floor() - catches_last_7d)))
}

impl Store {
    /// The scorecard for `f`, with "this week" ending on `today`.
    pub fn stats(&self, f: &Filter, today: NaiveDate) -> Result<Stats> {
        let (clause, args) = f.sql();
        let v = View { conn: self.conn(), clause, args };
        let t = totals(&v, today)?;
        let (first_day, last_day) = day_range(&v)?;
        let (labels, categories) = labels_and_categories(&v)?;
        let (real_chats, real_with_catch) = real_chats(self.conn(), f.source.as_deref())?;
        let (best_streak_days, current_streak_days) = streaks(&catch_days(&v)?, today);
        let (tier, next_tier) = tier_progress(t.catches_last_7d);
        Ok(Stats {
            first_day,
            last_day,
            messages: t.messages,
            catches: t.catches,
            humbled: t.humbled,
            labels,
            catches_last_7d: t.catches_last_7d,
            tier,
            next_tier,
            verdict: Answer::for_catches(t.catches_last_7d),
            messages_per_catch: (t.catches > 0).then(|| (t.messages as f64 / t.catches as f64).round() as i64),
            real_chats,
            pct_real_chats_with_catch: (real_chats > 0)
                .then(|| (1000.0 * real_with_catch as f64 / real_chats as f64).round() / 10.0),
            best_streak_days,
            current_streak_days,
            peak_hour: if t.catches > 0 { peak_hour(&catch_hours(&v)?) } else { None },
            categories,
            weekly: weekly(&v)?,
            models: models(&v)?,
            hall_of_fame: hall_of_fame(&v)?,
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
    fn next_tier_counts_what_is_left() {
        assert_eq!(tier_progress(0), (Tier::RubberStamp, Some((Tier::Coworker, 3))));
        assert_eq!(tier_progress(5), (Tier::Coworker, Some((Tier::SharpEyed, 2))));
        assert_eq!(tier_progress(15), (Tier::Irreplaceable, None));
    }

    #[test]
    fn filter_binds_only_what_is_set() {
        let f = Filter { provider: Some("OpenAI".into()), version: Some("5.1".into()), ..Default::default() };
        assert_eq!(f.sql(), (" AND provider = ? AND version = ?".into(), vec!["OpenAI".into(), "5.1".into()]));
        assert_eq!(Filter::default().sql(), (String::new(), vec![]));
    }
}
