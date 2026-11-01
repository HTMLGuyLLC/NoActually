//! Popups for moments worth sharing. What counts as a milestone is decided in
//! `store::milestones`; this module only decides which of them to show, and shows them.
//! Bulk reads (first launch, a newly allowed source, a rules rebuild) only move the baseline,
//! so backfilling history never fires a pile of old milestones.

use crate::{lock, App};
use chrono::Local;
use store::milestones::{crossed, goal_milestones, Milestone, Snapshot};
use store::GoalNews;
use tauri::AppHandle;

/// More new messages than a person writes between two checks means history was just read.
pub const BACKFILL_MESSAGES: usize = 40;

/// Which popups a pass holds back. The baselines move either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quiet {
    /// Correction counts, tiers and streaks: held back on any bulk read, since old history would
    /// cross numbers that aren't news.
    pub counts: bool,
    /// The goal's weeks and report card weeks: held back only when the history under them may have
    /// changed (a source just allowed, scores rebuilt). A busy pass doesn't move a week that was
    /// already over, so a met goal is still announced.
    pub goal: bool,
}

impl Quiet {
    /// `quiet`: a source was just allowed or changed. `messages`: new messages this pass counted.
    pub fn for_pass(quiet: bool, rescored: bool, messages: usize) -> Quiet {
        Quiet { counts: quiet || rescored || messages > BACKFILL_MESSAGES, goal: quiet || rescored }
    }
}

/// The popups to show, in order. Nothing the first time (no baseline to measure from).
fn due(
    prev: Option<&Snapshot>,
    now: &Snapshot,
    goal: &GoalNews,
    cards: Vec<Milestone>,
    quiet: Quiet,
) -> Vec<Milestone> {
    let Some(prev) = prev else { return vec![] };
    let mut out = if quiet.counts { vec![] } else { crossed(prev, now) };
    if !quiet.goal {
        out.extend(goal_milestones(&goal.newly_met, goal.met, goal.finished.is_some()));
        out.extend(cards);
    }
    out
}

pub fn check(handle: &AppHandle, app: &App, quiet: Quiet) {
    let store = lock(&app.writer);
    let Ok(stats) = store.stats(&store::Filter::default(), Local::now().date_naive()) else { return };
    let now = Snapshot::of(&stats);
    let prev = store.advance_milestones(&now);
    // Like the goal's weeks, the report card baseline moves even on a quiet pass.
    let cards = store.advance_card_milestones(Local::now().date_naive(), !quiet.goal);
    // Freeze the goal's finished weeks even on a quiet pass, so they're never re-judged later.
    let goal = store.close_goal_weeks(Local::now().date_naive());
    drop(store);
    let goal = goal.unwrap_or_else(|e| {
        log::warn!("couldn't close the goal's weeks: {e}");
        GoalNews::default()
    });
    let cards = cards.unwrap_or_else(|e| {
        log::warn!("couldn't save the report card milestone baseline: {e}");
        Vec::new()
    });
    let prev = match prev {
        Ok(prev) => prev,
        Err(e) => {
            log::warn!("couldn't save the milestone baseline: {e}");
            return;
        }
    };
    for m in due(prev.as_ref(), &now, &goal, cards, quiet) {
        crate::notify(handle, app, m.title, m.body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::Tier;

    fn snap(corrections: i64) -> Snapshot {
        Snapshot {
            corrections,
            corrections_last_7d: 0,
            tier: Some(Tier::Coworker),
            score: 80,
            streak: 0,
            prompt_tier: None,
            landed_pct: 0,
        }
    }

    fn card() -> Milestone {
        Milestone { title: "Straight A's!".into(), body: "x".into() }
    }

    fn done_goal() -> GoalNews {
        GoalNews {
            newly_met: vec![],
            met: 4,
            finished: Some(store::Goal {
                id: 1,
                kind: store::GoalKind::Tier { tier: Tier::Coworker },
                target: 0,
                start: chrono::NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            }),
        }
    }

    fn titles(v: &[Milestone]) -> Vec<&str> {
        v.iter().map(|m| m.title.as_str()).collect()
    }

    #[test]
    fn only_a_source_change_or_a_rebuild_holds_back_the_goal() {
        assert_eq!(Quiet::for_pass(false, false, 3), Quiet { counts: false, goal: false });
        assert_eq!(Quiet::for_pass(true, false, 3), Quiet { counts: true, goal: true });
        assert_eq!(Quiet::for_pass(false, true, 3), Quiet { counts: true, goal: true });
    }

    #[test]
    fn a_busy_pass_holds_back_counts_but_not_the_goal() {
        let q = Quiet::for_pass(false, false, BACKFILL_MESSAGES + 1);
        assert_eq!(q, Quiet { counts: true, goal: false });
        assert!(!Quiet::for_pass(false, false, BACKFILL_MESSAGES).counts, "40 is still a normal pass");
        let out = due(Some(&snap(0)), &snap(10), &done_goal(), vec![card()], q);
        // No correction-count popup, but the goal still shows.
        assert_eq!(titles(&out), ["Goal done! 🏆", "Straight A's!"]);
    }

    #[test]
    fn a_normal_pass_shows_counts_then_the_goal_then_cards() {
        let out = due(Some(&snap(0)), &snap(1), &done_goal(), vec![card()], Quiet::for_pass(false, false, 2));
        assert_eq!(titles(&out), ["Your 1st correction!", "Goal done! 🏆", "Straight A's!"]);
    }

    #[test]
    fn a_quiet_pass_shows_nothing() {
        let out = due(Some(&snap(0)), &snap(10), &done_goal(), vec![card()], Quiet::for_pass(true, false, 2));
        assert_eq!(out.len(), 0);
    }

    #[test]
    fn the_first_run_shows_nothing() {
        let out = due(None, &snap(10), &done_goal(), vec![card()], Quiet::for_pass(false, false, 2));
        assert!(out.is_empty(), "there's no baseline to measure from");
    }
}
