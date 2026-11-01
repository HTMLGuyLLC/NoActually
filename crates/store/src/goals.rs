//! The one goal you're working on: what it asks for, how each week of it went, the weeks it
//! already closed (frozen, so a later rebuild or import can't take back a week you earned), and
//! the goals to suggest.

use crate::progress::{floor_pct, task_weeks, HistoryWeek, MIN_WEEK};
use crate::stats::{monday_of, View};
use crate::{Error, Result, Store};
use chrono::{Duration, NaiveDate};
use engine::{Signal, Steer, Tier};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Weeks a habit or steering goal has to be met to be done.
pub const GOAL_WEEKS: i64 = 4;
/// Weeks of history a suggested goal's baseline looks at.
const BASELINE_WEEKS: i64 = 8;
/// A suggested habit goal asks for this many points more than your baseline...
const GOAL_STEP: i64 = 15;
/// ...but never more than this.
const GOAL_CAP: i64 = 80;
/// The highest ceiling a steering goal can have (more is no goal at all).
pub(crate) const STEER_CAP: i64 = 50;
/// "N more prompts" past this many (or past this week's count, if bigger) isn't reachable in a
/// week; it isn't shown.
const NEED_MAX: i64 = 50;

pub(crate) const GOAL_KEY: &str = "goal";

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

    pub(crate) fn parse(s: &str) -> Option<WeekState> {
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

/// Percent rounded up (a steering target is a ceiling: 10.1% isn't 10%).
pub(crate) fn ceil_pct(n: i64, total: i64) -> i64 {
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
pub(crate) fn prompts_needed(target: i64, n: i64, k: i64) -> Option<i64> {
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
pub(crate) fn streak(weeks: &[GoalWeek]) -> i64 {
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

pub(crate) fn longest_streak(states: impl IntoIterator<Item = WeekState>) -> i64 {
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
pub(crate) fn options(weeks: &[HistoryWeek], today: NaiveDate, tier: Option<Tier>) -> Vec<Goal> {
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
pub(crate) fn suggest(weeks: &[HistoryWeek], today: NaiveDate, tier: Option<Tier>) -> Goal {
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
    pub(crate) fn goal_status(&self, goal: Goal, today: NaiveDate, score: Option<i64>) -> Result<GoalStatus> {
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
    pub(crate) fn trophies(&self) -> Result<Vec<Trophy>> {
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
}
