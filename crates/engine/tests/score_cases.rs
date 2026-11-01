//! The score rule, as a table. The browser demo re-derives the score in TypeScript
//! (`judge` in `app/src/api.demo.ts`), and its Vitest suite reads this same file, so the two can't
//! drift apart unnoticed.
//!
//! Nothing in `expected` is written by hand. To add a case, add its `name` and `window` with any
//! `expected`, then run:
//!
//! ```text
//! WRITE_SCORE_CASES=1 cargo test -p noactually-engine --test score_cases the_table_matches
//! ```

use engine::{Answer, CorrectionTier, Score, Tier, Window, MIN_PROMPTS};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

const CASES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/score-cases.json");

#[derive(Debug, Serialize, Deserialize)]
struct Table {
    cases: Vec<Case>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Case {
    name: String,
    window: Input,
    expected: Option<Expected>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Input {
    continued: i64,
    landed: i64,
    corrections: i64,
    humbled: i64,
    days: i64,
}

/// What the score says, the way `stats.rs` turns it into `Overall`.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Expected {
    score: Option<i64>,
    direction: Option<i64>,
    judgment: Option<i64>,
    tier: Option<String>,
    next_tier: Option<String>,
    verdict: Answer,
    verdict_label: String,
    correction_tier: Option<String>,
    next_correction_tier: Option<String>,
    to_rank: i64,
}

fn judge(i: Input) -> Expected {
    let window = Window {
        continued: i.continued,
        landed: i.landed,
        corrections: i.corrections,
        humbled: i.humbled,
        days: i.days,
    };
    let score = Score::of(window);
    let tier = score.map(|s| s.tier);
    let correction_tier = score.map(|s| CorrectionTier::for_judgment(s.judgment));
    let verdict = Answer::for_tier(tier);
    Expected {
        score: score.map(|s| s.score),
        direction: score.map(|s| s.direction),
        judgment: score.map(|s| s.judgment),
        tier: tier.map(|t| t.id().to_string()),
        next_tier: tier.and_then(Tier::next).map(|t| t.id().to_string()),
        verdict,
        verdict_label: verdict.label().to_string(),
        correction_tier: correction_tier.map(|t| t.id().to_string()),
        next_correction_tier: correction_tier.and_then(CorrectionTier::next).map(|t| t.id().to_string()),
        to_rank: (MIN_PROMPTS - i.continued).max(0),
    }
}

fn table() -> Table {
    serde_json::from_str(&std::fs::read_to_string(CASES).unwrap()).unwrap()
}

#[test]
fn the_table_matches_the_real_tier_code() {
    let mut table = table();
    if std::env::var_os("WRITE_SCORE_CASES").is_some() {
        for case in &mut table.cases {
            case.expected = Some(judge(case.window));
        }
        let json = serde_json::to_string_pretty(&table).unwrap();
        std::fs::write(Path::new(CASES), json + "\n").unwrap();
    }
    for case in &table.cases {
        assert_eq!(case.expected.as_ref(), Some(&judge(case.window)), "{}", case.name);
    }
}

#[test]
fn the_table_reaches_every_outcome() {
    let table = table();
    let names: BTreeSet<&str> = table.cases.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names.len(), table.cases.len(), "every case has its own name");

    let reached = |f: fn(&Expected) -> Option<&str>| -> BTreeSet<String> {
        table.cases.iter().filter_map(|c| f(c.expected.as_ref()?)).map(str::to_string).collect()
    };
    let tiers: BTreeSet<String> = Tier::ALL.iter().map(|t| t.id().to_string()).collect();
    let correction_tiers: BTreeSet<String> = CorrectionTier::ALL.iter().map(|t| t.id().to_string()).collect();
    assert_eq!(reached(|e| e.tier.as_deref()), tiers);
    assert_eq!(reached(|e| e.correction_tier.as_deref()), correction_tiers);
    assert_eq!(reached(|e| Some(&e.verdict_label)).len(), 2, "both answers");
    assert!(table.cases.iter().any(|c| c.expected.as_ref().is_some_and(|e| e.score.is_none())), "an unranked window");
}
