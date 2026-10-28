//! Static tables the UI needs (tiers, categories, sources), so it never hard-codes them.

use engine::{Category, Tier};
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
    }
}
