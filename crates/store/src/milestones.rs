//! Moments worth a popup: catch counts, tier ups (catches and briefs), streaks. Pure decisions plus the stored
//! baseline they're measured from; the app only shows what [`crossed`] returns.

use crate::{GoalWeek, Result, Stats, Store, GOAL_WEEKS};
use engine::{BriefTier, Tier};

/// Catch totals worth announcing.
pub const COUNTS: [i64; 9] = [1, 10, 25, 50, 100, 250, 500, 1000, 2500];
/// Streak lengths (days) worth announcing.
pub const STREAKS: [i64; 5] = [3, 7, 14, 30, 100];

const TOTAL_KEY: &str = "milestone.total";
/// The overall tier. (Before the score existed, `milestone.tier` held a catch-count tier.)
const TIER_KEY: &str = "milestone.score_tier";
const STREAK_KEY: &str = "milestone.streak";
const BRIEF_TIER_KEY: &str = "milestone.brief_tier";

/// The numbers milestones are measured on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snapshot {
    /// Catches ever.
    pub catches: i64,
    /// Catches in the last 7 days.
    pub catches_last_7d: i64,
    /// The overall tier, once ranked.
    pub tier: Option<Tier>,
    /// The overall score (0 until ranked).
    pub score: i64,
    /// Current streak, in days.
    pub streak: i64,
    /// This week's brief tier, once ranked.
    pub brief_tier: Option<BriefTier>,
    /// Percent of the window's continued tasks that landed, rounded down like the tier floors.
    pub landed_pct: i64,
}

impl Snapshot {
    /// The snapshot in a scorecard.
    pub fn of(s: &Stats) -> Self {
        let b = &s.briefs;
        Snapshot {
            catches: s.catches,
            catches_last_7d: s.catches_last_7d,
            tier: s.overall.tier,
            score: s.overall.score.unwrap_or(0),
            streak: s.current_streak_days,
            brief_tier: b.tier,
            // Rounded down, like the tier floors, so the popup never says 95% for a Tech lead week.
            landed_pct: if b.window_tasks > 0 { 100 * b.window_landed / b.window_tasks } else { 0 },
        }
    }
}

/// One popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Milestone {
    /// Notification title.
    pub title: String,
    /// Notification body.
    pub body: String,
}

/// "1st", "2nd", "11th", "112th".
pub fn ordinal(n: i64) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, x) if x != 11 => "st",
        (2, x) if x != 12 => "nd",
        (3, x) if x != 13 => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// What moving from `prev` to `now` earned: at most one of each kind, the biggest crossed.
pub fn crossed(prev: &Snapshot, now: &Snapshot) -> Vec<Milestone> {
    let mut out = Vec::new();
    if let Some(m) = COUNTS.iter().rev().find(|m| prev.catches < **m && now.catches >= **m) {
        out.push(Milestone {
            title: format!("Your {} correction!", ordinal(*m)),
            body: format!("You've corrected your AI {m} times. Could it replace you? Not today."),
        });
    }
    // Like brief tiers: a first ranking at the bottom isn't news, climbing is.
    if let Some(t) = now.tier.filter(|t| *t > Tier::RubberStamp && now.tier > prev.tier) {
        out.push(Milestone {
            title: format!("New tier: {}", t.name()),
            body: format!("Could AI replace you? Your score is {}. Open the scorecard to share it.", now.score),
        });
    }
    // Getting ranked at the bottom isn't news; climbing is.
    if let Some(t) = now.brief_tier.filter(|t| *t > BriefTier::VibesOnly && now.brief_tier > prev.brief_tier) {
        out.push(Milestone {
            title: format!("Prompts: {}", t.name()),
            body: format!(
                "{}% of your tasks in the last 28 days landed without steering. Open the scorecard to share it.",
                now.landed_pct
            ),
        });
    }
    if let Some(s) = STREAKS.iter().rev().find(|s| prev.streak < **s && now.streak >= **s) {
        out.push(Milestone {
            title: format!("{s}-day streak"),
            body: format!("You've corrected a mistake {s} days in a row."),
        });
    }
    out
}

/// The goal's popup after a sync froze its finished weeks: a finished goal, or else the latest
/// week met. At most one, so catching up on several weeks never piles them up.
pub fn goal_milestones(newly_met: &[GoalWeek], met: i64, finished: bool) -> Vec<Milestone> {
    if finished {
        return vec![Milestone {
            title: "Goal done! 🏆".into(),
            body: "It's on your trophy shelf. Open Improve to pick the next one.".into(),
        }];
    }
    newly_met
        .last()
        .map(|w| Milestone {
            title: "Goal week met 🎯".into(),
            body: format!("{}% last week. That's {met} of {GOAL_WEEKS} weeks met.", w.value),
        })
        .into_iter()
        .collect()
}

/// Saved instead of a tier id while unranked, so the first ranking is measured from "none".
const UNRANKED: &str = "none";

/// The best tier reached so far (`saved`) and what to save now. Tiers are announced only above
/// the best: a 28-day score wobbling across a floor would otherwise announce it again and again.
/// Never saved (first run, or an older version): today's tier is the baseline.
fn best_tier<T: Copy + Ord + std::str::FromStr>(
    saved: Option<String>,
    now: Option<T>,
    id: fn(T) -> &'static str,
) -> (Option<T>, String) {
    let prev = match saved.as_deref() {
        None => now,
        Some(UNRANKED) => None,
        Some(v) => v.parse().ok(),
    };
    let best = prev.max(now);
    (prev, best.map_or(UNRANKED, id).to_string())
}

impl Store {
    /// Records `now` as the milestone baseline and returns the previous one, or `None` the first
    /// time (nothing to compare against, so nothing to announce).
    pub fn advance_milestones(&self, now: &Snapshot) -> Result<Option<Snapshot>> {
        let total = self.get_setting(TOTAL_KEY)?;
        let first_run = total.is_none();
        let (tier, save_tier) = best_tier(self.get_setting(TIER_KEY)?, now.tier, Tier::id);
        let (brief_tier, save_brief_tier) = best_tier(self.get_setting(BRIEF_TIER_KEY)?, now.brief_tier, BriefTier::id);
        let prev = Snapshot {
            catches: total.and_then(|v| v.parse().ok()).unwrap_or(now.catches),
            catches_last_7d: now.catches_last_7d,
            tier,
            score: now.score,
            streak: self.get_setting(STREAK_KEY)?.and_then(|v| v.parse().ok()).unwrap_or(now.streak),
            brief_tier,
            landed_pct: now.landed_pct,
        };
        self.set_setting(TOTAL_KEY, &now.catches.to_string())?;
        self.set_setting(TIER_KEY, &save_tier)?;
        self.set_setting(STREAK_KEY, &now.streak.to_string())?;
        self.set_setting(BRIEF_TIER_KEY, &save_brief_tier)?;
        Ok((!first_run).then_some(prev))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(catches: i64, tier: Tier, streak: i64) -> Snapshot {
        Snapshot { catches, catches_last_7d: 8, tier: Some(tier), score: 84, streak, brief_tier: None, landed_pct: 0 }
    }

    fn briefs(tier: Option<BriefTier>) -> Snapshot {
        Snapshot { brief_tier: tier, landed_pct: 92, ..snap(30, Tier::Coworker, 0) }
    }

    #[test]
    fn a_tier_is_announced_once_however_the_score_wobbles() {
        let s = Store::open_in_memory().unwrap();
        let at = |tier| Snapshot { tier, ..snap(30, Tier::Coworker, 0) };
        let mut popups = 0;
        s.advance_milestones(&at(None)).unwrap(); // first run: unranked
        for tier in [
            None,
            Some(Tier::Coworker),
            Some(Tier::SharpEyed),
            Some(Tier::Coworker),
            Some(Tier::SharpEyed),
            None,
            Some(Tier::SharpEyed),
        ] {
            let now = at(tier);
            let prev = s.advance_milestones(&now).unwrap().unwrap();
            popups += crossed(&prev, &now).iter().filter(|m| m.title.starts_with("New tier")).count();
        }
        assert_eq!(popups, 2, "Coworker once (the first ranking counts), Sharp-eyed once");
    }

    #[test]
    fn goal_popups_are_one_at_most() {
        let w = |value| GoalWeek { week: "2026-10-19".parse().unwrap(), count: 8, value, state: crate::WeekState::Met };
        assert_eq!(goal_milestones(&[], 0, false), vec![]);
        let m = goal_milestones(&[w(50), w(62)], 3, false);
        assert_eq!((m.len(), m[0].body.as_str()), (1, "62% last week. That's 3 of 4 weeks met."));
        assert_eq!(goal_milestones(&[w(50)], 4, true)[0].title, "Goal done! 🏆");
    }

    #[test]
    fn ordinals() {
        let got: Vec<String> = [1, 2, 3, 4, 11, 12, 13, 21, 22, 23, 101, 111, 112, 113].map(ordinal).into();
        assert_eq!(
            got,
            [
                "1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "23rd", "101st", "111th", "112th",
                "113th"
            ]
        );
    }

    #[test]
    fn announces_the_biggest_count_crossed_once() {
        let m = crossed(&snap(8, Tier::Coworker, 0), &snap(27, Tier::Coworker, 0));
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].title, "Your 25th correction!");
        assert_eq!(crossed(&snap(27, Tier::Coworker, 0), &snap(27, Tier::Coworker, 0)), vec![]);
        assert_eq!(crossed(&snap(0, Tier::RubberStamp, 0), &snap(1, Tier::RubberStamp, 0))[0].title, "Your 1st correction!");
    }

    #[test]
    fn tier_ups_and_streaks() {
        let m = crossed(&snap(30, Tier::Coworker, 6), &snap(30, Tier::SharpEyed, 7));
        let titles: Vec<&str> = m.iter().map(|m| m.title.as_str()).collect();
        assert_eq!(titles, ["New tier: Sharp-eyed", "7-day streak"]);
        assert_eq!(m[0].body, "Could AI replace you? Your score is 84. Open the scorecard to share it.");
        assert_eq!(crossed(&snap(30, Tier::SharpEyed, 7), &snap(30, Tier::Coworker, 7)), vec![], "never for a drop");
        let unranked = Snapshot { tier: None, ..snap(30, Tier::Coworker, 7) };
        assert_eq!(crossed(&unranked, &snap(30, Tier::RubberStamp, 7)), vec![], "a first ranking at the bottom");
        assert_eq!(crossed(&unranked, &snap(30, Tier::Coworker, 7)).len(), 1);
    }

    #[test]
    fn brief_tier_ups_but_not_a_first_ranking_at_the_bottom() {
        use BriefTier::*;
        let m = crossed(&briefs(Some(TicketWriter)), &briefs(Some(TechLead)));
        assert_eq!(
            m,
            vec![Milestone {
                title: "Prompts: Tech lead".into(),
                body: "92% of your tasks in the last 28 days landed without steering. Open the scorecard to share it."
                    .into()
            }]
        );
        assert_eq!(crossed(&briefs(None), &briefs(Some(Architect))).len(), 1, "a first ranking above the bottom");
        assert_eq!(crossed(&briefs(None), &briefs(Some(VibesOnly))), vec![]);
        assert_eq!(crossed(&briefs(Some(TechLead)), &briefs(Some(TicketWriter))), vec![], "never for a drop");
        assert_eq!(crossed(&briefs(Some(TechLead)), &briefs(None)), vec![]);
    }

    #[test]
    fn brief_tier_baseline_is_saved_and_starts_quiet() {
        let s = Store::open_in_memory().unwrap();
        s.advance_milestones(&briefs(None)).unwrap();
        // An upgrade from a version without briefs has no saved brief tier: no popup for it.
        s.conn().execute("DELETE FROM settings WHERE key = ?1", [BRIEF_TIER_KEY]).unwrap();
        let now = briefs(Some(BriefTier::Architect));
        assert_eq!(s.advance_milestones(&now).unwrap().unwrap().brief_tier, Some(BriefTier::Architect));
        assert_eq!(s.advance_milestones(&briefs(None)).unwrap().unwrap().brief_tier, Some(BriefTier::Architect));
        let back = s.advance_milestones(&now).unwrap().unwrap();
        assert_eq!(back.brief_tier, Some(BriefTier::Architect), "a quiet week doesn't reset the tier");
        assert_eq!(crossed(&back, &now), vec![], "so ranking again at the same tier isn't news");
    }

    #[test]
    fn baseline_starts_quiet_and_ignores_the_old_catch_tier() {
        let s = Store::open_in_memory().unwrap();
        // Written by a version whose tiers counted catches: never compared against the score's.
        s.set_setting("milestone.tier", "rubber_stamp").unwrap();
        let now = snap(5, Tier::Coworker, 2);
        assert_eq!(s.advance_milestones(&now).unwrap(), None, "first run only sets the baseline");
        assert_eq!(s.advance_milestones(&snap(11, Tier::SharpEyed, 3)).unwrap().unwrap().tier, Some(Tier::Coworker));
        let quiet = Snapshot { tier: None, ..now };
        s.advance_milestones(&quiet).unwrap();
        assert_eq!(
            s.advance_milestones(&now).unwrap().unwrap().tier,
            Some(Tier::SharpEyed),
            "a quiet stretch keeps the tier"
        );
    }
}
