//! What your own history says works, your personal bests, and how your report cards moved.

use crate::cards::{current, Current};
use crate::progress::{floor_pct, HistoryWeek, MIN_CARD_BEST, MIN_CARD_HALF, MIN_WEEK};
use crate::stats::{steer_totals, task_groups, tip_for, View, CONTINUED, LANDED};
use crate::tips::{self, TipTrend, TopTip};
use crate::{Result, Tip};
use chrono::NaiveDate;
use engine::{habit_bits, Grade, Signal, Steer, MIN_PROMPTS};
use serde::Serialize;

/// "What works for you" compares models only when each has this many continued tasks...
const INSIGHT_MODEL_MIN: i64 = 20;
/// ...and they differ by at least this many points.
const INSIGHT_MODEL_GAP: i64 = 10;
/// A steering insight needs this many steering messages that were on you.
const INSIGHT_STEER_MIN: i64 = 10;

/// A habit that went from missed to met more often.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HabitDrop {
    /// The habit's name.
    pub habit: String,
    /// Tasks of the first [`MIN_CARD_HALF`] that missed it.
    pub first: i64,
    /// Tasks of the last [`MIN_CARD_HALF`] that missed it.
    pub last: i64,
}

/// "Am I getting better?" across all report cards under the current model filter: your first
/// [`MIN_CARD_HALF`] graded tasks against your latest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CardTrend {
    /// Percent of the first tasks that got an A or B.
    pub first_ab: i64,
    /// Percent of the latest tasks that got an A or B.
    pub last_ab: i64,
    /// The habit that dropped most between them. `None` when none dropped.
    pub habit: Option<HabitDrop>,
}

/// Your best numbers, each with when it happened.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Bests {
    /// Best landed percent over any 4 weeks with enough tasks, and the last Monday of them.
    pub landed: Option<(i64, NaiveDate)>,
    /// Best share of specific prompts in a week with enough of them.
    pub sharp: Option<(i64, NaiveDate)>,
    /// Longest run of goal weeks met.
    pub goal_streak: i64,
    /// Best share of graded tasks that got an A or B in a week with enough of them.
    pub report_card: Option<(i64, NaiveDate)>,
}

/// One model's landed rate, for comparing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelLanded {
    /// Family ("Claude Opus").
    pub family: String,
    /// Percent of its continued tasks that landed, rounded down.
    pub landed: i64,
    /// Continued tasks.
    pub continued: i64,
}

/// What your own history says works, each only when there's enough of it.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Insights {
    /// Signals whose prompts landed clearly more often than prompts without them.
    pub signals: Vec<Tip>,
    /// The kind of steering that was on you most often, and its share of that steering.
    pub steer: Option<(Steer, i64)>,
    /// The best and worst landing models, when they differ enough.
    pub models: Vec<ModelLanded>,
    /// How your report cards moved, over all your history.
    pub report_cards: Option<CardTrend>,
    /// The tip you get most, from your latest ones.
    pub top_tip: Option<TopTip>,
    /// How many tasks needed a tip, last full week against the week before.
    pub tip_trend: Option<TipTrend>,
}

/// Graded tasks that got an A or B, as a rounded-down percent of `rows`.
fn ab_pct(rows: &[&Current]) -> i64 {
    let good = rows.iter().filter(|c| c.grade.is_ab()).count();
    floor_pct(good as i64, rows.len() as i64)
}

/// Your first and latest [`MIN_CARD_HALF`] graded tasks, compared, once there are twice as many as
/// it takes to fill both halves.
fn card_trend(v: &View) -> Result<Option<CardTrend>> {
    let rows = current(v, "ORDER BY start_ts, key")?;
    let rows: Vec<&Current> = rows.iter().collect();
    let half = MIN_CARD_HALF as usize;
    if rows.len() < 2 * half {
        return Ok(None);
    }
    let (first, last) = (&rows[..half], &rows[rows.len() - half..]);
    let missed = |part: &[&Current], bit: u8| part.iter().filter(|c| c.missing & bit != 0).count() as i64;
    let habit = habit_bits()
        .into_iter()
        .map(|(name, bit)| HabitDrop { habit: name.to_string(), first: missed(first, bit), last: missed(last, bit) })
        .filter(|h| h.first > h.last)
        // Reversed, because the last of equal drops wins and the habit on the card first should.
        .rev()
        .max_by_key(|h| h.first - h.last);
    Ok(Some(CardTrend { first_ab: ab_pct(first), last_ab: ab_pct(last), habit }))
}

/// Best share of graded tasks with an A or B in a week with [`MIN_CARD_BEST`] of them. Ties: the
/// most recent, like the other bests.
pub(crate) fn card_best(weeks: &[HistoryWeek]) -> Best {
    weeks
        .iter()
        .filter(|w| w.report_cards.graded >= MIN_CARD_BEST)
        .filter_map(|w| Some((w.report_cards.ab_pct()?, w.week)))
        .max_by_key(|(p, d)| (*p, *d))
}

/// A percent and the week it happened.
type Best = Option<(i64, NaiveDate)>;

/// Best 4-week landed rate and best specific week.
pub(crate) fn bests(weeks: &[HistoryWeek]) -> (Best, Best) {
    let landed = weeks
        .windows(4)
        .filter_map(|ws| {
            let (c, l): (i64, i64) = ws.iter().fold((0, 0), |(c, l), w| (c + w.continued, l + w.landed));
            (c >= MIN_PROMPTS).then(|| (floor_pct(l, c), ws[3].week))
        })
        // Ties: the most recent, so beating your best by matching it today still shows today.
        .max_by_key(|(p, d)| (*p, *d));
    let sharp = weeks
        .iter()
        .filter(|w| w.tasks >= MIN_WEEK)
        .map(|w| (floor_pct(w.grades.get(Grade::Sharp.as_str()).copied().unwrap_or(0), w.tasks), w.week))
        .max_by_key(|(p, d)| (*p, *d));
    (landed, sharp)
}

/// `cards` is the same filter without the range: the improvement headline and the tips cover all
/// your history.
pub(crate) fn insights(v: &View, cards: &View, today: NaiveDate) -> Result<Insights> {
    let groups = task_groups(v)?;
    let signals =
        Signal::ALL.into_iter().map(|s| tip_for(&groups, s)).filter(|t| t.landed_with_without.is_some()).collect();

    let counts: Vec<(Steer, i64)> = steer_totals(v)?.into_iter().filter(|(c, _)| c.is_on_you()).collect();
    let total: i64 = counts.iter().map(|c| c.1).sum();
    let steer = (total >= INSIGHT_STEER_MIN)
        .then(|| counts.iter().max_by_key(|c| c.1).map(|(c, n)| (*c, floor_pct(*n, total))))
        .flatten();

    let sql = format!(
        "SELECT family, COALESCE(SUM({CONTINUED}), 0), COALESCE(SUM({}), 0) FROM tasks WHERE 1=1 {} GROUP BY family",
        LANDED.as_str(),
        v.clause
    );
    let mut families: Vec<ModelLanded> = v
        .rows(&sql, &[], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))?
        .into_iter()
        .filter(|(family, c, _)| !family.is_empty() && *c >= INSIGHT_MODEL_MIN)
        .map(|(family, continued, landed)| ModelLanded { family, landed: floor_pct(landed, continued), continued })
        .collect();
    families.sort_by_key(|m| std::cmp::Reverse((m.landed, m.continued)));
    let models = match (families.first(), families.last()) {
        (Some(best), Some(worst)) if families.len() >= 2 && best.landed - worst.landed >= INSIGHT_MODEL_GAP => {
            vec![best.clone(), worst.clone()]
        }
        _ => vec![],
    };
    Ok(Insights {
        signals,
        steer,
        models,
        report_cards: card_trend(cards)?,
        top_tip: tips::top_tip(cards)?,
        tip_trend: tips::trend(cards, today)?,
    })
}
