//! "Could AI replace you?": one score from both sides of working with AI, and the tiers and
//! headline answer it gives. The one table the scorecard, quick glance, CLI and milestone popups
//! all read.
//!
//! The score mixes direction (how often your tasks land without you steering them) with judgment
//! (how rarely the AI has to correct you, once you've shown you check its work). Corrections only
//! have to show that you check: past [`CORRECTION_PROOF`] in the window, more add nothing, and
//! judgment never compares your corrections with the AI's, so prompting better and correcting fewer
//! mistakes never costs you points.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Percent of the score that comes from direction (landed tasks); the rest is judgment.
pub const DIRECTION_WEIGHT: i64 = 70;
/// Corrections in the window that fully show you check its work.
pub const CORRECTION_PROOF: i64 = 3;
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

/// How well your prompts land: of the tasks in the last [`WINDOW_DAYS`] that went past their first
/// message, the share you never had to add to, clarify or redo. A one-message task can't need
/// steering, so it doesn't count either way. The AI getting it wrong doesn't count against you
/// (that's a correction). Ordered lowest first.
///
/// The floors come from one heavy user's history (tasks that continued landed 83–90% of the time
/// over 28 days, typically 86%). Public chat logs can't set them: casual chats almost never steer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptTier {
    /// Under 70% landed.
    VibesOnly,
    /// 70% or more.
    TicketWriter,
    /// 80% or more.
    TechLead,
    /// 90% or more.
    Architect,
}

/// Days of tasks a prompt tier looks at: long enough that one "also…" doesn't swing it.
pub const WINDOW_DAYS: i64 = 28;

/// Tasks that continued, in the window, needed before a tier is given; fewer says too little.
pub const MIN_PROMPTS: i64 = 10;

impl PromptTier {
    /// Every tier, ascending by floor.
    pub const ALL: [PromptTier; 4] =
        [PromptTier::VibesOnly, PromptTier::TicketWriter, PromptTier::TechLead, PromptTier::Architect];

    /// Percent of the window's continued tasks that must land.
    pub const fn floor(self) -> i64 {
        match self {
            PromptTier::VibesOnly => 0,
            PromptTier::TicketWriter => 70,
            PromptTier::TechLead => 80,
            PromptTier::Architect => 90,
        }
    }

    /// Stable id, as stored and serialized.
    pub const fn id(self) -> &'static str {
        match self {
            PromptTier::VibesOnly => "vibes_only",
            PromptTier::TicketWriter => "ticket_writer",
            PromptTier::TechLead => "tech_lead",
            PromptTier::Architect => "architect",
        }
    }

    /// Display name.
    pub const fn name(self) -> &'static str {
        match self {
            PromptTier::VibesOnly => "Vibes only",
            PromptTier::TicketWriter => "Ticket writer",
            PromptTier::TechLead => "Tech lead",
            PromptTier::Architect => "Architect",
        }
    }

    /// The answer to "Do you make it guess?" at this tier.
    pub const fn verdict(self) -> &'static str {
        match self {
            PromptTier::VibesOnly => "Constantly.",
            PromptTier::TicketWriter => "Often.",
            PromptTier::TechLead => "Sometimes.",
            PromptTier::Architect => "Rarely.",
        }
    }

    /// The tier reached when `landed` of `tasks` landed, or `None` below [`MIN_PROMPTS`] tasks.
    pub fn for_tasks(landed: i64, tasks: i64) -> Option<PromptTier> {
        (tasks >= MIN_PROMPTS)
            .then(|| PromptTier::ALL.into_iter().rev().find(|t| landed * 100 >= t.floor() * tasks))
            .flatten()
    }

    /// The tier above this one, if any.
    pub fn next(self) -> Option<PromptTier> {
        PromptTier::ALL.into_iter().find(|t| *t > self)
    }
}

impl fmt::Display for PromptTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for PromptTier {
    type Err = UnknownTier;

    /// Accepts the id only: prompt tiers were never stored by display name.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        PromptTier::ALL.into_iter().find(|t| t.id() == s).ok_or_else(|| UnknownTier(s.to_string()))
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

/// "Do you correct its mistakes?" How judgment reads, in a word (rarely < sometimes < often <
/// usually; never "always": nobody catches everything). Ordered from lowest to highest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionTier {
    /// Judgment under 50.
    Autopilot,
    /// 50 to 74.
    SpotChecker,
    /// 75 to 89.
    Reviewer,
    /// 90 or more.
    HawkEyed,
}

impl CorrectionTier {
    /// Every tier, ascending by floor.
    pub const ALL: [CorrectionTier; 4] =
        [CorrectionTier::Autopilot, CorrectionTier::SpotChecker, CorrectionTier::Reviewer, CorrectionTier::HawkEyed];

    /// The judgment needed to reach this tier.
    pub const fn floor(self) -> i64 {
        match self {
            CorrectionTier::Autopilot => 0,
            CorrectionTier::SpotChecker => 50,
            CorrectionTier::Reviewer => 75,
            CorrectionTier::HawkEyed => 90,
        }
    }

    /// Stable id, as serialized (`hawk_eyed`).
    pub const fn id(self) -> &'static str {
        match self {
            CorrectionTier::Autopilot => "autopilot",
            CorrectionTier::SpotChecker => "spot_checker",
            CorrectionTier::Reviewer => "reviewer",
            CorrectionTier::HawkEyed => "hawk_eyed",
        }
    }

    /// Display name ("Hawk-eyed").
    pub const fn name(self) -> &'static str {
        match self {
            CorrectionTier::Autopilot => "Autopilot",
            CorrectionTier::SpotChecker => "Spot-checker",
            CorrectionTier::Reviewer => "Reviewer",
            CorrectionTier::HawkEyed => "Hawk-eyed",
        }
    }

    /// The answer to "Do you correct its mistakes?" at this tier.
    pub const fn verdict(self) -> &'static str {
        match self {
            CorrectionTier::Autopilot => "Rarely.",
            CorrectionTier::SpotChecker => "Sometimes.",
            CorrectionTier::Reviewer => "Often.",
            CorrectionTier::HawkEyed => "Usually.",
        }
    }

    /// The highest tier whose floor `judgment` reaches.
    pub fn for_judgment(judgment: i64) -> CorrectionTier {
        CorrectionTier::ALL.into_iter().rev().find(|t| judgment >= t.floor()).unwrap_or(CorrectionTier::Autopilot)
    }

    /// The tier above this one, if any.
    pub fn next(self) -> Option<CorrectionTier> {
        CorrectionTier::ALL.into_iter().find(|t| *t > self)
    }
}

/// The last [`crate::WINDOW_DAYS`] days, counted the way the score reads them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Window {
    /// Tasks that went past their first message.
    pub continued: i64,
    /// Of those, the ones you never had to add to, clarify or redo.
    pub landed: i64,
    /// Times you corrected the AI.
    pub corrections: i64,
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
    /// [`CORRECTION_PROOF`] corrections.
    pub judgment: i64,
    /// The tier `score` reaches.
    pub tier: Tier,
}

impl Score {
    /// The score for a window, or `None` until it has [`MIN_PROMPTS`] continued tasks: with fewer,
    /// one task swings the number too much to rank.
    pub fn of(w: Window) -> Option<Score> {
        if w.continued < MIN_PROMPTS {
            return None;
        }
        // Thousandths so the weights don't round twice.
        let direction = 1000 * w.landed.clamp(0, w.continued) / w.continued;
        // Per task, not per correction: comparing with corrections would make fewer corrections cost points.
        let right = (1000 - HUMBLED_COST * 1000 * w.humbled.max(0) / w.continued.max(HUMBLED_BASE)).max(0);
        let judgment = right * w.corrections.clamp(0, CORRECTION_PROOF) / CORRECTION_PROOF;
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

    fn w(continued: i64, landed: i64, corrections: i64, humbled: i64) -> Window {
        Window { continued, landed, corrections, humbled }
    }

    #[test]
    fn correction_tiers_follow_judgment() {
        let at = |n| CorrectionTier::for_judgment(n);
        assert_eq!(
            (at(-1), at(0), at(49)),
            (CorrectionTier::Autopilot, CorrectionTier::Autopilot, CorrectionTier::Autopilot)
        );
        assert_eq!((at(50), at(74)), (CorrectionTier::SpotChecker, CorrectionTier::SpotChecker));
        assert_eq!((at(75), at(89)), (CorrectionTier::Reviewer, CorrectionTier::Reviewer));
        assert_eq!((at(90), at(100)), (CorrectionTier::HawkEyed, CorrectionTier::HawkEyed));
        assert!(CorrectionTier::ALL.windows(2).all(|w| w[0] < w[1] && w[0].floor() < w[1].floor()));
        assert_eq!(CorrectionTier::HawkEyed.next(), None);
        // No corrections yet reads as autopilot, however rarely it corrected you.
        let s = Score::of(w(20, 20, 0, 0)).map(|s| CorrectionTier::for_judgment(s.judgment));
        assert_eq!(s, Some(CorrectionTier::Autopilot));
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
        assert_eq!(Score::of(w(MIN_PROMPTS - 1, MIN_PROMPTS - 1, 50, 0)), None);
        assert!(Score::of(w(MIN_PROMPTS, 0, 0, 0)).is_some());
    }

    #[test]
    fn score_mixes_direction_and_judgment() {
        // The author's month: 125 of 150 landed, 41 corrections to 2.
        let s = Score::of(w(150, 125, 41, 2)).unwrap();
        assert_eq!((s.direction, s.judgment, s.score, s.tier), (83, 93, 86, Tier::SharpEyed));
        // Architect-level prompts and a clean record: Irreplaceable.
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
    fn correcting_fewer_mistakes_never_costs_points() {
        // Past the proof, fewer corrections (because the AI is getting it right) changes nothing,
        // even when it has corrected you too.
        for humbled in [0, 1, 2, 5] {
            let many = Score::of(w(100, 85, 40, humbled)).unwrap();
            let few = Score::of(w(100, 85, CORRECTION_PROOF, humbled)).unwrap();
            assert_eq!(many.score, few.score, "humbled {humbled}");
        }
        // Below it, each correction counts toward showing you check.
        let one = Score::of(w(100, 85, 1, 0)).unwrap();
        let two = Score::of(w(100, 85, 2, 0)).unwrap();
        let three = Score::of(w(100, 85, CORRECTION_PROOF, 0)).unwrap();
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
