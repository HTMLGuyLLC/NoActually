//! Static tables the UI needs (tiers, categories, sources), so it never hard-codes them.

use crate::{GOAL_WEEKS, MIN_WEEK};
use engine::{
    Category, CorrectionTier, Grade, PromptTier, Signal, Steer, Tier, CORRECTION_PROOF, DIRECTION_WEIGHT, HUMBLED_BASE,
    HUMBLED_COST, MIN_PROMPTS, WINDOW_DAYS,
};
use serde::Serialize;
use sources::Source;

/// Everything the UI would otherwise duplicate from Rust.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Meta {
    /// Overall tiers, ascending by floor.
    pub tiers: Vec<TierInfo>,
    /// Percent of the overall score from landed tasks; the rest is who corrects whom.
    pub direction_weight: i64,
    /// Corrections in the window that fully show you check its work.
    pub correction_proof: i64,
    /// Judgment points lost per 1% of tasks the AI had to correct you on.
    pub humbled_cost: i64,
    /// Fewest tasks corrections are counted against.
    pub humbled_base: i64,
    /// In display order.
    pub categories: Vec<CategoryInfo>,
    /// In display order.
    pub sources: Vec<SourceInfo>,
    /// Prompt tiers, ascending by floor.
    pub prompt_tiers: Vec<PromptTierInfo>,
    /// "Do you correction its mistakes?" tiers, by judgment, ascending.
    pub correction_tiers: Vec<PromptTierInfo>,
    /// Continued tasks in the window before a prompt tier is given.
    pub min_prompts: i64,
    /// Days of tasks a prompt tier looks at.
    pub prompt_window_days: i64,
    /// What a prompt can carry, in display order.
    pub signals: Vec<SignalInfo>,
    /// Signals a prompt needs to be sharp.
    pub sharp_signals: u32,
    /// Prompt grades, lowest first.
    pub grades: Vec<Titled>,
    /// Why a task was steered, in display order.
    pub steers: Vec<Titled>,
    /// Prompts a week needs before it counts toward a chart point, goal or best.
    pub min_week: i64,
    /// Weeks a habit or steering goal must be met.
    pub goal_weeks: i64,
}

/// A tier with a one-word answer: prompt tiers and correction tiers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromptTierInfo {
    /// Stable id (`tech_lead`).
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// Percent of the window's continued tasks that must land.
    pub floor: i64,
    /// Its answer ("Sometimes.").
    pub verdict: &'static str,
}

/// One prompt signal.
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
    /// Overall score needed to reach it.
    pub floor: i64,
}

/// One correction category.
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
        direction_weight: DIRECTION_WEIGHT,
        correction_proof: CORRECTION_PROOF,
        humbled_cost: HUMBLED_COST,
        humbled_base: HUMBLED_BASE,
        categories: Category::ALL.iter().map(|c| CategoryInfo { id: c.as_str(), title: c.title() }).collect(),
        sources: sources.iter().map(|s| SourceInfo { id: s.id(), name: s.name() }).collect(),
        prompt_tiers: PromptTier::ALL
            .iter()
            .map(|t| PromptTierInfo { id: t.id(), name: t.name(), floor: t.floor(), verdict: t.verdict() })
            .collect(),
        correction_tiers: CorrectionTier::ALL
            .iter()
            .map(|t| PromptTierInfo { id: t.id(), name: t.name(), floor: t.floor(), verdict: t.verdict() })
            .collect(),
        min_prompts: MIN_PROMPTS,
        prompt_window_days: WINDOW_DAYS,
        signals: Signal::ALL.iter().map(|s| SignalInfo { id: s.as_str(), title: s.title(), hint: s.hint() }).collect(),
        sharp_signals: Grade::SHARP,
        grades: Grade::ALL.iter().map(|g| Titled { id: g.as_str(), title: g.title() }).collect(),
        steers: Steer::ALL.iter().map(|c| Titled { id: c.as_str(), title: c.title() }).collect(),
        min_week: MIN_WEEK,
        goal_weeks: GOAL_WEEKS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_ui_contract() {
        let m = serde_json::to_value(meta(&sources::all())).unwrap();
        assert_eq!(m["tiers"][0], serde_json::json!({"id":"rubber_stamp","name":"Rubber stamp","floor":0}));
        assert_eq!(m["tiers"][3], serde_json::json!({"id":"irreplaceable","name":"Irreplaceable","floor":90}));
        assert_eq!(m["categories"][0], serde_json::json!({"id":"made_it_up","title":"Made it up"}));
        assert_eq!(m["categories"].as_array().unwrap().len(), Category::ALL.len());
        assert_eq!(m["sources"][0], serde_json::json!({"id":"claude-code","name":"Claude Code"}));
        assert_eq!(
            m["prompt_tiers"][2],
            serde_json::json!({"id":"tech_lead","name":"Tech lead","floor":80,"verdict":"Sometimes."})
        );
        assert_eq!(m["signals"][0]["id"], "spot");
        assert_eq!(m["grades"][2], serde_json::json!({"id":"sharp","title":"Sharp"}));
        assert_eq!(m["steers"][0], serde_json::json!({"id":"ai_wrong","title":"It got it wrong"}));
        assert_eq!((m["min_prompts"].as_i64(), m["sharp_signals"].as_u64()), (Some(10), Some(3)));
        assert_eq!(m["prompt_window_days"], 28);
        assert_eq!((m["direction_weight"].as_i64(), m["correction_proof"].as_i64()), (Some(70), Some(3)));
        assert_eq!((m["min_week"].as_i64(), m["goal_weeks"].as_i64()), (Some(5), Some(4)));
    }
}
