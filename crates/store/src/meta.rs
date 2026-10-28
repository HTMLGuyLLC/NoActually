//! Static tables the UI needs (tiers, categories, sources), so it never hard-codes them.

use engine::{BriefTier, Category, Grade, Signal, Steer, Tier, MIN_BRIEFS};
use serde::Serialize;
use sources::Source;

/// Everything the UI would otherwise duplicate from Rust.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Meta {
    /// Ascending by floor.
    pub tiers: Vec<TierInfo>,
    /// In display order.
    pub categories: Vec<CategoryInfo>,
    /// In display order.
    pub sources: Vec<SourceInfo>,
    /// Brief tiers, ascending by floor.
    pub brief_tiers: Vec<BriefTierInfo>,
    /// Tasks in the last 7 days before a brief tier is given.
    pub min_briefs: i64,
    /// What a brief can carry, in display order.
    pub signals: Vec<SignalInfo>,
    /// Signals a brief needs to be sharp.
    pub sharp_signals: u32,
    /// Brief grades, lowest first.
    pub grades: Vec<Titled>,
    /// Why a task was steered, in display order.
    pub steers: Vec<Titled>,
}

/// One brief tier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BriefTierInfo {
    /// Stable id (`tech_lead`).
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// Percent of the week's tasks that must land.
    pub floor: i64,
    /// The answer to "Do you make it guess?".
    pub verdict: &'static str,
}

/// One brief signal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SignalInfo {
    /// Stable id (`spot`).
    pub id: &'static str,
    /// Display title.
    pub title: &'static str,
    /// What counts, in a few words.
    pub hint: &'static str,
}

/// An id and its display title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Titled {
    /// Stable id.
    pub id: &'static str,
    /// Display title.
    pub title: &'static str,
}

/// One tier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TierInfo {
    /// Stable id (`sharp_eyed`).
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// Catches in the last 7 days needed to reach it.
    pub floor: i64,
}

/// One catch category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CategoryInfo {
    /// Stable id (`made_it_up`).
    pub id: &'static str,
    /// Display title.
    pub title: &'static str,
}

/// One source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceInfo {
    /// Stable id (`claude-code`).
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
}

/// The tables for these sources.
pub fn meta(sources: &[Box<dyn Source>]) -> Meta {
    Meta {
        tiers: Tier::ALL.iter().map(|t| TierInfo { id: t.id(), name: t.name(), floor: t.floor() }).collect(),
        categories: Category::ALL.iter().map(|c| CategoryInfo { id: c.as_str(), title: c.title() }).collect(),
        sources: sources.iter().map(|s| SourceInfo { id: s.id(), name: s.name() }).collect(),
        brief_tiers: BriefTier::ALL
            .iter()
            .map(|t| BriefTierInfo { id: t.id(), name: t.name(), floor: t.floor(), verdict: t.verdict() })
            .collect(),
        min_briefs: MIN_BRIEFS,
        signals: Signal::ALL.iter().map(|s| SignalInfo { id: s.as_str(), title: s.title(), hint: s.hint() }).collect(),
        sharp_signals: Grade::SHARP,
        grades: Grade::ALL.iter().map(|g| Titled { id: g.as_str(), title: g.title() }).collect(),
        steers: Steer::ALL.iter().map(|c| Titled { id: c.as_str(), title: c.title() }).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_ui_contract() {
        let m = serde_json::to_value(meta(&sources::all())).unwrap();
        assert_eq!(m["tiers"][0], serde_json::json!({"id":"rubber_stamp","name":"Rubber stamp","floor":0}));
        assert_eq!(m["tiers"][3], serde_json::json!({"id":"irreplaceable","name":"Irreplaceable","floor":15}));
        assert_eq!(m["categories"][0], serde_json::json!({"id":"made_it_up","title":"Made it up"}));
        assert_eq!(m["categories"].as_array().unwrap().len(), Category::ALL.len());
        assert_eq!(m["sources"][0], serde_json::json!({"id":"claude-code","name":"Claude Code"}));
        assert_eq!(
            m["brief_tiers"][2],
            serde_json::json!({"id":"tech_lead","name":"Tech lead","floor":90,"verdict":"Sometimes."})
        );
        assert_eq!(m["signals"][0]["id"], "spot");
        assert_eq!(m["grades"][2], serde_json::json!({"id":"sharp","title":"Sharp"}));
        assert_eq!(m["steers"][0], serde_json::json!({"id":"ai_wrong","title":"It got it wrong"}));
        assert_eq!((m["min_briefs"].as_i64(), m["sharp_signals"].as_u64()), (Some(5), Some(3)));
    }
}
