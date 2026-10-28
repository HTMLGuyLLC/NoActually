//! Read side: everything the scorecard, quick glance and model breakdown show.

use crate::Store;
use anyhow::Result;
use chrono::{Datelike, Duration, Local, NaiveDate, Timelike};
use rusqlite::params_from_iter;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const TIERS: [(i64, &str); 4] = [(15, "Irreplaceable"), (7, "Sharp-eyed"), (3, "Coworker"), (0, "Rubber stamp")];
const REAL_CHAT: i64 = 10;
const CATCH_KINDS: &str = "('confirmed','admitted','claimed')";

/// Narrow any view to a source and/or a model at any level (provider → family → version).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Filter {
    pub source: Option<String>,
    pub provider: Option<String>,
    pub family: Option<String>,
    pub version: Option<String>,
}

impl Filter {
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
                sql.push_str(&format!(" AND {col} = ?"));
                args.push(v.clone());
            }
        }
        (sql, args)
    }
}

#[derive(Debug, Serialize)]
pub struct ModelRow {
    pub provider: String,
    pub family: String,
    pub version: String,
    pub messages: i64,
    pub catches: i64,
    pub humbled: i64,
}

#[derive(Debug, Serialize)]
pub struct WeekRow {
    pub week: NaiveDate,
    pub messages: i64,
    pub catches: i64,
}

#[derive(Debug, Serialize)]
pub struct HallEntry {
    pub id: i64,
    pub day: String,
    pub source: String,
    pub model: String,
    pub label: String,
    pub category: String,
    pub human: Option<String>,
    pub ai: Option<String>,
    pub pinned: bool,
}

#[derive(Debug, Serialize)]
pub struct Stats {
    pub first_day: Option<String>,
    pub last_day: Option<String>,
    pub messages: i64,
    pub catches: i64,
    pub humbled: i64,
    pub labels: BTreeMap<String, i64>,
    pub catches_last_7d: i64,
    pub tier: &'static str,
    pub next_tier: Option<(&'static str, i64)>,
    pub messages_per_catch: Option<i64>,
    pub real_chats: i64,
    pub pct_real_chats_with_catch: Option<f64>,
    pub best_streak_days: i64,
    pub current_streak_days: i64,
    pub peak_hour: Option<u32>,
    pub categories: BTreeMap<String, i64>,
    pub weekly: Vec<WeekRow>,
    pub models: Vec<ModelRow>,
    pub hall_of_fame: Vec<HallEntry>,
}

impl Store {
    pub fn stats(&self, f: &Filter, today: NaiveDate) -> Result<Stats> {
        let c = self.conn();
        let (w, args) = f.sql();
        let sum = |extra: &str| -> Result<i64> {
            let q = format!("SELECT COALESCE(SUM(n), 0) FROM daily WHERE 1=1 {w} {extra}");
            Ok(c.query_row(&q, params_from_iter(&args), |r| r.get(0))?)
        };
        let messages = sum("AND kind = 'message'")?;
        let catches = sum(&format!("AND kind IN {CATCH_KINDS}"))?;
        let humbled = sum("AND kind = 'humbled'")?;
        let week_ago = (today - Duration::days(6)).to_string();
        let catches_last_7d = sum(&format!("AND kind IN {CATCH_KINDS} AND day >= '{week_ago}'"))?;
        let tier = TIERS.iter().find(|(floor, _)| catches_last_7d >= *floor).map_or("Rubber stamp", |t| t.1);
        let next_tier = TIERS
            .iter()
            .rev()
            .find(|(floor, _)| *floor > catches_last_7d)
            .map(|(floor, name)| (*name, floor - catches_last_7d));

        let (first_day, last_day): (Option<String>, Option<String>) = c.query_row(
            &format!("SELECT MIN(day), MAX(day) FROM daily WHERE 1=1 {w}"),
            params_from_iter(&args),
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;

        let mut labels = BTreeMap::new();
        let mut categories = BTreeMap::new();
        let mut stmt = c.prepare(&format!(
            "SELECT kind, category, SUM(n) FROM daily WHERE kind != 'message' {w} GROUP BY kind, category"
        ))?;
        for row in stmt.query_map(params_from_iter(&args), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
        })? {
            let (kind, cat, n) = row?;
            *labels.entry(kind.clone()).or_insert(0) += n;
            if kind != "humbled" && !cat.is_empty() {
                *categories.entry(cat).or_insert(0) += n;
            }
        }

        // Chats only know their source, not a model, so this respects the source filter alone.
        let (real_chats, real_with_catch): (i64, i64) = c.query_row(
            "SELECT COUNT(*), COALESCE(SUM(catches > 0), 0) FROM sessions WHERE messages >= ?1 AND (?2 IS NULL OR source = ?2)",
            rusqlite::params![REAL_CHAT, f.source],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;

        // Streaks over days that had at least one catch.
        let mut days: Vec<NaiveDate> = c
            .prepare(&format!("SELECT DISTINCT day FROM daily WHERE kind IN {CATCH_KINDS} {w} ORDER BY day"))?
            .query_map(params_from_iter(&args), |r| r.get::<_, String>(0))?
            .filter_map(|d| d.ok()?.parse().ok())
            .collect();
        days.dedup();
        let (mut best, mut run) = (0i64, 0i64);
        for (i, d) in days.iter().enumerate() {
            run = if i > 0 && *d - days[i - 1] == Duration::days(1) { run + 1 } else { 1 };
            best = best.max(run);
        }
        let current = match days.last() {
            Some(d) if today - *d <= Duration::days(1) => run,
            _ => 0,
        };

        let mut hours = [0i64; 24];
        let mut stmt = c.prepare(&format!("SELECT ts FROM events WHERE label IN {CATCH_KINDS} {w}"))?;
        for ts in stmt.query_map(params_from_iter(&args), |r| r.get::<_, String>(0))? {
            if let Ok(t) = chrono::DateTime::parse_from_rfc3339(&ts?) {
                hours[t.with_timezone(&Local).hour() as usize] += 1;
            }
        }
        let peak_hour = (catches > 0).then(|| (0..24).max_by_key(|h| hours[*h as usize]).unwrap());

        let mut weeks: BTreeMap<NaiveDate, (i64, i64)> = BTreeMap::new();
        let mut stmt = c.prepare(&format!(
            "SELECT day, SUM(CASE WHEN kind = 'message' THEN n ELSE 0 END), SUM(CASE WHEN kind IN {CATCH_KINDS} THEN n ELSE 0 END)
             FROM daily WHERE 1=1 {w} GROUP BY day"
        ))?;
        for row in stmt.query_map(params_from_iter(&args), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
        })? {
            let (day, m, k) = row?;
            let Ok(d) = day.parse::<NaiveDate>() else { continue };
            let monday = d - Duration::days(d.weekday().num_days_from_monday() as i64);
            let e = weeks.entry(monday).or_default();
            e.0 += m;
            e.1 += k;
        }
        let weekly = weeks.into_iter().map(|(week, (messages, catches))| WeekRow { week, messages, catches }).collect();

        let mut stmt = c.prepare(&format!(
            "SELECT provider, family, version,
                SUM(CASE WHEN kind = 'message' THEN n ELSE 0 END),
                SUM(CASE WHEN kind IN {CATCH_KINDS} THEN n ELSE 0 END),
                SUM(CASE WHEN kind = 'humbled' THEN n ELSE 0 END)
             FROM daily WHERE 1=1 {w} GROUP BY provider, family, version ORDER BY 5 DESC, 4 DESC"
        ))?;
        let models = stmt
            .query_map(params_from_iter(&args), |r| {
                Ok(ModelRow {
                    provider: r.get(0)?,
                    family: r.get(1)?,
                    version: r.get(2)?,
                    messages: r.get(3)?,
                    catches: r.get(4)?,
                    humbled: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let mut stmt = c.prepare(&format!(
            "SELECT id, day, source, family || CASE WHEN version = '' THEN '' ELSE ' ' || version END, label, category,
                COALESCE(human_full, human_snip), COALESCE(ai_full, ai_snip), pinned
             FROM events WHERE level != 'category' {w}
             ORDER BY pinned DESC, label = 'confirmed' DESC, ts DESC LIMIT 12"
        ))?;
        let hall_of_fame = stmt
            .query_map(params_from_iter(&args), |r| {
                Ok(HallEntry {
                    id: r.get(0)?,
                    day: r.get(1)?,
                    source: r.get(2)?,
                    model: r.get(3)?,
                    label: r.get(4)?,
                    category: r.get(5)?,
                    human: r.get(6)?,
                    ai: r.get(7)?,
                    pinned: r.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        Ok(Stats {
            first_day,
            last_day,
            messages,
            catches,
            humbled,
            labels,
            catches_last_7d,
            tier,
            next_tier,
            messages_per_catch: (catches > 0).then(|| (messages as f64 / catches as f64).round() as i64),
            real_chats,
            pct_real_chats_with_catch: (real_chats > 0)
                .then(|| (1000.0 * real_with_catch as f64 / real_chats as f64).round() / 10.0),
            best_streak_days: best,
            current_streak_days: current,
            peak_hour,
            categories,
            weekly,
            models,
            hall_of_fame,
        })
    }

    /// Total catches ever, for milestone popups (10th, 50th, 100th…).
    pub fn total_catches(&self) -> Result<i64> {
        Ok(self.conn().query_row(
            &format!("SELECT COALESCE(SUM(n),0) FROM daily WHERE kind IN {CATCH_KINDS}"),
            [],
            |r| r.get(0),
        )?)
    }
}
