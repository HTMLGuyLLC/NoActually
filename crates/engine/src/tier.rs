//! Tiers and the headline answer, from catches in the last 7 days. The one table the
//! scorecard, quick glance, CLI and milestone popups all read.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// How sharp you've been this week. Ordered from lowest to highest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Fewer than 3 catches in the last 7 days.
    RubberStamp,
    /// 3 to 6.
    Coworker,
    /// 7 to 14.
    SharpEyed,
    /// 15 or more.
    Irreplaceable,
}

impl Tier {
    /// Every tier, ascending by floor.
    pub const ALL: [Tier; 4] = [Tier::RubberStamp, Tier::Coworker, Tier::SharpEyed, Tier::Irreplaceable];

    /// Catches in the last 7 days needed to reach this tier.
    pub const fn floor(self) -> i64 {
        match self {
            Tier::RubberStamp => 0,
            Tier::Coworker => 3,
            Tier::SharpEyed => 7,
            Tier::Irreplaceable => 15,
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

    /// The highest tier whose floor `catches_last_7d` reaches.
    pub fn for_catches(catches_last_7d: i64) -> Tier {
        Tier::ALL.into_iter().rev().find(|t| catches_last_7d >= t.floor()).unwrap_or(Tier::RubberStamp)
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
    /// You caught it often enough this week to be at least a Coworker.
    No,
    /// Not enough catches this week to say.
    Hmm,
}

impl Answer {
    /// "No" once the week's catches reach the Coworker floor.
    pub fn for_catches(catches_last_7d: i64) -> Answer {
        if catches_last_7d >= Tier::Coworker.floor() {
            Answer::No
        } else {
            Answer::Hmm
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_follow_floors() {
        let at = |n| Tier::for_catches(n);
        assert_eq!((at(-1), at(0), at(2)), (Tier::RubberStamp, Tier::RubberStamp, Tier::RubberStamp));
        assert_eq!((at(3), at(6)), (Tier::Coworker, Tier::Coworker));
        assert_eq!((at(7), at(14)), (Tier::SharpEyed, Tier::SharpEyed));
        assert_eq!((at(15), at(500)), (Tier::Irreplaceable, Tier::Irreplaceable));
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
        assert_eq!(Answer::for_catches(2), Answer::Hmm);
        assert_eq!(Answer::for_catches(3), Answer::No);
        assert_eq!(serde_json::to_string(&Answer::Hmm).unwrap(), "\"hmm\"");
    }
}
