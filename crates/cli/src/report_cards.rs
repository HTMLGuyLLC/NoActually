//! `noactually stats --report-cards`: the report cards that still count (a newer card for the same
//! task replaces an older one), added up over the models and range `stats` filters by.
//!
//! Numbers only, as stored; no chat text. [`Summary`] is the `--json` output, whose fields and
//! their order stay as they are. A number that can't be worked out is `null`, never missing.

use engine::LetterGrade;
use serde::Serialize;
use sources::{GRADE_COMMAND, GRADE_SKILL_CODEX};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use store::{HabitInfo, RecentReportCard, ReportCardWeek, TipTrend, TopTip};

/// The habit report cards most often say was left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Missed {
    /// The name a report card writes it with (`finish`).
    pub habit: String,
    /// What the app calls it ("Finish line").
    pub title: String,
    /// Graded tasks that left it out.
    pub tasks: i64,
}

/// Everything `--report-cards` prints, and the whole of `--report-cards --json`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Summary {
    /// Graded tasks.
    pub graded: i64,
    /// Graded tasks per grade, "A" to "F" (all five keys, 0 when none).
    pub grades: BTreeMap<String, i64>,
    /// Percent of graded tasks that got an A or B, rounded down. `null` when none are graded.
    pub ab_percent: Option<i64>,
    /// Follow-ups that redid work, summed.
    pub avoidable: i64,
    /// `avoidable` over `graded`, to two places. `null` when none are graded.
    pub avoidable_per_task: Option<f64>,
    /// What the redone work cost in US dollars at list prices, from the fully priced tasks only (one
    /// that used any model with no price is left out). `null` when no graded task is fully priced.
    pub extra_cost: Option<f64>,
    /// Graded tasks every model of which has a price: the ones `extra_cost` adds up.
    pub priced: i64,
    /// When the prices were last checked.
    pub prices_as_of: &'static str,
    /// Tokens redone: new input, cache writes and output.
    pub extra_tokens: u64,
    /// Minutes of the redone work.
    pub extra_minutes: i64,
    /// Graded tasks that left out each habit, by name (all six keys, 0 when none).
    pub missing: BTreeMap<String, i64>,
    /// The habit left out most often, or `null` when none was left out. A tie goes to the habit
    /// a report card lists first.
    pub most_missed: Option<Missed>,
    /// The latest report cards that count, newest first.
    pub recent: Vec<RecentReportCard>,
    /// How many tasks needed a tip, last full week against the week before. `null` until both have
    /// enough tasks.
    pub tip_trend: Option<TipTrend>,
    /// The tip you get most. `null` until there are enough tips.
    pub top_tip: Option<TopTip>,
    /// Report cards written in chat that the app couldn't count, across all your tools.
    pub left_out: i64,
}

/// The summary of `totals` (see `Store::report_card_totals`) and the latest cards. `habits` are the
/// habits in the order a report card lists them.
pub(crate) fn summary(totals: &ReportCardWeek, recent: Vec<RecentReportCard>, habits: &[HabitInfo]) -> Summary {
    let n = totals.graded;
    let grades = LetterGrade::ALL.iter().map(|g| (g.as_str().to_string(), totals.count(*g))).collect();
    let missing: BTreeMap<String, i64> =
        habits.iter().map(|h| (h.id.to_string(), totals.missing.get(h.id).copied().unwrap_or(0))).collect();
    let most_missed = habits
        .iter()
        .map(|h| (h, missing[h.id]))
        .filter(|(_, tasks)| *tasks > 0)
        // The first of equal counts, which `max_by_key` would give to the last.
        .rev()
        .max_by_key(|(_, tasks)| *tasks)
        .map(|(h, tasks)| Missed { habit: h.id.to_string(), title: h.title.to_string(), tasks });
    Summary {
        graded: n,
        grades,
        ab_percent: totals.ab_pct(),
        avoidable: totals.avoidable,
        avoidable_per_task: (n > 0).then(|| (totals.avoidable as f64 / n as f64 * 100.0).round() / 100.0),
        extra_cost: totals.extra_cost,
        priced: totals.priced,
        prices_as_of: engine::PRICES_AS_OF,
        extra_tokens: totals.extra_tokens,
        extra_minutes: totals.extra_minutes,
        missing,
        most_missed,
        recent,
        tip_trend: None,
        top_tip: None,
        left_out: 0,
    }
}

/// What a card's redone work took, minutes first: "about 4 extra minutes · about $0.30 at pay-as-you-go prices".
/// Dollars only when the task has a price. Never a count of tokens.
fn extra_text(minutes: i64, cost: Option<f64>) -> String {
    let mut parts = Vec::new();
    if minutes > 0 {
        parts.push(format!("about {minutes} extra minutes"));
    }
    if let Some(c) = cost.filter(|c| *c >= 0.005) {
        parts.push(format!("about ${c:.2} at pay-as-you-go prices"));
    }
    if parts.is_empty() {
        "nothing redone".into()
    } else {
        parts.join(" · ")
    }
}

/// "Last week, 6 of 40 tasks got a tip (down from 9 of 38 the week before)." and the tip you get most, when there's
/// enough to say each.
fn tip_lines(s: &Summary, habits: &[HabitInfo]) -> Vec<String> {
    let trend = s.tip_trend.as_ref().map(|t| {
        let (now, then) = (&t.latest, &t.before);
        // Shares, not counts: 3 of 30 is the same as 2 of 20.
        let change = match (now.tips * then.tasks).cmp(&(then.tips * now.tasks)) {
            Ordering::Greater => "up from",
            Ordering::Less => "down from",
            Ordering::Equal => "same as",
        };
        format!(
            "  Last week, {} of {} tasks got a tip ({change} {} of {} the week before).",
            now.tips, now.tasks, then.tips, then.tasks
        )
    });
    let top = s.top_tip.as_ref().and_then(|t| {
        let plain = habits.iter().find(|h| h.id == t.habit)?.plain;
        Some(format!("  Your most common tip: {plain} ({} of your last {}).", t.tips, t.of))
    });
    trend.into_iter().chain(top).collect()
}

/// Says so when report cards in chat never reached the app.
fn left_out_line(s: &Summary) -> Option<String> {
    (s.left_out > 0).then(|| {
        let cards = if s.left_out == 1 { "report card" } else { "report cards" };
        format!("  Couldn't count {} {cards} from your chats. See the app's log for why.", s.left_out)
    })
}

/// The text `--report-cards` prints. `window` says which part of history it is ("all history",
/// "since 2026-10-01").
pub(crate) fn lines(s: &Summary, window: &str, habits: &[HabitInfo]) -> Vec<String> {
    if s.graded == 0 {
        let none = format!(
            "No report cards {window}. Ask for one with {GRADE_COMMAND} in Claude Code or {GRADE_SKILL_CODEX} in Codex, then run `noactually sync`."
        );
        return std::iter::once(none).chain(tip_lines(s, habits)).chain(left_out_line(s)).collect();
    }
    let grades: Vec<String> = s.grades.iter().map(|(g, n)| format!("{g} {n}")).collect();
    let one = s.graded == 1;
    let tasks = if one { "task" } else { "tasks" };
    let missed = s
        .most_missed
        .as_ref()
        .map_or("nothing".to_string(), |m| format!("{} ({} of {} {tasks})", m.title, m.tasks, s.graded));
    let mut out = vec![
        format!("Report cards {window}"),
        format!("  Graded tasks    {} ({})", s.graded, grades.join(" · ")),
        format!("  A or B          {}%", s.ab_percent.unwrap_or(0)),
        format!(
            "  Redid work      {:.2} per graded task ({} {} that redid work)",
            s.avoidable_per_task.unwrap_or(0.0),
            s.avoidable,
            if s.avoidable == 1 { "follow-up" } else { "follow-ups" }
        ),
        format!("  Extra minutes   {}", s.extra_minutes),
    ];
    // Dollars only when something has a price. A subscription pays out of its usage limit instead.
    if let Some(c) = s.extra_cost {
        out.push(format!(
            "  Extra cost      about ${c:.2} at pay-as-you-go prices ({} of {} {} a price; prices as of {})",
            s.priced,
            s.graded,
            if one { "task has" } else { "tasks have" },
            s.prices_as_of
        ));
    }
    out.push(format!("  Most missed     {missed}"));
    out.extend(tip_lines(s, habits));
    out.extend(left_out_line(s));
    if !s.recent.is_empty() {
        out.push("\n  Latest".into());
        for c in &s.recent {
            let left_out = c.missing.as_deref().map_or("nothing left out".to_string(), |id| {
                let title = habits.iter().find(|h| h.id == id).map_or(id, |h| h.title);
                format!("left out: {}", title.to_lowercase())
            });
            out.push(format!(
                "    {}  {:<13} {}  {}  {}",
                c.day,
                c.source,
                c.grade,
                left_out,
                extra_text(c.extra_minutes, c.extra_cost)
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use store::TipWeek;

    fn habits() -> Vec<HabitInfo> {
        store::meta(&sources::all()).habits
    }

    fn totals() -> ReportCardWeek {
        ReportCardWeek {
            graded: 12,
            grades: [("A", 3), ("B", 6), ("C", 2), ("D", 1)].map(|(g, n)| (g.to_string(), n)).into(),
            avoidable: 7,
            extra_tokens: 345_000,
            extra_cost: Some(3.421),
            priced: 10,
            extra_minutes: 120,
            missing: [("finish", 6), ("target", 5), ("context", 2)].map(|(h, n)| (h.to_string(), n)).into(),
            ..ReportCardWeek::default()
        }
    }

    fn recent() -> Vec<RecentReportCard> {
        vec![
            RecentReportCard {
                day: "2026-10-27".into(),
                graded_day: "2026-10-28".parse().unwrap(),
                source: "claude-code".into(),
                grade: "B".into(),
                missing: Some("finish".into()),
                extra_cost: Some(0.3),
                extra_tokens: 1000,
                extra_minutes: 4,
            },
            RecentReportCard {
                day: "2026-10-20".into(),
                graded_day: "2026-10-20".parse().unwrap(),
                source: "codex".into(),
                grade: "A".into(),
                missing: None,
                extra_cost: None,
                extra_tokens: 0,
                extra_minutes: 0,
            },
            RecentReportCard {
                day: "2026-10-19".into(),
                graded_day: "2026-10-21".parse().unwrap(),
                source: "codex".into(),
                grade: "C".into(),
                missing: Some("context".into()),
                extra_cost: None,
                extra_tokens: 12_345,
                extra_minutes: 9,
            },
        ]
    }

    #[test]
    fn the_text_has_every_number_and_says_which_were_priced() {
        let s = summary(&totals(), recent(), &habits());
        let text = lines(&s, "in all history", &habits()).join("\n");
        assert_eq!(
            text,
            "Report cards in all history
  Graded tasks    12 (A 3 · B 6 · C 2 · D 1 · F 0)
  A or B          75%
  Redid work      0.58 per graded task (7 follow-ups that redid work)
  Extra minutes   120
  Extra cost      about $3.42 at pay-as-you-go prices (10 of 12 tasks have a price; prices as of 2026-10)
  Most missed     Finish line (6 of 12 tasks)

  Latest
    2026-10-27  claude-code   B  left out: finish line  about 4 extra minutes · about $0.30 at pay-as-you-go prices
    2026-10-20  codex         A  nothing left out  nothing redone
    2026-10-19  codex         C  left out: context  about 9 extra minutes"
        );
    }

    #[test]
    fn a_range_with_no_price_shows_minutes_and_no_dollars_or_tokens_and_one_with_no_cards_says_how_to_get_one() {
        let unpriced = ReportCardWeek { extra_cost: None, priced: 0, ..totals() };
        let text = lines(&summary(&unpriced, vec![], &habits()), "since 2026-10-01", &habits()).join("\n");
        assert!(text.starts_with("Report cards since 2026-10-01\n"), "{text}");
        assert!(text.contains("  Extra minutes   120\n"), "minutes are there whatever the price: {text}");
        assert!(!text.contains("Extra cost") && !text.contains('$'), "no price, no dollars: {text}");
        assert!(!text.contains("token") && !text.contains("345000"), "never a count of tokens: {text}");
        assert!(!text.contains("Latest"), "{text}");

        let none = summary(&ReportCardWeek::default(), vec![], &habits());
        let text = lines(&none, "since 2026-10-01", &habits());
        assert_eq!(text.len(), 1);
        let ask = format!("Ask for one with {GRADE_COMMAND} in Claude Code or {GRADE_SKILL_CODEX} in Codex");
        assert!(text[0].starts_with("No report cards since 2026-10-01. ") && text[0].contains(&ask), "{}", text[0]);
        let nothing_missed = ReportCardWeek { missing: BTreeMap::new(), ..totals() };
        let text = lines(&summary(&nothing_missed, vec![], &habits()), "in all history", &habits()).join("\n");
        assert!(text.contains("  Most missed     nothing"), "{text}");
    }

    #[test]
    fn cards_that_werent_counted_get_a_line() {
        let s = Summary { left_out: 2, ..summary(&totals(), vec![], &habits()) };
        let text = lines(&s, "in all history", &habits()).join("\n");
        assert!(text.ends_with("Couldn't count 2 report cards from your chats. See the app's log for why."), "{text}");
    }

    #[test]
    fn tips_print_in_their_own_lines_with_or_without_report_cards() {
        let week = |tips, tasks| TipWeek { tips, tasks };
        let with = |s: Summary, latest, before| Summary {
            tip_trend: Some(TipTrend { latest, before }),
            top_tip: Some(TopTip { habit: "finish".into(), tips: 6, of: 20 }),
            ..s
        };
        let cards = with(summary(&totals(), vec![], &habits()), week(6, 40), week(9, 38));
        let text = lines(&cards, "in all history", &habits()).join("\n");
        assert!(text.ends_with("Most missed     Finish line (6 of 12 tasks)\n  Last week, 6 of 40 tasks got a tip (down from 9 of 38 the week before).\n  Your most common tip: say what done looks like (6 of your last 20)."), "{text}");

        let none = with(summary(&ReportCardWeek::default(), vec![], &habits()), week(3, 30), week(2, 20));
        let text = lines(&none, "in all history", &habits());
        assert_eq!(text.len(), 3, "{text:?}");
        assert_eq!(text[1], "  Last week, 3 of 30 tasks got a tip (same as 2 of 20 the week before).");

        let up = with(summary(&totals(), vec![], &habits()), week(9, 30), week(2, 20));
        assert!(lines(&up, "x", &habits()).join("\n").contains("(up from 2 of 20 the week before)"));
    }

    #[test]
    fn a_latest_line_leads_with_minutes_and_names_the_price_it_used() {
        assert_eq!(extra_text(4, Some(0.3)), "about 4 extra minutes · about $0.30 at pay-as-you-go prices");
        assert_eq!(extra_text(9, None), "about 9 extra minutes", "no price, no dollars, and no tokens");
        assert_eq!(extra_text(0, Some(1.5)), "about $1.50 at pay-as-you-go prices");
        assert_eq!(extra_text(0, Some(0.001)), "nothing redone", "under half a cent is nothing");
        assert_eq!(extra_text(0, None), "nothing redone");
    }

    #[test]
    fn the_most_missed_habit_is_the_first_of_a_tie() {
        let tied = ReportCardWeek {
            missing: [("finish", 5), ("target", 5)].map(|(h, n)| (h.to_string(), n)).into(),
            ..totals()
        };
        let s = summary(&tied, vec![], &habits());
        let m = s.most_missed.unwrap();
        assert_eq!((m.habit.as_str(), m.tasks), ("target", 5), "target comes before finish on a card");
        assert_eq!(m.title, "Target");
    }

    #[test]
    fn the_json_has_a_fixed_shape() {
        let s = summary(&totals(), recent()[..1].to_vec(), &habits());
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(
            v,
            json!({
                "graded": 12,
                "grades": {"A": 3, "B": 6, "C": 2, "D": 1, "F": 0},
                "ab_percent": 75,
                "avoidable": 7,
                "avoidable_per_task": 0.58,
                "extra_cost": 3.421,
                "priced": 10,
                "prices_as_of": "2026-10",
                "extra_tokens": 345_000,
                "extra_minutes": 120,
                "missing": {"context": 2, "evidence": 0, "finish": 6, "guardrails": 0, "purpose": 0, "target": 5},
                "most_missed": {"habit": "finish", "title": "Finish line", "tasks": 6},
                "recent": [{"day": "2026-10-27", "graded_day": "2026-10-28", "source": "claude-code", "grade": "B",
                    "missing": "finish", "extra_cost": 0.3, "extra_tokens": 1000, "extra_minutes": 4}],
                "tip_trend": null,
                "top_tip": null,
                "left_out": 0
            })
        );
        // The same keys, in the same order, when nothing is graded: nulls and zeros, nothing missing.
        let empty = serde_json::to_value(summary(&ReportCardWeek::default(), vec![], &habits())).unwrap();
        let keys = |v: &serde_json::Value| v.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
        assert_eq!(keys(&empty), keys(&v));
        assert_eq!(empty["graded"], 0);
        assert!(
            empty["ab_percent"].is_null() && empty["avoidable_per_task"].is_null() && empty["extra_cost"].is_null()
        );
        assert!(empty["most_missed"].is_null());
        assert_eq!(empty["recent"], json!([]));
        assert_eq!(empty["grades"], json!({"A": 0, "B": 0, "C": 0, "D": 0, "F": 0}));
        assert_eq!(empty["missing"].as_object().unwrap().len(), 6);
        // Field order is declaration order, which is what the docs list.
        let printed = serde_json::to_string(&s).unwrap();
        assert!(
            printed
                .starts_with(r#"{"graded":12,"grades":{"A":3,"B":6,"C":2,"D":1,"F":0},"ab_percent":75,"avoidable":7,"#),
            "{printed}"
        );
    }
}
