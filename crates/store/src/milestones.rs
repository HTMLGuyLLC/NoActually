//! Moments worth a popup: catch counts, tier ups (catches and briefs), streaks. Pure decisions plus the stored
//! baseline they're measured from; the app only shows what [`crossed`] returns.

use crate::{Result, Stats, Store};
use engine::{BriefTier, Tier};

/// Catch totals worth announcing.
pub const COUNTS: [i64; 9] = [1, 10, 25, 50, 100, 250, 500, 1000, 2500];
/// Streak lengths (days) worth announcing.
pub const STREAKS: [i64; 5] = [3, 7, 14, 30, 100];

const TOTAL_KEY: &str = "milestone.total";
const TIER_KEY: &str = "milestone.tier";
const STREAK_KEY: &str = "milestone.streak";
const BRIEF_TIER_KEY: &str = "milestone.brief_tier";

/// The numbers milestones are measured on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snapshot {
    /// Catches ever.
    pub catches: i64,
    /// Catches in the last 7 days.
    pub catches_last_7d: i64,
    /// This week's tier.
    pub tier: Tier,
    /// Current streak, in days.
    pub streak: i64,
    /// This week's brief tier, once ranked.
    pub brief_tier: Option<BriefTier>,
    /// Percent of this week's tasks that landed, rounded.
    pub landed_pct: i64,
}

impl Snapshot {
    /// The snapshot in a scorecard.
    pub fn of(s: &Stats) -> Self {
        let b = &s.briefs;
        Snapshot {
            catches: s.catches,
            catches_last_7d: s.catches_last_7d,
            tier: s.tier,
            streak: s.current_streak_days,
            brief_tier: b.tier,
            // Rounded down, like the tier floors, so the popup never says 95% for a Tech lead week.
            landed_pct: if b.tasks_last_7d > 0 { 100 * b.landed_last_7d / b.tasks_last_7d } else { 0 },
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
            title: format!("Your {} catch!", ordinal(*m)),
            body: format!("You've caught your AI {m} times. Could it replace you? Not today."),
        });
    }
    if now.tier > prev.tier {
        out.push(Milestone {
            title: format!("New tier: {}", now.tier.name()),
            body: format!("{} catches in the last 7 days. Open the scorecard to share it.", now.catches_last_7d),
        });
    }
    // Getting ranked at the bottom isn't news; climbing is.
    if let Some(t) = now.brief_tier.filter(|t| *t > BriefTier::VibesOnly && now.brief_tier > prev.brief_tier) {
        out.push(Milestone {
            title: format!("Briefs: {}", t.name()),
            body: format!(
                "{}% of this week's tasks landed without steering. Open the scorecard to share it.",
                now.landed_pct
            ),
        });
    }
    if let Some(s) = STREAKS.iter().rev().find(|s| prev.streak < **s && now.streak >= **s) {
        out.push(Milestone {
            title: format!("{s}-day streak"),
            body: format!("You've caught a mistake {s} days in a row."),
        });
    }
    out
}

impl Store {
    /// Records `now` as the milestone baseline and returns the previous one, or `None` the first
    /// time (nothing to compare against, so nothing to announce).
    pub fn advance_milestones(&self, now: &Snapshot) -> Result<Option<Snapshot>> {
        let total = self.get_setting(TOTAL_KEY)?;
        let first_run = total.is_none();
        let prev = Snapshot {
            catches: total.and_then(|v| v.parse().ok()).unwrap_or(now.catches),
            catches_last_7d: now.catches_last_7d,
            // Older versions saved the tier's display name; `Tier::from_str` reads both.
            tier: self.get_setting(TIER_KEY)?.and_then(|v| v.parse().ok()).unwrap_or(now.tier),
            streak: self.get_setting(STREAK_KEY)?.and_then(|v| v.parse().ok()).unwrap_or(now.streak),
            // Never saved (first run, or a version without briefs): today's is the baseline.
            brief_tier: match self.get_setting(BRIEF_TIER_KEY)? {
                Some(v) => v.parse().ok(),
                None => now.brief_tier,
            },
            landed_pct: now.landed_pct,
        };
        self.set_setting(TOTAL_KEY, &now.catches.to_string())?;
        self.set_setting(TIER_KEY, now.tier.id())?;
        self.set_setting(STREAK_KEY, &now.streak.to_string())?;
        // A quiet week (too few tasks to rank) keeps the last tier, so being ranked again at the
        // same tier isn't announced as new.
        if let Some(t) = now.brief_tier {
            self.set_setting(BRIEF_TIER_KEY, t.id())?;
        }
        Ok((!first_run).then_some(prev))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(catches: i64, tier: Tier, streak: i64) -> Snapshot {
        Snapshot { catches, catches_last_7d: 8, tier, streak, brief_tier: None, landed_pct: 0 }
    }

    fn briefs(tier: Option<BriefTier>) -> Snapshot {
        Snapshot { brief_tier: tier, landed_pct: 92, ..snap(30, Tier::Coworker, 0) }
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
        assert_eq!(m[0].title, "Your 25th catch!");
        assert_eq!(crossed(&snap(27, Tier::Coworker, 0), &snap(27, Tier::Coworker, 0)), vec![]);
        assert_eq!(crossed(&snap(0, Tier::RubberStamp, 0), &snap(1, Tier::RubberStamp, 0))[0].title, "Your 1st catch!");
    }

    #[test]
    fn tier_ups_and_streaks() {
        let m = crossed(&snap(30, Tier::Coworker, 6), &snap(30, Tier::SharpEyed, 7));
        let titles: Vec<&str> = m.iter().map(|m| m.title.as_str()).collect();
        assert_eq!(titles, ["New tier: Sharp-eyed", "7-day streak"]);
        assert_eq!(m[0].body, "8 catches in the last 7 days. Open the scorecard to share it.");
        assert_eq!(crossed(&snap(30, Tier::SharpEyed, 7), &snap(30, Tier::Coworker, 7)), vec![], "never for a drop");
    }

    #[test]
    fn brief_tier_ups_but_not_a_first_ranking_at_the_bottom() {
        use BriefTier::*;
        let m = crossed(&briefs(Some(TicketWriter)), &briefs(Some(TechLead)));
        assert_eq!(
            m,
            vec![Milestone {
                title: "Briefs: Tech lead".into(),
                body: "92% of this week's tasks landed without steering. Open the scorecard to share it.".into()
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
    fn baseline_starts_quiet_and_reads_old_tier_names() {
        let s = Store::open_in_memory().unwrap();
        let now = snap(5, Tier::Coworker, 2);
        assert_eq!(s.advance_milestones(&now).unwrap(), None, "first run only sets the baseline");
        assert_eq!(s.advance_milestones(&snap(11, Tier::SharpEyed, 3)).unwrap(), Some(now));
        s.set_setting(TIER_KEY, "Sharp-eyed").unwrap(); // written by an older version
        assert_eq!(s.advance_milestones(&now).unwrap().unwrap().tier, Tier::SharpEyed);
    }
}
