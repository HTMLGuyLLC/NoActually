//! Improve: how your numbers move week to week, the one goal you're working on, your personal
//! bests, and what works for you. Read side, plus the goal you set and the weeks it already
//! closed (frozen, so a later rebuild or import can't take back a week you earned).

use crate::cards::{current, gradable_ids, local_day, Current};
use crate::stats::{monday_of, steer_totals, tally, task_groups, tip_for, View, CONTINUED, CORRECTION_KINDS, LANDED};
use crate::tips::{self, TipTrend, TipWeek, TopTip};
use crate::{Error, Filter, Result, Store, Tip};
use chrono::{Duration, NaiveDate};
use engine::{habit_bits, Grade, LetterGrade, Signal, Signals, Steer, Tier, MIN_PROMPTS};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Prompts (or, for a steering goal, continued tasks) a week needs before it counts: for a chart
/// point, a goal week or a personal best. Fewer and one task swings it to 0% or 100%.
pub const MIN_WEEK: i64 = 5;
/// Graded tasks a week needs to be a solid point on the report card charts (quieter weeks are
/// hollow dots).
pub const MIN_CARD_WEEK: i64 = 3;
/// Graded tasks a week needs to be a personal best, or a straight-A's week.
pub const MIN_CARD_BEST: i64 = 5;
/// Graded tasks on each side of the improvement comparison: it needs twice this many.
pub const MIN_CARD_HALF: i64 = 20;
/// How many of the latest report cards a recent list shows (Improve, `stats --report-cards`).
pub const RECENT_REPORT_CARDS: usize = 5;
/// Weeks a habit or steering goal has to be met to be done.
pub const GOAL_WEEKS: i64 = 4;
/// Weeks of history a suggested goal's baseline looks at.
const BASELINE_WEEKS: i64 = 8;
/// A suggested habit goal asks for this many points more than your baseline...
const GOAL_STEP: i64 = 15;
/// ...but never more than this.
const GOAL_CAP: i64 = 80;
/// The highest ceiling a steering goal can have (more is no goal at all).
const STEER_CAP: i64 = 50;
/// "N more prompts" past this many (or past this week's count, if bigger) isn't reachable in a
/// week; it isn't shown.
const NEED_MAX: i64 = 50;
/// "What works for you" compares models only when each has this many continued tasks...
const INSIGHT_MODEL_MIN: i64 = 20;
/// ...and they differ by at least this many points.
const INSIGHT_MODEL_GAP: i64 = 10;
/// A steering insight needs this many steering messages that were on you.
const INSIGHT_STEER_MIN: i64 = 10;

const GOAL_KEY: &str = "goal";
const LEARNED_KEY: &str = "learn.done";
/// Lesson ids are short slugs; anything else is refused.
const LESSON_ID_MAX: usize = 48;
const LESSONS_MAX: usize = 200;

/// One week of history.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct HistoryWeek {
    /// The week's Monday.
    pub week: NaiveDate,
    /// Messages sent.
    pub messages: i64,
    /// Times you corrected it.
    pub corrections: i64,
    /// Times it corrected you.
    pub humbled: i64,
    /// Tasks started.
    pub tasks: i64,
    /// Of those, the ones that went past their first message.
    pub continued: i64,
    /// Continued tasks nothing on you steered.
    pub landed: i64,
    /// Prompts carrying each signal, by signal id.
    pub signals: BTreeMap<String, i64>,
    /// Prompts per grade id.
    pub grades: BTreeMap<String, i64>,
    /// Steering messages per cause id.
    pub steering: BTreeMap<String, i64>,
    /// Tasks steered at least once, per cause id.
    pub steered: BTreeMap<String, i64>,
    /// The week's report cards (current ones only).
    pub report_cards: ReportCardWeek,
    /// The week's tips.
    pub tips: TipWeek,
}

/// One week of report cards, from the cards that still count (a newer card for the same task
/// replaces them). Also a whole range of weeks, added up by [`Store::report_card_totals`].
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ReportCardWeek {
    /// Tasks with a report card that week.
    pub graded: i64,
    /// Graded tasks per grade, "A" to "F" (all five keys once any task is graded).
    pub grades: BTreeMap<String, i64>,
    /// Avoidable rounds, summed.
    pub avoidable: i64,
    /// Tokens redone because of avoidable rounds: new input, cache writes and output.
    pub extra_tokens: u64,
    /// What that work cost at list prices, in US dollars, from the fully priced tasks only (a task
    /// that used any model with no price is left out of it). `None` when no graded task is fully priced.
    pub extra_cost: Option<f64>,
    /// Of `graded`, the tasks every model of which has a price: the ones `extra_cost` counts. A cost
    /// per task divides by this, not by `graded`.
    pub priced: i64,
    /// Minutes of that work.
    pub extra_minutes: i64,
    /// Graded tasks that missed each habit, by name: target, evidence, finish, guardrails, purpose,
    /// context (all six keys once any task is graded).
    pub missing: BTreeMap<String, i64>,
    /// Tasks started that week in the sources that can have a card (`Source::gradable`).
    pub gradable_tasks: i64,
}

impl ReportCardWeek {
    /// A week with no cards yet, with all five grades and all six habits present at zero, so the
    /// shape is the same however few tasks were graded.
    pub fn with_keys() -> ReportCardWeek {
        ReportCardWeek {
            grades: LetterGrade::ALL.iter().map(|g| (g.as_str().to_string(), 0)).collect(),
            missing: habit_bits().iter().map(|(name, _)| ((*name).to_string(), 0)).collect(),
            ..ReportCardWeek::default()
        }
    }

    /// Graded tasks that got `grade`.
    pub fn count(&self, grade: LetterGrade) -> i64 {
        self.grades.get(grade.as_str()).copied().unwrap_or(0)
    }

    /// Percent of the graded tasks that got an A or a B, rounded down. `None` when none are graded.
    pub fn ab_pct(&self) -> Option<i64> {
        (self.graded > 0).then(|| {
            let good: i64 = LetterGrade::ALL.iter().filter(|g| g.is_ab()).map(|g| self.count(*g)).sum();
            floor_pct(good, self.graded)
        })
    }

    /// Adds `other`'s numbers to these. The cost stays unknown only while neither has a price.
    fn add(&mut self, other: &ReportCardWeek) {
        self.graded += other.graded;
        self.avoidable += other.avoidable;
        self.extra_tokens += other.extra_tokens;
        self.extra_cost = match (self.extra_cost, other.extra_cost) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0.0) + b.unwrap_or(0.0)),
        };
        self.priced += other.priced;
        self.extra_minutes += other.extra_minutes;
        self.gradable_tasks += other.gradable_tasks;
        for (k, n) in &other.grades {
            *self.grades.entry(k.clone()).or_insert(0) += n;
        }
        for (k, n) in &other.missing {
            *self.missing.entry(k.clone()).or_insert(0) += n;
        }
    }
}

/// One of the latest report cards, a line in the recent list. Numbers only.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RecentReportCard {
    /// The day the task started.
    pub day: String,
    /// The day the card was written, in the local time zone: when it is "from". Settings shows the
    /// last card by this day.
    pub graded_day: NaiveDate,
    /// Source id.
    pub source: String,
    /// "A" to "F".
    pub grade: String,
    /// The first habit this card says the task missed, in the order the card lists them (target,
    /// evidence, finish line, guardrails, purpose, context). `None` when it missed none.
    pub missing: Option<String>,
    /// What the redone work cost at list prices, in US dollars; `None` for a model with no price.
    pub extra_cost: Option<f64>,
    /// Tokens redone: new input, cache writes and output.
    pub extra_tokens: u64,
    /// Minutes of work redone. Every card has this, priced or not.
    pub extra_minutes: i64,
}

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

/// What a goal asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GoalKind {
    /// At least `target`% of a week's prompts carry `signal`.
    Habit {
        /// The signal.
        signal: Signal,
    },
    /// At most `target`% of a week's continued tasks get this kind of steering.
    Steer {
        /// The cause; never `ai_wrong` (that's the AI's mistake, not yours).
        steer: Steer,
    },
    /// Reach this overall tier.
    Tier {
        /// The tier.
        tier: Tier,
    },
}

/// The one goal you're working on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Goal {
    /// When it was set, in milliseconds; also its id.
    pub id: i64,
    /// What it asks for.
    #[serde(flatten)]
    pub kind: GoalKind,
    /// Percent: the floor for a habit, the ceiling for steering. Unused for a tier.
    pub target: i64,
    /// The day it was set. Only tasks from then on count.
    pub start: NaiveDate,
}

impl Goal {
    /// Checks the goal makes sense before it's saved.
    pub fn validate(&self) -> Result<()> {
        let ok = match self.kind {
            GoalKind::Habit { .. } => (5..=95).contains(&self.target),
            GoalKind::Steer { steer } => steer.is_on_you() && (0..=STEER_CAP).contains(&self.target),
            GoalKind::Tier { tier } => tier > Tier::RubberStamp,
        };
        if ok {
            Ok(())
        } else {
            Err(Error::Invalid("That goal can't be measured."))
        }
    }
}

/// How one week of a goal went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeekState {
    /// Still under way.
    Open,
    /// Reached the target.
    Met,
    /// Had enough tasks and missed it.
    Missed,
    /// Too few tasks to count. Doesn't break a streak.
    Paused,
}

impl WeekState {
    fn as_str(self) -> &'static str {
        match self {
            WeekState::Open => "open",
            WeekState::Met => "met",
            WeekState::Missed => "missed",
            WeekState::Paused => "paused",
        }
    }

    fn parse(s: &str) -> Option<WeekState> {
        [WeekState::Open, WeekState::Met, WeekState::Missed, WeekState::Paused].into_iter().find(|w| w.as_str() == s)
    }
}

/// One week of a habit or steering goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct GoalWeek {
    /// The week's Monday.
    pub week: NaiveDate,
    /// Prompts (habit) or continued tasks (steering) counted.
    pub count: i64,
    /// Percent of them that carried the signal (habit) or got that steering (steering).
    pub value: i64,
    /// How it went.
    pub state: WeekState,
}

/// Where the goal stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GoalStatus {
    /// The goal.
    pub goal: Goal,
    /// Every week since it started, oldest first; the last one may be open.
    pub weeks: Vec<GoalWeek>,
    /// Weeks met so far.
    pub met: i64,
    /// Weeks met in a row, ending with the last closed week (paused weeks are skipped).
    pub streak: i64,
    /// Weeks it takes (habit and steering goals).
    pub needed: i64,
    /// Finished: met enough weeks, or the tier was reached.
    pub done: bool,
    /// Habit goals: more prompts carrying the signal this week would reach the target (none when
    /// that's out of reach this week).
    pub need: Option<i64>,
    /// Tier goals: the overall score now.
    pub score: Option<i64>,
}

/// What a sync changed about the goal, for popups.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GoalNews {
    /// Weeks just finished and met.
    pub newly_met: Vec<GoalWeek>,
    /// Weeks met in all, so far.
    pub met: i64,
    /// The goal, if it was just finished.
    pub finished: Option<Goal>,
}

/// A finished goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Trophy {
    /// The goal.
    pub goal: Goal,
    /// The day it was finished.
    pub done: NaiveDate,
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

/// Everything the Improve tab shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Progress {
    /// Every week from the first with data to this one (empty weeks included), for the filter.
    pub weeks: Vec<HistoryWeek>,
    /// The goal you set, across all AI.
    pub goal: Option<GoalStatus>,
    /// A goal to try, from your own numbers.
    pub suggestion: Goal,
    /// Every goal you could pick now, with targets from your own numbers.
    pub options: Vec<Goal>,
    /// Finished goals, newest first.
    pub trophies: Vec<Trophy>,
    /// Personal bests, across all AI.
    pub bests: Bests,
    /// What works for you, for the filter.
    pub insights: Insights,
}

/// One count per steering cause, in [`Steer::ALL`] order.
type PerSteer = [i64; Steer::ALL.len()];

type TaskRow = (String, Signals, bool, bool, i64, PerSteer, PerSteer);

/// Tasks per week (from `since`, when given), plus messages and corrections from `daily`.
fn task_weeks(v: &View, since: Option<NaiveDate>) -> Result<BTreeMap<NaiveDate, HistoryWeek>> {
    let sums: Vec<String> = Steer::ALL.iter().map(|c| format!("SUM({})", c.as_str())).collect();
    let any: Vec<String> = Steer::ALL.iter().map(|c| format!("SUM({} > 0)", c.as_str())).collect();
    let after = if since.is_some() { " AND day >= ?" } else { "" };
    let sql = format!(
        "SELECT day, signals, {CONTINUED}, {}, COUNT(*), {}, {} FROM tasks WHERE 1=1 {}{after} GROUP BY 1, 2, 3, 4",
        LANDED.as_str(),
        sums.join(", "),
        any.join(", "),
        v.clause
    );
    let since_s = since.map(|d| d.to_string());
    let extra: Vec<&str> = since_s.iter().map(String::as_str).collect();
    let rows: Vec<TaskRow> = v.rows(&sql, &extra, |r| {
        let per_steer = |at: usize| -> rusqlite::Result<PerSteer> {
            let mut out = [0; Steer::ALL.len()];
            for (i, n) in out.iter_mut().enumerate() {
                *n = r.get(at + i)?;
            }
            Ok(out)
        };
        Ok((
            r.get(0)?,
            Signals(r.get(1)?),
            r.get(2)?,
            r.get(3)?,
            r.get(4)?,
            per_steer(5)?,
            per_steer(5 + Steer::ALL.len())?,
        ))
    })?;
    let mut weeks: BTreeMap<NaiveDate, HistoryWeek> = BTreeMap::new();
    for (day, signals, continued, landed, n, steering, steered) in rows {
        let Ok(d) = day.parse::<NaiveDate>() else { continue };
        let w =
            weeks.entry(monday_of(d)).or_insert_with(|| HistoryWeek { week: monday_of(d), ..HistoryWeek::default() });
        w.tasks += n;
        w.continued += if continued { n } else { 0 };
        w.landed += if landed { n } else { 0 };
        tally(&mut w.grades, &mut w.signals, signals, n);
        for (i, c) in Steer::ALL.iter().enumerate() {
            *w.steering.entry(c.as_str().to_string()).or_insert(0) += steering[i];
            *w.steered.entry(c.as_str().to_string()).or_insert(0) += steered[i];
        }
    }
    Ok(weeks)
}

/// Every week from the first with any data to `today`'s, oldest first.
fn history(v: &View, today: NaiveDate) -> Result<Vec<HistoryWeek>> {
    let mut weeks = task_weeks(v, None)?;
    let sql = format!(
        "SELECT day, SUM(CASE WHEN kind = 'message' THEN n ELSE 0 END), SUM(CASE WHEN kind IN {} THEN n ELSE 0 END),
            SUM(CASE WHEN kind = 'humbled' THEN n ELSE 0 END)
         FROM daily WHERE 1=1 {} GROUP BY day",
        *CORRECTION_KINDS, v.clause
    );
    let days: Vec<(String, i64, i64, i64)> = v.rows(&sql, &[], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    for (day, messages, corrections, humbled) in days {
        let Ok(d) = day.parse::<NaiveDate>() else { continue };
        let w =
            weeks.entry(monday_of(d)).or_insert_with(|| HistoryWeek { week: monday_of(d), ..HistoryWeek::default() });
        w.messages += messages;
        w.corrections += corrections;
        w.humbled += humbled;
    }
    for (week, report_cards) in card_weeks(v)? {
        weeks.entry(week).or_insert_with(|| HistoryWeek { week, ..HistoryWeek::default() }).report_cards = report_cards;
    }
    for (week, tips) in tips::weeks(v)? {
        weeks.entry(week).or_insert_with(|| HistoryWeek { week, ..HistoryWeek::default() }).tips = tips;
    }
    let this_week = monday_of(today);
    // A clock set wrong once can write future days: they'd stretch the chart, so they're dropped.
    weeks.retain(|w, _| *w <= this_week);
    let Some(first) = weeks.keys().next().copied() else { return Ok(vec![]) };
    let mut out = Vec::new();
    let mut w = first;
    while w <= this_week {
        out.push(weeks.remove(&w).unwrap_or_else(|| HistoryWeek { week: w, ..HistoryWeek::default() }));
        w += Duration::days(7);
    }
    Ok(out)
}

/// Report cards per week from the cards that still count, plus the tasks that could have one.
pub(crate) fn card_weeks(v: &View) -> Result<BTreeMap<NaiveDate, ReportCardWeek>> {
    let mut weeks: BTreeMap<NaiveDate, ReportCardWeek> = BTreeMap::new();
    let habits = habit_bits();
    // In a fixed order, so the cost adds up the same way every time.
    for c in current(v, "ORDER BY start_ts, key")? {
        let Ok(day) = c.day.parse::<NaiveDate>() else { continue };
        let w = weeks.entry(monday_of(day)).or_insert_with(ReportCardWeek::with_keys);
        w.graded += 1;
        *w.grades.entry(c.grade.as_str().to_string()).or_insert(0) += 1;
        w.avoidable += c.avoidable;
        w.extra_tokens += c.shown_tokens();
        if let Some(cost) = c.cost() {
            *w.extra_cost.get_or_insert(0.0) += cost;
            w.priced += 1;
        }
        w.extra_minutes += c.minutes;
        for (name, bit) in habits {
            if c.missing & bit != 0 {
                *w.missing.entry(name.to_string()).or_insert(0) += 1;
            }
        }
    }
    let sources: Vec<String> = gradable_ids().iter().map(|s| format!("'{s}'")).collect();
    let sql =
        format!("SELECT day, COUNT(*) FROM tasks WHERE source IN ({}) {} GROUP BY day", sources.join(","), v.clause);
    for (day, n) in v.rows(&sql, &[], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
        if let Ok(day) = day.parse::<NaiveDate>() {
            weeks.entry(monday_of(day)).or_default().gradable_tasks += n; // no keys until a task is graded
        }
    }
    Ok(weeks)
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
fn card_best(weeks: &[HistoryWeek]) -> Best {
    weeks
        .iter()
        .filter(|w| w.report_cards.graded >= MIN_CARD_BEST)
        .filter_map(|w| Some((w.report_cards.ab_pct()?, w.week)))
        .max_by_key(|(p, d)| (*p, *d))
}

/// How a habit or steering goal did in `w`: (count, value).
fn goal_value(kind: GoalKind, w: &HistoryWeek) -> (i64, i64) {
    match kind {
        GoalKind::Habit { signal } => {
            (w.tasks, floor_pct(w.signals.get(signal.as_str()).copied().unwrap_or(0), w.tasks))
        }
        GoalKind::Steer { steer } => {
            (w.continued, ceil_pct(w.steered.get(steer.as_str()).copied().unwrap_or(0), w.continued))
        }
        GoalKind::Tier { .. } => (0, 0),
    }
}

/// Percent rounded down (a habit target is a floor: 49.9% isn't 50%).
pub fn floor_pct(n: i64, total: i64) -> i64 {
    if total > 0 {
        100 * n / total
    } else {
        0
    }
}

/// Percent rounded up (a steering target is a ceiling: 10.1% isn't 10%).
fn ceil_pct(n: i64, total: i64) -> i64 {
    if total > 0 {
        (100 * n + total - 1) / total
    } else {
        0
    }
}

/// How a week went. A week isn't judged until it's over: reaching the target on Monday doesn't
/// count if the rest of the week undoes it.
fn week_state(goal: &Goal, count: i64, value: i64, closed: bool) -> WeekState {
    if !closed {
        return WeekState::Open;
    }
    let met = match goal.kind {
        GoalKind::Habit { .. } => value >= goal.target,
        GoalKind::Steer { .. } => value <= goal.target,
        GoalKind::Tier { .. } => false,
    };
    match (count >= MIN_WEEK, met) {
        (false, _) => WeekState::Paused,
        (true, true) => WeekState::Met,
        (true, false) => WeekState::Missed,
    }
}

/// More prompts carrying the signal this week that would reach a habit target (and the week's
/// minimum): the smallest `x` with `(k + x) / (n + x) >= target` and `n + x >= MIN_WEEK`, or
/// `None` past [`NEED_MAX`].
fn prompts_needed(target: i64, n: i64, k: i64) -> Option<i64> {
    let gap = target * n - 100 * k;
    let for_target = if gap <= 0 { 0 } else { (gap + (100 - target) - 1) / (100 - target).max(1) };
    let x = for_target.max(MIN_WEEK - n).max(0);
    (x <= NEED_MAX.max(n)).then_some(x)
}

/// A habit target: 15 points over your share, rounded up to 5, capped.
fn habit_target(share: i64) -> i64 {
    (((share + GOAL_STEP).min(GOAL_CAP) + 4) / 5 * 5).clamp(5, GOAL_CAP)
}

/// A steering ceiling: 5 points under your share, rounded down to 5, capped.
fn steer_target(share: i64) -> i64 {
    ((share - 5).max(0) / 5 * 5).min(STEER_CAP)
}

/// Weeks met in a row, ending with the last closed week; paused weeks are skipped, not breaks.
fn streak(weeks: &[GoalWeek]) -> i64 {
    let mut n = 0;
    for w in weeks.iter().rev() {
        match w.state {
            WeekState::Met => n += 1,
            WeekState::Paused | WeekState::Open => {}
            WeekState::Missed => break,
        }
    }
    n
}

fn longest_streak(states: impl IntoIterator<Item = WeekState>) -> i64 {
    let (mut best, mut run) = (0, 0);
    for s in states {
        match s {
            WeekState::Met => {
                run += 1;
                best = best.max(run);
            }
            WeekState::Missed => run = 0,
            WeekState::Paused | WeekState::Open => {}
        }
    }
    best
}

/// The last [`BASELINE_WEEKS`] weeks' share of prompts carrying each signal, and of continued
/// tasks steered each way.
struct Baseline {
    tasks: i64,
    signals: Vec<(Signal, i64)>,
    steers: Vec<(Steer, i64)>,
}

fn baseline(weeks: &[HistoryWeek]) -> Baseline {
    let recent: Vec<&HistoryWeek> = weeks.iter().rev().take(BASELINE_WEEKS as usize).collect();
    let tasks: i64 = recent.iter().map(|w| w.tasks).sum();
    let continued: i64 = recent.iter().map(|w| w.continued).sum();
    let sum = |f: &dyn Fn(&HistoryWeek) -> i64| recent.iter().map(|w| f(w)).sum::<i64>();
    let signals = Signal::ALL
        .into_iter()
        .map(|s| (s, floor_pct(sum(&|w| w.signals.get(s.as_str()).copied().unwrap_or(0)), tasks)))
        .collect();
    let steers = Steer::ALL
        .into_iter()
        .filter(|c| c.is_on_you())
        .map(|c| (c, ceil_pct(sum(&|w| w.steered.get(c.as_str()).copied().unwrap_or(0)), continued)))
        .collect();
    Baseline { tasks, signals, steers }
}

/// Every goal you could pick now: a habit for each signal, a ceiling for each kind of steering
/// that's on you, and the tiers above yours. Targets come from your own numbers.
fn options(weeks: &[HistoryWeek], today: NaiveDate, tier: Option<Tier>) -> Vec<Goal> {
    let b = baseline(weeks);
    let goal = |kind, target| Goal { id: 0, kind, target, start: today };
    let habits =
        b.signals.iter().map(|(signal, share)| goal(GoalKind::Habit { signal: *signal }, habit_target(*share)));
    let steers = b.steers.iter().map(|(steer, share)| goal(GoalKind::Steer { steer: *steer }, steer_target(*share)));
    let tiers = Tier::ALL
        .into_iter()
        .filter(|t| Some(*t) > tier && *t > Tier::RubberStamp)
        .map(|tier| goal(GoalKind::Tier { tier }, 0));
    habits.chain(steers).chain(tiers).collect()
}

/// A goal to try: the habit your prompts carry least (if any has room to grow), else the steering
/// that's on you most, else the next tier, else saying what done is.
fn suggest(weeks: &[HistoryWeek], today: NaiveDate, tier: Option<Tier>) -> Goal {
    let b = baseline(weeks);
    let goal = |kind, target| Goal { id: 0, kind, target, start: today };
    if b.tasks == 0 {
        return goal(GoalKind::Habit { signal: Signal::Done }, 30);
    }
    if let Some((signal, share)) = b.signals.iter().min_by_key(|(_, p)| *p).filter(|(_, p)| p + 5 <= GOAL_CAP) {
        return goal(GoalKind::Habit { signal: *signal }, habit_target(*share));
    }
    if let Some((steer, share)) = b.steers.iter().max_by_key(|(_, p)| *p).filter(|(_, p)| *p > 0) {
        return goal(GoalKind::Steer { steer: *steer }, steer_target(*share));
    }
    match tier.map_or(Some(Tier::Coworker), Tier::next) {
        Some(next) => goal(GoalKind::Tier { tier: next }, 0),
        None => goal(GoalKind::Habit { signal: Signal::Done }, habit_target(GOAL_CAP)),
    }
}

/// A percent and the week it happened.
type Best = Option<(i64, NaiveDate)>;

/// Best 4-week landed rate and best specific week.
fn bests(weeks: &[HistoryWeek]) -> (Best, Best) {
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
fn insights(v: &View, cards: &View, today: NaiveDate) -> Result<Insights> {
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

fn valid_lesson_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= LESSON_ID_MAX
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl Store {
    /// The goal you set, if any. One saved by a newer version (unreadable here) reads as none.
    pub fn goal(&self) -> Result<Option<Goal>> {
        Ok(self.get_setting(GOAL_KEY)?.and_then(|v| serde_json::from_str(&v).ok()))
    }

    /// Sets the goal (replacing any other), starting today. Its id is `now_ms`. A tier you've
    /// already reached isn't a goal.
    pub fn set_goal(&self, kind: GoalKind, target: i64, today: NaiveDate, now_ms: i64) -> Result<Goal> {
        let goal = Goal { id: now_ms, kind, target, start: today };
        goal.validate()?;
        if let GoalKind::Tier { tier } = kind {
            if self.overall(today)?.score.is_some_and(|s| s >= tier.floor()) {
                return Err(Error::Invalid("You're already there. Pick a higher tier."));
            }
        }
        self.set_setting(GOAL_KEY, &serde_json::to_string(&goal)?)?;
        Ok(goal)
    }

    /// Drops the goal. Weeks it already closed stay, for personal bests.
    pub fn clear_goal(&self) -> Result<()> {
        self.conn().execute("DELETE FROM settings WHERE key = ?1", [GOAL_KEY])?;
        Ok(())
    }

    /// Weeks of `goal` already frozen, by Monday.
    fn frozen_weeks(&self, goal: &Goal) -> Result<BTreeMap<NaiveDate, GoalWeek>> {
        let mut stmt = self.conn().prepare("SELECT week, count, value, state FROM goal_weeks WHERE goal = ?1")?;
        let rows =
            stmt.query_map([goal.id], |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?, r.get::<_, String>(3)?)))?;
        let mut out = BTreeMap::new();
        for row in rows {
            let (week, count, value, state) = row?;
            if let (Ok(week), Some(state)) = (week.parse::<NaiveDate>(), WeekState::parse(&state)) {
                out.insert(week, GoalWeek { week, count, value, state });
            }
        }
        Ok(out)
    }

    /// Where `goal` stands on `today`: frozen weeks as they were, the rest from current history.
    fn goal_status(&self, goal: Goal, today: NaiveDate, score: Option<i64>) -> Result<GoalStatus> {
        let mut status = GoalStatus {
            goal,
            weeks: vec![],
            met: 0,
            streak: 0,
            needed: GOAL_WEEKS,
            done: false,
            need: None,
            score: None,
        };
        if let GoalKind::Tier { tier } = goal.kind {
            status.needed = 0;
            status.score = score;
            status.done = self.trophy_for(goal.id)?.is_some() || score.is_some_and(|s| s >= tier.floor());
            return Ok(status);
        }
        let v = View::all(self.conn());
        let live = task_weeks(&v, Some(goal.start))?;
        let frozen = self.frozen_weeks(&goal)?;
        // A finished goal stops at the week it was finished: later weeks aren't part of it.
        let this_week = monday_of(self.trophy_for(goal.id)?.map_or(today, |done| done.min(today)));
        let mut w = monday_of(goal.start);
        while w <= this_week {
            let week = if let Some(f) = frozen.get(&w) {
                *f
            } else {
                let empty = HistoryWeek { week: w, ..HistoryWeek::default() };
                let (count, value) = goal_value(goal.kind, live.get(&w).unwrap_or(&empty));
                GoalWeek { week: w, count, value, state: week_state(&goal, count, value, w < this_week) }
            };
            status.weeks.push(week);
            w += Duration::days(7);
        }
        status.met = status.weeks.iter().filter(|w| w.state == WeekState::Met).count() as i64;
        status.streak = streak(&status.weeks);
        status.done = status.met >= GOAL_WEEKS;
        if let (GoalKind::Habit { signal }, Some(last)) = (goal.kind, status.weeks.last()) {
            if last.state == WeekState::Open {
                let empty = HistoryWeek::default();
                let lw = live.get(&last.week).unwrap_or(&empty);
                status.need =
                    prompts_needed(goal.target, lw.tasks, lw.signals.get(signal.as_str()).copied().unwrap_or(0));
            }
        }
        Ok(status)
    }

    fn trophy_for(&self, goal: i64) -> Result<Option<NaiveDate>> {
        let day: Option<String> =
            self.conn().query_row("SELECT done FROM trophies WHERE id = ?1", [goal], |r| r.get(0)).optional()?;
        Ok(day.and_then(|d| d.parse().ok()))
    }

    /// Trophies, newest first.
    fn trophies(&self) -> Result<Vec<Trophy>> {
        let mut stmt = self.conn().prepare("SELECT goal, done FROM trophies ORDER BY done DESC, rowid DESC")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (goal, done) = row?;
            if let (Ok(goal), Ok(done)) = (serde_json::from_str::<Goal>(&goal), done.parse()) {
                out.push(Trophy { goal, done });
            }
        }
        Ok(out)
    }

    /// Freezes the goal's finished weeks and records a trophy once it's done. Runs after each
    /// sync; returns what's new (weeks just met, and whether the goal was just finished) for
    /// popups. A finished goal is left alone: nothing more to freeze or announce.
    pub fn close_goal_weeks(&self, today: NaiveDate) -> Result<GoalNews> {
        let Some(goal) = self.goal()? else { return Ok(GoalNews::default()) };
        if self.trophy_for(goal.id)?.is_some() {
            return Ok(GoalNews::default());
        }
        let score = self.overall(today)?.score;
        let status = self.goal_status(goal, today, score)?;
        let mut newly_met = Vec::new();
        let tx = self.conn().unchecked_transaction()?;
        for w in status.weeks.iter().filter(|w| w.state != WeekState::Open) {
            let added = tx.execute(
                "INSERT OR IGNORE INTO goal_weeks (goal, week, count, value, state) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![goal.id, w.week.to_string(), w.count, w.value, w.state.as_str()],
            )?;
            if added == 1 && w.state == WeekState::Met {
                newly_met.push(*w);
            }
        }
        if status.done {
            tx.execute(
                "INSERT OR IGNORE INTO trophies (goal, id, done) VALUES (?1, ?2, ?3)",
                params![serde_json::to_string(&goal)?, goal.id, today.to_string()],
            )?;
        }
        tx.commit()?;
        Ok(GoalNews { newly_met, met: status.met, finished: status.done.then_some(goal) })
    }

    /// Everything the Improve tab shows. History and insights follow `f`; the goal and personal
    /// bests are across all AI.
    pub fn progress(&self, f: &Filter, today: NaiveDate) -> Result<Progress> {
        let (clause, args) = f.sql();
        let v = View { conn: self.conn(), clause, args };
        let weeks = history(&v, today)?;
        let all = View::all(self.conn());
        let (clause, args) = Filter { since: None, ..f.clone() }.sql();
        let whole = View { conn: self.conn(), clause, args };
        let all_weeks = if *f == Filter::default() { weeks.clone() } else { history(&all, today)? };
        let overall = self.overall(today)?;
        let goal = self.goal()?.map(|g| self.goal_status(g, today, overall.score)).transpose()?;
        let (landed, sharp) = bests(&all_weeks);
        let mut stmt = self.conn().prepare("SELECT goal, state FROM goal_weeks ORDER BY goal, week")?;
        let rows: Vec<(i64, String)> =
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        let mut goal_streak = 0;
        for chunk in rows.chunk_by(|a, b| a.0 == b.0) {
            goal_streak = goal_streak.max(longest_streak(chunk.iter().filter_map(|r| WeekState::parse(&r.1))));
        }
        if let Some(g) = &goal {
            goal_streak = goal_streak.max(longest_streak(g.weeks.iter().map(|w| w.state)));
        }
        Ok(Progress {
            suggestion: suggest(&all_weeks, today, overall.tier),
            options: options(&all_weeks, today, overall.tier),
            goal,
            trophies: self.trophies()?,
            bests: Bests { landed, sharp, goal_streak, report_card: card_best(&all_weeks) },
            insights: insights(&v, &whole, today)?,
            weeks,
        })
    }

    /// Every report card that still counts under `f` (its models and `since`), added up.
    pub fn report_card_totals(&self, f: &Filter) -> Result<ReportCardWeek> {
        let (clause, args) = f.sql();
        let mut total = ReportCardWeek::default();
        for week in card_weeks(&View { conn: self.conn(), clause, args })?.values() {
            total.add(week);
        }
        Ok(total)
    }

    /// The `n` latest report cards that still count under `f`, newest first.
    pub fn recent_report_cards(&self, f: &Filter, n: usize) -> Result<Vec<RecentReportCard>> {
        let (clause, args) = f.sql();
        let v = View { conn: self.conn(), clause, args };
        let rows = current(&v, &format!("ORDER BY graded_at DESC, start_ts DESC, key DESC LIMIT {n}"))?;
        let habits = habit_bits();
        Ok(rows
            .iter()
            .filter_map(|c| {
                Some(RecentReportCard {
                    day: c.day.clone(),
                    graded_day: local_day(&c.graded_at)?,
                    source: c.source.clone(),
                    grade: c.grade.as_str().to_string(),
                    missing: habits.iter().find(|(_, bit)| c.missing & bit != 0).map(|(name, _)| name.to_string()),
                    extra_cost: c.cost(),
                    extra_tokens: c.shown_tokens(),
                    extra_minutes: c.minutes,
                })
            })
            .collect())
    }

    /// When each source last had a report card written, as the local day, by source id. Sources with
    /// no card aren't listed. Whatever the model filter, and counting every card kept, replaced or not.
    pub fn last_report_card_days(&self) -> Result<Vec<(String, NaiveDate)>> {
        let mut stmt =
            self.conn().prepare("SELECT source, MAX(graded_at) FROM report_cards GROUP BY source ORDER BY source")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (source, last) = row?;
            out.extend(local_day(&last).map(|day| (source, day)));
        }
        Ok(out)
    }

    /// Lessons marked done.
    pub fn learned(&self) -> Result<Vec<String>> {
        Ok(self
            .get_setting(LEARNED_KEY)?
            .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
            .unwrap_or_default())
    }

    /// Marks a lesson done or not done.
    pub fn set_learned(&self, id: &str, done: bool) -> Result<Vec<String>> {
        if !valid_lesson_id(id) {
            return Err(Error::Invalid("Unknown lesson."));
        }
        let mut ids = self.learned()?;
        ids.retain(|x| x != id);
        if done {
            if ids.len() >= LESSONS_MAX {
                return Err(Error::Invalid("Too many lessons."));
            }
            ids.push(id.to_string());
        }
        self.set_setting(LEARNED_KEY, &serde_json::to_string(&ids)?)?;
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_keyed_card_week_has_every_grade_and_habit_at_zero() {
        let w = ReportCardWeek::with_keys();
        let grades: Vec<_> = w.grades.iter().map(|(g, n)| (g.as_str(), *n)).collect();
        assert_eq!(grades, [("A", 0), ("B", 0), ("C", 0), ("D", 0), ("F", 0)]);
        let habits: Vec<_> = w.missing.keys().map(String::as_str).collect();
        assert_eq!(habits, ["context", "evidence", "finish", "guardrails", "purpose", "target"]);
        assert!(w.missing.values().all(|n| *n == 0));
        assert!(ReportCardWeek::default().grades.is_empty(), "the plain default has no keys");
    }

    #[test]
    fn the_share_of_a_and_b_is_rounded_down_and_unknown_when_nothing_is_graded() {
        assert_eq!(ReportCardWeek::with_keys().ab_pct(), None);
        let week = |a, b, c| {
            let mut w = ReportCardWeek::with_keys();
            w.grades.extend([("A", a), ("B", b), ("C", c)].map(|(g, n)| (g.to_string(), n)));
            w.graded = a + b + c;
            w
        };
        assert_eq!(week(1, 1, 1).ab_pct(), Some(66), "two of three, rounded down");
        assert_eq!(week(0, 0, 4).ab_pct(), Some(0));
        assert_eq!(week(3, 2, 0).ab_pct(), Some(100));
        assert_eq!(week(2, 1, 0).count(LetterGrade::B), 1);
        assert_eq!(week(2, 1, 0).count(LetterGrade::F), 0);
        assert_eq!(ReportCardWeek::default().count(LetterGrade::A), 0, "a missing key is zero");
    }

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    /// One task: its day, the signals its prompt carried, messages, and "also…" steering.
    fn task(s: &Store, day: &str, signals: u8, messages: i64, added: i64) {
        s.conn()
            .execute(
                "INSERT INTO tasks (day, source, session, provider, family, version, signals, messages, added)
                 VALUES (?1, 'claude-code', 's', 'Anthropic', 'Claude Opus', '5.5', ?2, ?3, ?4)",
                params![day, signals, messages, added],
            )
            .unwrap();
    }

    fn tasks(s: &Store, day: &str, n: usize, signals: u8, messages: i64, added: i64) {
        for _ in 0..n {
            task(s, day, signals, messages, added);
        }
    }

    const DONE: u8 = Signal::Done.bit();

    fn habit(s: &Store, target: i64, start: &str) -> Goal {
        s.set_goal(GoalKind::Habit { signal: Signal::Done }, target, d(start), 1).unwrap()
    }

    #[test]
    fn history_fills_empty_weeks_and_drops_future_days() {
        let s = Store::open_in_memory().unwrap();
        task(&s, "2026-10-06", DONE, 2, 0);
        task(&s, "2026-10-21", 0, 3, 1);
        task(&s, "2027-01-01", 0, 1, 0); // a clock set wrong once
        let p = s.progress(&Filter::default(), d("2026-10-28")).unwrap();
        let weeks: Vec<String> = p.weeks.iter().map(|w| w.week.to_string()).collect();
        assert_eq!(weeks, ["2026-10-05", "2026-10-12", "2026-10-19", "2026-10-26"]);
        assert_eq!((p.weeks[0].tasks, p.weeks[0].landed, p.weeks[0].signals["done"]), (1, 1, 1));
        assert_eq!(p.weeks[1], HistoryWeek { week: d("2026-10-12"), ..HistoryWeek::default() });
        assert_eq!((p.weeks[2].continued, p.weeks[2].landed, p.weeks[2].steered["added"]), (1, 0, 1));
        let empty = Store::open_in_memory().unwrap().progress(&Filter::default(), d("2026-10-28")).unwrap();
        assert_eq!(empty.weeks, vec![]);
    }

    #[test]
    fn a_habit_goal_counts_from_its_start_and_pauses_quiet_weeks() {
        let s = Store::open_in_memory().unwrap();
        tasks(&s, "2026-10-05", 9, 0, 1, 0); // before the goal: never counts
        let g = habit(&s, 50, "2026-10-06");
        tasks(&s, "2026-10-07", 3, DONE, 1, 0);
        tasks(&s, "2026-10-08", 2, 0, 1, 0); // week of Oct 5: 3 of 5 = 60%, met
        tasks(&s, "2026-10-13", 2, DONE, 1, 0); // week of Oct 12: only 2 prompts, paused
        tasks(&s, "2026-10-20", 1, DONE, 1, 0);
        tasks(&s, "2026-10-21", 5, 0, 1, 0); // week of Oct 19: 1 of 6, missed
        tasks(&s, "2026-10-26", 1, DONE, 1, 0); // this week, open
        let st = s.goal_status(g, d("2026-10-28"), None).unwrap();
        let states: Vec<WeekState> = st.weeks.iter().map(|w| w.state).collect();
        assert_eq!(states, [WeekState::Met, WeekState::Paused, WeekState::Missed, WeekState::Open]);
        assert_eq!((st.weeks[0].count, st.weeks[0].value), (5, 60));
        assert_eq!((st.met, st.streak, st.done), (1, 0, false));
        // This week: 1 of 1 so far, so 4 more prompts with it reach both 50% and the minimum.
        assert_eq!(st.need, Some(4));
    }

    #[test]
    fn paused_weeks_dont_break_a_streak() {
        use WeekState::*;
        let w = |state| GoalWeek { week: d("2026-10-05"), count: 0, value: 0, state };
        assert_eq!(streak(&[w(Missed), w(Met), w(Paused), w(Met), w(Open)]), 2);
        assert_eq!(longest_streak([Met, Met, Missed, Met, Paused, Met, Met]), 3);
    }

    #[test]
    fn a_met_week_is_reported_once_however_often_it_closes() {
        let s = Store::open_in_memory().unwrap();
        habit(&s, 50, "2026-09-28");
        tasks(&s, "2026-09-28", 5, DONE, 1, 0);
        let first = s.close_goal_weeks(d("2026-10-06")).unwrap();
        assert_eq!((first.newly_met.len(), first.met, first.finished), (1, 1, None));
        let again = s.close_goal_weeks(d("2026-10-06")).unwrap();
        assert_eq!((again.newly_met.len(), again.met), (0, 1), "already frozen, so not announced again");
        assert_eq!(s.conn().query_row("SELECT COUNT(*) FROM goal_weeks", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    }

    #[test]
    fn closed_weeks_freeze_and_a_finished_goal_gets_one_trophy() {
        let s = Store::open_in_memory().unwrap();
        let g = habit(&s, 50, "2026-09-28");
        for monday in ["2026-09-28", "2026-10-05", "2026-10-12", "2026-10-19"] {
            tasks(&s, monday, 5, DONE, 1, 0);
        }
        let news = s.close_goal_weeks(d("2026-10-28")).unwrap();
        assert_eq!((news.newly_met.len(), news.met, news.finished), (4, 4, Some(g)));
        // Later history changes (a rebuild, Forget) don't take back frozen weeks.
        s.conn().execute("DELETE FROM tasks", []).unwrap();
        let st = s.goal_status(g, d("2026-10-28"), None).unwrap();
        assert_eq!((st.met, st.done), (4, true));
        assert_eq!(s.close_goal_weeks(d("2026-10-28")).unwrap(), GoalNews::default(), "nothing new, no second trophy");
        let p = s.progress(&Filter::default(), d("2026-10-28")).unwrap();
        assert_eq!(p.trophies, vec![Trophy { goal: g, done: d("2026-10-28") }]);
        assert_eq!(p.bests.goal_streak, 4);
    }

    #[test]
    fn a_steering_goal_is_a_ceiling_rounded_up() {
        let s = Store::open_in_memory().unwrap();
        let g = s.set_goal(GoalKind::Steer { steer: Steer::Added }, 10, d("2026-10-19"), 2).unwrap();
        tasks(&s, "2026-10-19", 9, 0, 2, 0);
        tasks(&s, "2026-10-20", 1, 0, 2, 1); // 1 of 10 = 10%: met
        let st = s.goal_status(g, d("2026-10-28"), None).unwrap();
        assert_eq!((st.weeks[0].value, st.weeks[0].state), (10, WeekState::Met));
        tasks(&s, "2026-10-26", 10, 0, 2, 0);
        tasks(&s, "2026-10-27", 1, 0, 2, 1); // 1 of 11 = 9.1%, rounds up to 10
        let st = s.goal_status(g, d("2026-11-02"), None).unwrap();
        assert_eq!((st.weeks[1].value, st.weeks[1].state), (10, WeekState::Met));
        assert_eq!(ceil_pct(1, 9), 12);
    }

    #[test]
    fn a_week_isnt_judged_or_frozen_until_its_over() {
        let s = Store::open_in_memory().unwrap();
        s.set_goal(GoalKind::Steer { steer: Steer::Added }, 10, d("2026-10-26"), 2).unwrap();
        tasks(&s, "2026-10-26", 5, 0, 2, 0); // Monday: 0% steered, on target so far
        assert_eq!(s.close_goal_weeks(d("2026-10-27")).unwrap(), GoalNews::default());
        assert_eq!(s.conn().query_row("SELECT COUNT(*) FROM goal_weeks", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        tasks(&s, "2026-10-29", 5, 0, 2, 1); // the rest of the week undoes it: 50%
        let news = s.close_goal_weeks(d("2026-11-02")).unwrap();
        assert_eq!(news.newly_met, vec![], "missed, so nothing to announce");
        let g = s.goal().unwrap().unwrap();
        assert_eq!(s.goal_status(g, d("2026-11-02"), None).unwrap().weeks[0].state, WeekState::Missed);
    }

    #[test]
    fn a_tier_goal_is_done_once_the_score_reaches_it() {
        let s = Store::open_in_memory().unwrap();
        let g = s.set_goal(GoalKind::Tier { tier: Tier::Irreplaceable }, 0, d("2026-10-01"), 3).unwrap();
        let st = s.goal_status(g, d("2026-10-28"), Some(89)).unwrap();
        assert_eq!((st.done, st.score, st.needed), (false, Some(89), 0));
        assert!(s.goal_status(g, d("2026-10-28"), Some(90)).unwrap().done);
    }

    #[test]
    fn goals_are_validated() {
        let s = Store::open_in_memory().unwrap();
        let today = d("2026-10-28");
        assert!(
            s.set_goal(GoalKind::Steer { steer: Steer::AiWrong }, 10, today, 1).is_err(),
            "the AI's mistakes aren't yours"
        );
        assert!(s.set_goal(GoalKind::Habit { signal: Signal::Why }, 100, today, 1).is_err());
        assert!(s.set_goal(GoalKind::Tier { tier: Tier::RubberStamp }, 0, today, 1).is_err());
        assert_eq!(s.goal().unwrap(), None, "nothing invalid was saved");
        let g = s.set_goal(GoalKind::Habit { signal: Signal::Why }, 40, today, 7).unwrap();
        assert_eq!(s.goal().unwrap(), Some(g));
        s.set_setting(GOAL_KEY, r#"{"kind":"someday","id":1}"#).unwrap(); // a newer version's goal
        assert_eq!(s.goal().unwrap(), None);
        s.clear_goal().unwrap();
        assert_eq!(s.goal().unwrap(), None);
    }

    #[test]
    fn prompts_needed_reaches_the_target_and_the_minimum() {
        assert_eq!(prompts_needed(50, 0, 0), Some(MIN_WEEK));
        assert_eq!(prompts_needed(50, 10, 5), Some(0));
        assert_eq!(prompts_needed(50, 10, 3), Some(4)); // 7 of 14
        assert_eq!(prompts_needed(80, 6, 0), Some(24)); // 24 of 30
        assert_eq!(prompts_needed(95, 100, 0), None, "1,900 more isn't happening this week");
        assert_eq!(prompts_needed(60, 200, 100), Some(50), "a big week can need more than 50");
        assert_eq!(prompts_needed(60, 200, 0), None);
        for (target, n, k) in [(35, 7, 2), (80, 13, 9), (5, 1, 0)] {
            let x = prompts_needed(target, n, k).unwrap();
            assert!(n + x >= MIN_WEEK && 100 * (k + x) >= target * (n + x), "{target} {n} {k}");
            assert!(
                x == 0 || n + x - 1 < MIN_WEEK || 100 * (k + x - 1) < target * (n + x - 1),
                "smallest: {target} {n} {k}"
            );
        }
    }

    #[test]
    fn suggestions_step_up_from_your_baseline() {
        let today = d("2026-10-28");
        assert_eq!(suggest(&[], today, None).kind, GoalKind::Habit { signal: Signal::Done });
        let week = |signals: &[(&str, i64)], tasks| HistoryWeek {
            tasks,
            continued: tasks,
            signals: signals.iter().map(|(k, v)| ((*k).to_string(), *v)).collect(),
            ..HistoryWeek::default()
        };
        // "why" in 2 of 10 prompts (20%): aim for 35%.
        let g = suggest(&[week(&[("spot", 9), ("proof", 9), ("done", 9), ("limits", 9), ("why", 2)], 10)], today, None);
        assert_eq!((g.kind, g.target), (GoalKind::Habit { signal: Signal::Why }, 35));
        assert!(g.validate().is_ok());
        // Everything already common: work on the steering instead.
        let mut w = week(&[("spot", 10), ("proof", 10), ("done", 10), ("limits", 10), ("why", 9)], 10);
        w.steered.insert("clarified".into(), 3);
        let g = suggest(&[w.clone()], today, None);
        assert_eq!((g.kind, g.target), (GoalKind::Steer { steer: Steer::Clarified }, 25));
        assert!(g.validate().is_ok());
        // Steering on most tasks: the ceiling still has to be one a goal can have.
        w.steered.insert("clarified".into(), 9);
        assert_eq!(suggest(&[w.clone()], today, None).target, STEER_CAP);
        // Nothing left to work on: the next tier.
        w.steered.clear();
        assert_eq!(suggest(&[w], today, Some(Tier::Coworker)).kind, GoalKind::Tier { tier: Tier::SharpEyed });
    }

    #[test]
    fn options_are_all_valid_and_only_tiers_above_yours() {
        let today = d("2026-10-28");
        let w = HistoryWeek {
            tasks: 10,
            continued: 10,
            signals: [("why".to_string(), 10)].into(),
            steered: [("added".to_string(), 10)].into(),
            ..HistoryWeek::default()
        };
        let o = options(&[w], today, Some(Tier::SharpEyed));
        assert_eq!(o.len(), 5 + 3 + 1, "5 habits, 3 kinds of steering, Irreplaceable");
        assert!(o.iter().all(|g| g.validate().is_ok()), "{o:?}");
        assert!(o.iter().all(|g| !matches!(g.kind, GoalKind::Tier { tier } if tier <= Tier::SharpEyed)));
        assert_eq!(options(&[], today, None).iter().filter(|g| matches!(g.kind, GoalKind::Tier { .. })).count(), 3);
    }

    #[test]
    fn a_tier_you_already_have_isnt_a_goal() {
        let s = Store::open_in_memory().unwrap();
        // 10 landed tasks today and 3 corrections: score 100.
        tasks(&s, "2026-10-28", 10, 0, 2, 0);
        s.conn()
            .execute(
                "INSERT INTO daily (day, source, provider, family, version, kind, category, n)
                 VALUES ('2026-10-28', 'claude-code', 'Anthropic', 'Claude Opus', '5.5', 'confirmed', 'made_it_up', 3)",
                [],
            )
            .unwrap();
        assert_eq!(s.overall(d("2026-10-28")).unwrap().score, Some(100));
        assert!(s.set_goal(GoalKind::Tier { tier: Tier::Coworker }, 0, d("2026-10-28"), 1).is_err());
    }

    #[test]
    fn a_finished_goal_announces_nothing_more() {
        let s = Store::open_in_memory().unwrap();
        habit(&s, 50, "2026-09-28");
        for monday in ["2026-09-28", "2026-10-05", "2026-10-12", "2026-10-19"] {
            tasks(&s, monday, 5, DONE, 1, 0);
        }
        assert!(s.close_goal_weeks(d("2026-10-28")).unwrap().finished.is_some());
        tasks(&s, "2026-10-26", 5, DONE, 1, 0);
        assert_eq!(s.close_goal_weeks(d("2026-11-02")).unwrap(), GoalNews::default(), "no more week popups");
    }

    #[test]
    fn bests_need_enough_tasks() {
        let w = |day: &str, continued, landed, tasks, sharp| HistoryWeek {
            week: d(day),
            tasks,
            continued,
            landed,
            grades: [("sharp".to_string(), sharp)].into(),
            ..HistoryWeek::default()
        };
        let weeks = [
            w("2026-09-28", 3, 3, 3, 3), // too few for a specific best
            w("2026-10-05", 3, 2, 5, 2),
            w("2026-10-12", 3, 3, 5, 1),
            w("2026-10-19", 3, 3, 5, 0),
        ];
        let (landed, sharp) = bests(&weeks);
        assert_eq!(landed, Some((91, d("2026-10-19")))); // 11 of 12
        assert_eq!(sharp, Some((40, d("2026-10-05"))));
        assert_eq!(bests(&weeks[1..]).0, None, "9 continued tasks isn't enough");
    }

    #[test]
    fn lessons_done_are_kept_and_checked() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.set_learned("say-why", true).unwrap(), ["say-why"]);
        assert_eq!(s.set_learned("spot", true).unwrap(), ["say-why", "spot"]);
        assert_eq!(s.set_learned("say-why", true).unwrap(), ["spot", "say-why"], "no duplicates");
        assert_eq!(s.set_learned("spot", false).unwrap(), ["say-why"]);
        for bad in ["", "Spot", "../x", "a b", &"x".repeat(LESSON_ID_MAX + 1)] {
            assert!(s.set_learned(bad, true).is_err(), "{bad}");
        }
        assert_eq!(s.learned().unwrap(), ["say-why"]);
    }

    #[test]
    fn the_tip_trend_compares_the_last_two_full_weeks_when_both_have_enough_tasks() {
        let s = Store::open_in_memory().unwrap();
        s.conn()
            .execute("INSERT INTO tip_chats (session, source, day) VALUES ('s','claude-code','2026-10-01')", [])
            .unwrap();
        let tip = |n: usize, day: &str| {
            for i in 0..n {
                s.conn()
                    .execute(
                        "INSERT INTO tips (key, source, session, day, ts, provider, family, version, habit, redid)
                         VALUES (?1,'claude-code','s',?2,?2,'Anthropic','Claude Opus','5.5',1,50)",
                        params![format!("{day}-{i}"), day],
                    )
                    .unwrap();
            }
        };
        let trend = |today: &str| s.progress(&Filter::default(), d(today)).unwrap().insights.tip_trend;
        tasks(&s, "2026-10-06", 12, 0, 1, 0);
        tasks(&s, "2026-10-13", 9, 0, 1, 0);
        tip(3, "2026-10-06");
        assert_eq!(trend("2026-10-21"), None, "the last full week has too few tasks");
        tasks(&s, "2026-10-13", 1, 0, 1, 0);
        tip(14, "2026-10-13");
        let shown = trend("2026-10-21").unwrap();
        assert_eq!(shown.before, TipWeek { tips: 3, tasks: 12 });
        assert_eq!(shown.latest, TipWeek { tips: 10, tasks: 10 }, "never more tips than tasks");
        assert_eq!(trend("2026-10-14"), None, "this week is still under way");
    }

    #[test]
    fn insights_only_speak_with_enough_history() {
        let s = Store::open_in_memory().unwrap();
        tasks(&s, "2026-10-20", 12, DONE, 2, 0); // all landed
        tasks(&s, "2026-10-20", 12, 0, 2, 1); // none landed
        let i = s.progress(&Filter::default(), d("2026-10-28")).unwrap().insights;
        assert_eq!(i.signals.len(), 1);
        assert_eq!((i.signals[0].signal.as_str(), i.signals[0].landed_with_without), ("done", Some((100.0, 0.0))));
        assert_eq!(i.steer, Some((Steer::Added, 100)));
        assert_eq!(i.models, vec![], "one model: nothing to compare");
        let few = Store::open_in_memory().unwrap();
        tasks(&few, "2026-10-20", 3, 0, 2, 1);
        assert_eq!(few.progress(&Filter::default(), d("2026-10-28")).unwrap().insights, Insights::default());
    }
}
