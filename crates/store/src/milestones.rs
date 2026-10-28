//! Moments worth a popup: catch counts, tier ups, streaks. Pure decisions plus the stored
//! baseline they're measured from; the app only shows what [`crossed`] returns.

use crate::{Result, Stats, Store};
use engine::Tier;

/// Catch totals worth announcing.
pub const COUNTS: [i64; 9] = [1, 10, 25, 50, 100, 250, 500, 1000, 2500];
/// Streak lengths (days) worth announcing.
pub const STREAKS: [i64; 5] = [3, 7, 14, 30, 100];

const TOTAL_KEY: &str = "milestone.total";
const TIER_KEY: &str = "milestone.tier";
const STREAK_KEY: &str = "milestone.streak";

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
}

impl Snapshot {
    /// The snapshot in a scorecard.
    pub fn of(s: &Stats) -> Self {
        Snapshot { catches: s.catches, catches_last_7d: s.catches_last_7d, tier: s.tier, streak: s.current_streak_days }
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
        };
        self.set_setting(TOTAL_KEY, &now.catches.to_string())?;
        self.set_setting(TIER_KEY, now.tier.id())?;
        self.set_setting(STREAK_KEY, &now.streak.to_string())?;
        Ok((!first_run).then_some(prev))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(catches: i64, tier: Tier, streak: i64) -> Snapshot {
        Snapshot { catches, catches_last_7d: 8, tier, streak }
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
    fn baseline_starts_quiet_and_reads_old_tier_names() {
        let s = Store::open_in_memory().unwrap();
        let now = snap(5, Tier::Coworker, 2);
        assert_eq!(s.advance_milestones(&now).unwrap(), None, "first run only sets the baseline");
        assert_eq!(s.advance_milestones(&snap(11, Tier::SharpEyed, 3)).unwrap(), Some(now));
        s.set_setting(TIER_KEY, "Sharp-eyed").unwrap(); // written by an older version
        assert_eq!(s.advance_milestones(&now).unwrap().unwrap().tier, Tier::SharpEyed);
    }
}
