//! "Could AI replace you?": one score from both sides of working with AI, and the tiers and
//! headline answer it gives. The one table the scorecard, quick glance, CLI and milestone popups
//! all read.
//!
//! The score mixes direction (how often your tasks land without you steering them) with judgment
//! (how rarely the AI has to correct you, once you've shown you check its work). Catches only
//! have to show that you check: past [`CATCH_PROOF`] in the window, more add nothing, and
//! judgment never compares catches with corrections, so briefing better and catching fewer
//! mistakes never costs you points.

use crate::brief::MIN_BRIEFS;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Percent of the score that comes from direction (landed tasks); the rest is judgment.
pub const DIRECTION_WEIGHT: i64 = 70;
/// Catches in the window that fully show you check its work.
pub const CATCH_PROOF: i64 = 3;
/// Judgment points lost per 1% of continued tasks the AI had to correct you on (so being
/// corrected on a fifth of your tasks leaves none).
pub const HUMBLED_COST: i64 = 5;
/// Below this many continued tasks, corrections are counted against this many instead, so two
/// corrections in a thin month don't wipe out judgment.
pub const HUMBLED_BASE: i64 = 30;

/// How replaceable you look. Ordered from lowest to highest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// A score under 65.
    RubberStamp,
    /// 65 to 79.
    Coworker,
    /// 80 to 89.
    SharpEyed,
    /// 90 or more.
    Irreplaceable,
}

impl Tier {
    /// Every tier, ascending by floor.
    pub const ALL: [Tier; 4] = [Tier::RubberStamp, Tier::Coworker, Tier::SharpEyed, Tier::Irreplaceable];

    /// The score needed to reach this tier.
    pub const fn floor(self) -> i64 {
        match self {
            Tier::RubberStamp => 0,
            Tier::Coworker => 65,
            Tier::SharpEyed => 80,
            Tier::Irreplaceable => 90,
        }
    }

    /// Stable id, as serialized (`sharp_eyed`).
    pub const fn id(self) -> &'static str {
        match self {
            Tier::RubberStamp => "rubber_stamp",
            Tier::Coworker => "coworker",
            Tier::SharpEyed => "sharp_eyed",
            Tier::Irreplaceable => "irreplaceable",
        }
    }

    /// Display name ("Sharp-eyed").
    pub const fn name(self) -> &'static str {
        match self {
            Tier::RubberStamp => "Rubber stamp",
            Tier::Coworker => "Coworker",
            Tier::SharpEyed => "Sharp-eyed",
            Tier::Irreplaceable => "Irreplaceable",
        }
    }

    /// The highest tier whose floor `score` reaches.
    pub fn for_score(score: i64) -> Tier {
        Tier::ALL.into_iter().rev().find(|t| score >= t.floor()).unwrap_or(Tier::RubberStamp)
    }

    /// The tier above this one, if any.
    pub fn next(self) -> Option<Tier> {
        Tier::ALL.into_iter().find(|t| *t > self)
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A string that names no tier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownTier(pub String);

impl fmt::Display for UnknownTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown tier '{}'", self.0)
    }
}

impl std::error::Error for UnknownTier {}

impl FromStr for Tier {
    type Err = UnknownTier;

    /// Accepts the id or the display name (older stores saved the name).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Tier::ALL.into_iter().find(|t| t.id() == s || t.name() == s).ok_or_else(|| UnknownTier(s.to_string()))
    }
}

/// "Could AI replace you?" The headline every view shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    /// Coworker or better.
    No,
    /// Not ranked yet, or a Rubber stamp.
    Hmm,
}

impl Answer {
    /// "No" from Coworker up.
    pub fn for_tier(tier: Option<Tier>) -> Answer {
        if tier.is_some_and(|t| t >= Tier::Coworker) {
            Answer::No
        } else {
            Answer::Hmm
        }
    }
}

/// The last [`crate::WINDOW_DAYS`] days, counted the way the score reads them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Window {
    /// Tasks that went past their first message.
    pub continued: i64,
    /// Of those, the ones you never had to add to, clarify or redo.
    pub landed: i64,
    /// Times you caught the AI.
    pub catches: i64,
    /// Times the AI corrected you.
    pub humbled: i64,
}

/// The overall score and the two parts it's made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Score {
    /// 0 to 100, rounded down like the floors.
    pub score: i64,
    /// Percent of continued tasks that landed, rounded down.
    pub direction: i64,
    /// 0 to 100: how rarely the AI corrected you (per task), scaled down until you've shown
    /// [`CATCH_PROOF`] catches.
    pub judgment: i64,
    /// The tier `score` reaches.
    pub tier: Tier,
}

impl Score {
    /// The score for a window, or `None` until it has [`MIN_BRIEFS`] continued tasks: with fewer,
    /// one task swings the number too much to rank.
    pub fn of(w: Window) -> Option<Score> {
        if w.continued < MIN_BRIEFS {
            return None;
        }
        // Thousandths so the weights don't round twice.
        let direction = 1000 * w.landed.clamp(0, w.continued) / w.continued;
        // Per task, not per catch: comparing with catches would make fewer catches cost points.
        let right = (1000 - HUMBLED_COST * 1000 * w.humbled.max(0) / w.continued.max(HUMBLED_BASE)).max(0);
        let judgment = right * w.catches.clamp(0, CATCH_PROOF) / CATCH_PROOF;
        let score = (DIRECTION_WEIGHT * direction + (100 - DIRECTION_WEIGHT) * judgment) / 100;
        Some(Score {
            score: score / 10,
            direction: direction / 10,
            judgment: judgment / 10,
            tier: Tier::for_score(score / 10),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(continued: i64, landed: i64, catches: i64, humbled: i64) -> Window {
        Window { continued, landed, catches, humbled }
    }

    #[test]
    fn tiers_follow_floors() {
        let at = |n| Tier::for_score(n);
        assert_eq!((at(-1), at(0), at(64)), (Tier::RubberStamp, Tier::RubberStamp, Tier::RubberStamp));
        assert_eq!((at(65), at(79)), (Tier::Coworker, Tier::Coworker));
        assert_eq!((at(80), at(89)), (Tier::SharpEyed, Tier::SharpEyed));
        assert_eq!((at(90), at(100)), (Tier::Irreplaceable, Tier::Irreplaceable));
        assert!(Tier::ALL.windows(2).all(|w| w[0] < w[1] && w[0].floor() < w[1].floor()));
    }

    #[test]
    fn next_tier_and_names() {
        assert_eq!(Tier::RubberStamp.next(), Some(Tier::Coworker));
        assert_eq!(Tier::Irreplaceable.next(), None);
        assert_eq!(serde_json::to_string(&Tier::SharpEyed).unwrap(), "\"sharp_eyed\"");
        for t in Tier::ALL {
            assert_eq!(serde_json::to_string(&t).unwrap(), format!("\"{}\"", t.id()));
            assert_eq!(t.id().parse(), Ok(t));
            assert_eq!(t.name().parse(), Ok(t), "older stores saved display names");
        }
        assert!("Legend".parse::<Tier>().is_err());
    }

    #[test]
    fn answer_is_no_from_coworker_up() {
        assert_eq!(Answer::for_tier(None), Answer::Hmm);
        assert_eq!(Answer::for_tier(Some(Tier::RubberStamp)), Answer::Hmm);
        assert_eq!(Answer::for_tier(Some(Tier::Coworker)), Answer::No);
        assert_eq!(serde_json::to_string(&Answer::Hmm).unwrap(), "\"hmm\"");
    }

    #[test]
    fn score_needs_enough_tasks() {
        assert_eq!(Score::of(w(MIN_BRIEFS - 1, MIN_BRIEFS - 1, 50, 0)), None);
        assert!(Score::of(w(MIN_BRIEFS, 0, 0, 0)).is_some());
    }

    #[test]
    fn score_mixes_direction_and_judgment() {
        // The author's month: 125 of 150 landed, 41 catches to 2.
        let s = Score::of(w(150, 125, 41, 2)).unwrap();
        assert_eq!((s.direction, s.judgment, s.score, s.tier), (83, 93, 86, Tier::SharpEyed));
        // Architect-level briefs and a clean record: Irreplaceable.
        assert_eq!(Score::of(w(100, 92, 10, 0)).unwrap().tier, Tier::Irreplaceable);
        // Landing everything but never catching anything: no proof you check its work.
        let s = Score::of(w(50, 50, 0, 0)).unwrap();
        assert_eq!((s.judgment, s.score, s.tier), (0, 70, Tier::Coworker));
        // The AI keeps correcting you: on 18% of tasks, 10 points are left.
        assert_eq!(Score::of(w(50, 40, 3, 9)).unwrap().judgment, 10);
        assert_eq!(Score::of(w(50, 40, 3, 30)).unwrap().judgment, 0, "never below zero");
        // A thin month: 2 corrections on 10 tasks count as 2 on 30, not 20%.
        assert_eq!(Score::of(w(10, 10, 3, 2)).unwrap().judgment, 66);
    }

    #[test]
    fn catching_fewer_mistakes_never_costs_points() {
        // Past the proof, fewer catches (because the AI is getting it right) changes nothing,
        // even when it has corrected you too.
        for humbled in [0, 1, 2, 5] {
            let many = Score::of(w(100, 85, 40, humbled)).unwrap();
            let few = Score::of(w(100, 85, CATCH_PROOF, humbled)).unwrap();
            assert_eq!(many.score, few.score, "humbled {humbled}");
        }
        // Below it, each catch counts toward showing you check.
        let one = Score::of(w(100, 85, 1, 0)).unwrap();
        let two = Score::of(w(100, 85, 2, 0)).unwrap();
        let three = Score::of(w(100, 85, CATCH_PROOF, 0)).unwrap();
        assert!(one.score < two.score && two.score < three.score);
    }

    #[test]
    fn landing_more_always_helps() {
        let mut last = -1;
        for landed in 0..=100 {
            let s = Score::of(w(100, landed, 5, 1)).unwrap().score;
            assert!(s >= last, "{landed}");
            last = s;
        }
    }

    #[test]
    fn landed_past_continued_is_clamped() {
        assert_eq!(Score::of(w(10, 99, 3, 0)).unwrap().direction, 100);
    }
}
