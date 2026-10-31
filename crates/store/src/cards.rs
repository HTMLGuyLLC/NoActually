//! Report cards in the database. A card's record block only names prompts and says which
//! follow-ups were avoidable; everything counted here (rounds, grade, tokens, minutes) comes from
//! the transcript, and only numbers are kept. The merge rule and the table are in
//! `docs/plans/prompt-feedback.md`.

use crate::stats::View;
use crate::{hash, Result, SourceState, NO_MODEL};
use chrono::{DateTime, Local, NaiveDate, SecondsFormat, Utc};
use engine::{normalize_model, price, tasks_overlap, LetterGrade, Model, Usage};
use rusqlite::{params, Connection};
use sources::{CardSeen, ChatView, SealedTask};
use std::collections::BTreeMap;

/// Ids of the sources whose chats can carry a report card (`Source::gradable`).
pub(crate) fn gradable_ids() -> Vec<&'static str> {
    sources::all().iter().filter(|s| s.gradable()).map(|s| s.id()).collect()
}

/// Setting: when the report card history was last deleted. Cards graded before it are never taken
/// back, even when a rebuild reads the same transcripts again.
pub(crate) const CLEARED_KEY: &str = "report_cards.cleared";

/// The tokens a task redid, by the raw id of the model that answered each round (`""` for a round
/// with no model recorded). Prices apply per model when it is read, so a newer table re-prices old
/// tokens, and a task that changed models is priced round by round.
pub(crate) type Tokens = BTreeMap<String, Usage>;

/// A time as stored. A fixed shape, so comparing the text compares the times.
pub(crate) fn ts_str(ts: DateTime<Utc>) -> String {
    ts.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// The local day of a stored time.
pub(crate) fn local_day(stored: &str) -> Option<NaiveDate> {
    let ts: DateTime<Utc> = stored.parse().ok()?;
    Some(ts.with_timezone(&Local).date_naive())
}

/// One graded task, as stored.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CardRow {
    key: String,
    card: String,
    source: String,
    session: String,
    /// The task's first prompt key: the same in every copy of a chat.
    first: String,
    day: String,
    start_ts: String,
    end_ts: String,
    provider: String,
    family: String,
    version: String,
    grade: LetterGrade,
    rounds: i64,
    avoidable: i64,
    missing: u8,
    tokens: Tokens,
    minutes: i64,
    graded_at: String,
}

/// True when `seen` holds a block past `read_to`, the last position a pass already read and
/// checked. Only then is the whole chat needed: a block at or before it is stored already, and
/// a pass that resumes at the reply before an unanswered message shows it again.
pub(crate) fn unread(seen: &[CardSeen], read_to: Option<u64>) -> bool {
    first_unread(seen, read_to).is_some()
}

/// Where the first block in `seen` past `read_to` starts, if there is one (see [`unread`]).
pub(crate) fn first_unread(seen: &[CardSeen], read_to: Option<u64>) -> Option<u64> {
    seen.iter().map(|c| c.pos).filter(|&pos| read_to.is_none_or(|to| pos > to)).min()
}

/// What a file's record blocks came to.
#[derive(Debug, Default)]
pub(crate) struct Cards {
    pub rows: Vec<CardRow>,
    /// Blocks and tasks left out, by reason. Reasons only, never content.
    pub(crate) dropped: BTreeMap<&'static str, usize>,
    /// Blocks were seen that couldn't be checked at all (no secret to check the seal with, or no
    /// chat to check them against). They are not "read": the position they were at must not be
    /// remembered, so a later pass looks at them again.
    pub unchecked: bool,
}

impl Cards {
    pub fn skip(&mut self, why: &'static str, n: usize) {
        *self.dropped.entry(why).or_insert(0) += n;
    }

    /// Cards that should have counted and didn't. Ones from before the history was deleted are meant
    /// to stay gone.
    pub fn left_out(&self) -> i64 {
        self.dropped.iter().filter(|(why, _)| **why != "deleted").map(|(_, n)| *n as i64).sum()
    }

    /// Says how many were left out and why, and in which chat (`chat` is its file).
    pub fn log(&self, source: &str, chat: &str) {
        if self.dropped.is_empty() {
            return;
        }
        let why: Vec<String> = self.dropped.iter().map(|(why, n)| format!("{n} {why}")).collect();
        log::warn!("report cards ({source}): left out {} in {chat}", why.join(", "));
    }
}

/// What every task of one block shares.
struct Block {
    key: String,
    graded_at: String,
}

/// The tasks of the record blocks in `view` that check out (see `ChatView::sealed_spans`), as rows.
/// The task's first prompt must be one the source's consent allows. `cleared` is when the history
/// was last deleted: nothing graded before it comes back.
pub(crate) fn read(state: &SourceState, view: &ChatView, secret: &[u8], cleared: Option<&str>) -> Cards {
    let mut out = Cards::default();
    let sealed = view.sealed_spans(secret);
    for (why, n) in sealed.left_out {
        out.skip(why, n);
    }
    // A block's tasks come together, in block order.
    for tasks in sealed.tasks.chunk_by(|a, b| a.card == b.card) {
        let seen = &view.cards[tasks[0].card];
        // A card with no time was written after the work it grades: its latest task's last prompt.
        let Some(at) = seen.ts.or_else(|| tasks.iter().map(|t| view.prompts[t.to].ts).max()) else { continue };
        let graded_at = ts_str(at);
        if cleared.is_some_and(|t| graded_at.as_str() <= t) {
            out.skip("deleted", tasks.len());
            continue;
        }
        let key = seen.key.clone().unwrap_or_else(|| hash(&[&state.id, &seen.session, &seen.pos.to_string()]));
        let block = Block { key, graded_at };
        // The spans (first prompt, last prompt) of this card's tasks kept so far. One card can't grade
        // the same work twice, so a task that overlaps an earlier one, by the store's rule, is left out.
        let mut kept: Vec<(usize, usize)> = Vec::new();
        for task in tasks {
            let span = (task.at, task.to);
            if kept.iter().any(|k| tasks_overlap(*k, span, k.0 == span.0)) {
                out.skip("overlaps another task in this card", 1);
                continue;
            }
            match row(state, view, &block, task) {
                Ok(Some(row)) => {
                    kept.push(span);
                    out.rows.push(row);
                }
                Ok(None) => {} // before the cutoff the person chose: not an error
                Err(why) => out.skip(why, 1),
            }
        }
    }
    out
}

/// The row for one task, `Ok(None)` when the person's consent skips its first prompt.
fn row(state: &SourceState, view: &ChatView, block: &Block, st: &SealedTask) -> Result<Option<CardRow>, &'static str> {
    let (at, to) = (st.at, st.to);
    // Each follow-up must be a later prompt of the task, and the task is cut at its last prompt.
    let mut follow_ups = Vec::new();
    for w in &st.task.waste {
        let i = view.index_of(&w.id).ok_or("unknown id")?;
        if i <= at || i > to {
            return Err("bad range");
        }
        if w.share > 0 {
            follow_ups.push((i, w.share));
        }
    }
    let prompts = &view.prompts;
    let first = &prompts[at];
    if !state.wants(first.ts) {
        return Ok(None);
    }
    // Each follow-up redid a share of the work before it, up to but not counting its own round. The
    // share of a model's rounds is taken once per follow-up, so a task that changed models keeps
    // each round's tokens under the model that answered it. Time is the shared seconds, summed over
    // the follow-ups and rounded to a minute once, at the end.
    let mut tokens = Tokens::new();
    let mut shared_seconds = 0u64;
    for (i, share) in follow_ups {
        let before = &prompts[at..i];
        let mut work: BTreeMap<&str, Usage> = BTreeMap::new();
        for p in before {
            work.entry(p.model.as_deref().unwrap_or_default()).or_default().add(&p.usage);
        }
        for (model, usage) in work {
            let part = usage.share(share);
            if part != Usage::default() {
                tokens.entry(model.to_string()).or_default().add(&part);
            }
        }
        shared_seconds += before.iter().map(|p| u64::from(p.seconds)).sum::<u64>() * u64::from(share) / 100;
    }
    let model = first.model.as_deref().and_then(normalize_model);
    let (provider, family, version) = model
        .as_ref()
        .map_or((NO_MODEL, NO_MODEL, ""), |m| (m.provider.as_str(), m.family.as_str(), m.version.as_str()));
    let source = state.id.as_str();
    Ok(Some(CardRow {
        key: hash(&[&block.key, &first.key]),
        card: hash(&[&block.key]),
        source: source.to_string(),
        session: hash(&[source, &view.session]),
        first: first.key.clone(),
        day: first.ts.with_timezone(&Local).date_naive().to_string(),
        start_ts: ts_str(first.ts),
        end_ts: ts_str(prompts[to].ts),
        provider: provider.to_string(),
        family: family.to_string(),
        version: version.to_string(),
        grade: st.task.grade(),
        rounds: view.rounds(at..=to) as i64,
        avoidable: st.task.avoidable() as i64,
        missing: st.task.missing,
        tokens,
        minutes: ((shared_seconds + 30) / 60) as i64,
        graded_at: block.graded_at.clone(),
    }))
}

/// Writes rows in the caller's transaction. A row for the same card and task is replaced, so
/// reading a card again (a re-sync, a rebuild, a rewound checkpoint) changes nothing.
pub(crate) fn save(conn: &Connection, rows: &[CardRow]) -> Result<()> {
    let mut stmt = conn.prepare_cached(
        "INSERT OR REPLACE INTO report_cards (key, card, source, session, first, day, start_ts, end_ts, provider, family,
            version, grade, rounds, avoidable, missing, auto, tokens, minutes, graded_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,0,?16,?17,?18)",
    )?;
    for r in rows {
        stmt.execute(params![
            r.key,
            r.card,
            r.source,
            r.session,
            r.first,
            r.day,
            r.start_ts,
            r.end_ts,
            r.provider,
            r.family,
            r.version,
            r.grade.as_str(),
            r.rounds,
            r.avoidable,
            r.missing,
            serde_json::to_string(&r.tokens)?,
            r.minutes,
            r.graded_at,
        ])?;
    }
    Ok(())
}

/// A report card that still counts (no newer card overlaps it), as the read side needs it.
pub(crate) struct Current {
    pub day: String,
    pub source: String,
    pub provider: String,
    pub family: String,
    pub version: String,
    pub grade: LetterGrade,
    pub avoidable: i64,
    pub missing: u8,
    /// The reworked tokens, by model.
    pub tokens: Tokens,
    pub minutes: i64,
    pub graded_at: String,
}

impl Current {
    /// Tokens redone: new input, cache writes and output, over every model.
    pub fn shown_tokens(&self) -> u64 {
        self.tokens.values().map(Usage::shown).sum()
    }

    /// What the reworked tokens cost at list prices: each model's tokens at its own price, added up.
    /// `None` unless every part has a price, so a task is either fully priced or left out of the
    /// cost (and of the count of priced tasks), never half counted. A task that redid nothing is
    /// priced, at nothing extra, when its own model has a price.
    pub fn cost(&self) -> Option<f64> {
        if self.tokens.is_empty() {
            let model = Model {
                provider: self.provider.clone(),
                family: self.family.clone(),
                version: self.version.clone(),
                raw: String::new(),
            };
            return price(&model).map(|_| 0.0);
        }
        self.tokens.iter().map(|(model, usage)| usage.cost_on(model)).sum()
    }
}

/// The report cards that still count under `v`'s filter, in the order `tail` gives (`ORDER BY …`,
/// and `LIMIT …` to read only some).
pub(crate) fn current(v: &View, tail: &str) -> Result<Vec<Current>> {
    let sql = format!(
        "SELECT day, source, provider, family, version, grade, avoidable, missing, tokens, minutes, graded_at
         FROM current_report_cards WHERE 1=1 {} {tail}",
        v.clause
    );
    // A grade this build doesn't know (written by a newer one) can't be counted: that card is left out.
    let rows = v.rows(&sql, &[], |r| {
        let Some(grade) = LetterGrade::from_id(&r.get::<_, String>(5)?) else { return Ok(None) };
        Ok(Some(Current {
            day: r.get(0)?,
            source: r.get(1)?,
            provider: r.get(2)?,
            family: r.get(3)?,
            version: r.get(4)?,
            grade,
            avoidable: r.get(6)?,
            missing: r.get(7)?,
            tokens: serde_json::from_str(&r.get::<_, String>(8)?).unwrap_or_default(),
            minutes: r.get(9)?,
            graded_at: r.get(10)?,
        }))
    })?;
    Ok(rows.into_iter().flatten().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn current_with(tokens: &[(&str, Usage)], family: &str, version: &str) -> Current {
        Current {
            day: "2026-10-05".into(),
            source: "claude-code".into(),
            provider: "Anthropic".into(),
            family: family.into(),
            version: version.into(),
            grade: LetterGrade::C,
            avoidable: 2,
            missing: 0,
            tokens: tokens.iter().map(|(m, u)| (m.to_string(), *u)).collect(),
            minutes: 3,
            graded_at: "2026-10-05T12:40:00.000Z".into(),
        }
    }

    const USED: Usage = Usage { input: 1_000, cache_write: 2_000, cache_read: 100_000, output: 3_000 };

    #[test]
    fn cost_adds_each_models_tokens_at_its_own_price() {
        // 0.094 on Opus 5.5 (see engine::pricing), and the same tokens on Sonnet 5.5 at half the price.
        let opus = USED.cost_on("claude-opus-5-5").unwrap();
        let sonnet = USED.cost_on("claude-sonnet-5-5").unwrap();
        assert!(sonnet < opus);
        let both = current_with(&[("claude-opus-5-5", USED), ("claude-sonnet-5-5", USED)], "Claude Opus", "5.5");
        assert!((both.cost().unwrap() - (opus + sonnet)).abs() < 1e-12);
        assert_eq!(both.shown_tokens(), 2 * USED.shown());
    }

    #[test]
    fn a_task_is_priced_only_when_every_part_is() {
        let mixed = current_with(&[("claude-opus-5-5", USED), ("gpt-5.1", USED)], "Claude Opus", "5.5");
        assert_eq!(mixed.cost(), None, "one unpriced part leaves the whole task out of the cost");
        let nameless = current_with(&[("", USED)], "Claude Opus", "5.5");
        assert_eq!(nameless.cost(), None, "tokens with no model recorded can't be priced");
        let unpriced = current_with(&[("gpt-5.1", USED)], "GPT", "5.1");
        assert_eq!(unpriced.cost(), None);
    }

    #[test]
    fn a_task_that_redid_nothing_is_priced_at_nothing_when_its_model_has_a_price() {
        assert_eq!(current_with(&[], "Claude Opus", "5.5").cost(), Some(0.0));
        assert_eq!(current_with(&[], "GPT", "5.1").cost(), None, "and unpriced when its model is");
    }

    #[test]
    fn stored_times_sort_like_times() {
        let t = |s: &str| ts_str(s.parse().unwrap());
        assert_eq!(t("2026-10-05T10:00:00Z"), "2026-10-05T10:00:00.000Z");
        assert!(t("2026-10-05T10:00:00Z") < t("2026-10-05T10:00:00.5Z"));
        assert!(t("2026-10-05T10:00:59Z") < t("2026-10-05T10:01:00Z"));
    }
}
