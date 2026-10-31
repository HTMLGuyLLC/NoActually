//! The chat view: every prompt in one file with what it cost, for the digest and the Stop hook.

use crate::pair::holds_card;
use crate::{pair, CardSeen, Event};
use chrono::{DateTime, Utc};
use engine::{
    classify, is_signoff, message_key, parse_card, read_prompt, seal, short_id, AiMsg, CardError, CardTask, Prompt,
    Turn, Usage,
};
use std::collections::{BTreeMap, HashMap};
use std::slice::SliceIndex;

/// A round that ran longer than this counts as this many seconds (the person went away).
const MAX_SECONDS: i64 = 30 * 60;

/// One chat file, read whole.
#[derive(Debug, Clone)]
pub struct ChatView {
    /// The adapter's id.
    pub source: &'static str,
    /// The chat, as the tool names it.
    pub session: String,
    /// Every message the person sent, in order.
    pub prompts: Vec<PromptView>,
    /// Record blocks found in the AI's replies.
    pub cards: Vec<CardSeen>,
    /// Each prompt's short id and where it is. The first of equal ids wins.
    index: HashMap<String, usize>,
}

/// A task of a record block that is this chat's, placed in it. See [`ChatView::sealed_spans`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedTask {
    /// The block it came from: a position in [`ChatView::cards`].
    pub card: usize,
    /// Position of the task's first prompt in [`ChatView::prompts`].
    pub at: usize,
    /// Position of its last prompt, never before `at`.
    pub to: usize,
    /// The task as the block wrote it. Its `waste` ids are not looked up yet: use [`ChatView::index_of`].
    pub task: CardTask,
}

/// What the record blocks of a chat came to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sealed {
    /// The tasks that check out: the block reads, the seal is this chat's, and the task's first and last
    /// prompt are in the chat, in order. In block order.
    pub tasks: Vec<SealedTask>,
    /// Blocks and tasks left out, by reason. Reasons only, never content, so they're safe to log.
    pub left_out: BTreeMap<&'static str, usize>,
}

impl Sealed {
    fn skip(&mut self, why: &'static str) {
        *self.left_out.entry(why).or_insert(0) += 1;
    }
}

/// Why a record block was left out, in words.
fn why(e: CardError) -> &'static str {
    match e {
        CardError::TooBig => "too big",
        CardError::BadJson => "not a record block",
        CardError::UnknownVersion => "unknown version",
        CardError::TaskCount => "wrong task count",
        CardError::BadId => "bad id",
        CardError::BadWaste => "bad share",
    }
}

impl ChatView {
    /// Prompts in `range` (positions in [`ChatView::prompts`]) that are rounds of work: all but
    /// sign-offs, which close a task without being part of it.
    pub fn rounds<R: SliceIndex<[PromptView], Output = [PromptView]>>(&self, range: R) -> usize {
        self.prompts[range].iter().filter(|p| !p.signoff).count()
    }

    /// The position of the prompt a record block names with this short id. If two share it, the first.
    pub fn index_of(&self, short_id: &str) -> Option<usize> {
        self.index.get(short_id).copied()
    }

    /// The tasks of every record block in this chat that can be trusted: the block parses as a card,
    /// its seal is the one `secret` makes for this chat (a block copied from another chat, a web page
    /// or the docs carries the wrong one), and the task's first and last prompt are in the chat, in
    /// that order. Whatever fails is counted in [`Sealed::left_out`].
    pub fn sealed_spans(&self, secret: &[u8]) -> Sealed {
        let mut out = Sealed::default();
        if self.cards.is_empty() {
            return out;
        }
        let want = seal(secret, &self.session);
        for (card, seen) in self.cards.iter().enumerate() {
            let parsed = match parse_card(&seen.body) {
                Ok(parsed) => parsed,
                Err(e) => {
                    out.skip(why(e));
                    continue;
                }
            };
            if parsed.seal != want {
                out.skip("bad seal");
                continue;
            }
            for task in parsed.tasks {
                let (Some(at), Some(to)) = (self.index_of(&task.at), self.index_of(&task.to)) else {
                    out.skip("unknown id");
                    continue;
                };
                if at > to {
                    out.skip("bad range");
                    continue;
                }
                out.tasks.push(SealedTask { card, at, to, task });
            }
        }
        out
    }
}

/// One message the person sent, and the round of AI work it started.
#[derive(Debug, Clone)]
pub struct PromptView {
    /// The message's key, exactly as the store computes it.
    pub key: String,
    /// The short form of `key` a report card names it by.
    pub id: String,
    /// When it was sent.
    pub ts: DateTime<Utc>,
    /// The whole message.
    pub text: String,
    /// The message is only a sign-off ("thanks", "lgtm"): it closes a task and is not a round of work.
    pub signoff: bool,
    /// How it reads: a task, steering or neither. Whether the chat has a task yet counts only
    /// earlier prompts in this file.
    pub prompt: Prompt,
    /// The raw model id that answered this round, if any.
    pub model: Option<String>,
    /// Tokens the AI used from this prompt until the next one.
    pub usage: Usage,
    /// Seconds from this prompt to the next (or to the round's last record), at most 1800 (30 minutes):
    /// a longer wait means the person went away.
    pub seconds: u32,
}

/// Tokens used (or a model seen) at one spot in a file. Adapters produce these; [`build`] files
/// each under the prompt before it.
#[derive(Debug, Clone)]
pub(crate) struct Sample {
    pub pos: u64,
    pub ts: Option<DateTime<Utc>>,
    pub model: Option<String>,
    /// Tokens used here alone, not a running total.
    pub usage: Usage,
}

/// Where report-card work starts. It ends the round before it; nothing from here to the next
/// real prompt is billed to anyone.
#[derive(Debug, Clone)]
pub(crate) struct Cut {
    pub pos: u64,
    pub ts: Option<DateTime<Utc>>,
}

/// What one round gathers: tokens, the answering model and the time of its last record.
#[derive(Clone, Default)]
struct Round {
    usage: Usage,
    model: Option<String>,
    last: Option<DateTime<Utc>>,
}

impl Round {
    fn see(&mut self, ts: Option<DateTime<Utc>>, model: Option<&String>) {
        self.last = self.last.max(ts);
        if let Some(m) = model.filter(|m| engine::is_real_model(m)) {
            self.model = Some(m.clone());
        }
    }
}

/// Builds the view from a file's events and token samples. `end` is the offset after the last
/// whole line. Samples before the first prompt belong to nobody and are dropped, and so is
/// everything after a cut (given, or a reply holding a card) until the next prompt. `mid_chat`:
/// the events start part way into the chat, so its first request has been made already and the
/// first prompt is read as any later one, though the reply before it is out of view.
pub(crate) fn build(
    source: &'static str,
    session: &str,
    events: Vec<Event>,
    samples: Vec<Sample>,
    mut cuts: Vec<Cut>,
    end: u64,
    mid_chat: bool,
) -> ChatView {
    let starts: Vec<u64> = events
        .iter()
        .filter_map(|e| match e {
            Event::Human { pos, .. } => Some(*pos),
            Event::Ai { .. } => None,
        })
        .collect();
    cuts.extend(events.iter().filter_map(|e| match e {
        Event::Ai { text, pos, ts, .. } if holds_card(text) => Some(Cut { pos: *pos, ts: *ts }),
        _ => None,
    }));
    cuts.sort_by_key(|c| c.pos);
    // The first cut inside each round: from there on, its records are nobody's.
    let cut_in: Vec<Option<&Cut>> = starts
        .iter()
        .enumerate()
        .map(|(i, start)| {
            let c = cuts.get(cuts.partition_point(|c| c.pos <= *start))?;
            starts.get(i + 1).is_none_or(|next| c.pos < *next).then_some(c)
        })
        .collect();
    let mut rounds = vec![Round::default(); starts.len()];
    // The prompt a record at `pos` belongs to: the last one before it.
    let round = |pos: u64| {
        let i = starts.partition_point(|p| *p <= pos).checked_sub(1)?;
        cut_in[i].is_none_or(|c| pos < c.pos).then_some(i)
    };
    for e in &events {
        if let Event::Ai { pos, ts, model, .. } = e {
            if let Some(i) = round(*pos) {
                rounds[i].see(*ts, model.as_ref());
            }
        }
    }
    for s in &samples {
        if let Some(i) = round(s.pos) {
            rounds[i].see(s.ts, s.model.as_ref());
            rounds[i].usage.add(&s.usage);
        }
    }

    // Every human event is a turn, so turns and rounds line up.
    let scan = pair(source, session, events, end, true);
    let mut has_task = mid_chat;
    let mut prompts = Vec::with_capacity(scan.turns.len());
    for (i, st) in scan.turns.iter().enumerate() {
        let key = message_key(source, &st.session, st.key.as_deref(), &st.ts.to_rfc3339(), &st.turn.human);
        let after_reply;
        let turn = if mid_chat && i == 0 && st.turn.prev_ai.is_none() {
            after_reply = Turn { prev_ai: Some(AiMsg::default()), ..st.turn.clone() };
            &after_reply
        } else {
            &st.turn
        };
        let prompt = read_prompt(turn, &classify(turn), has_task);
        has_task |= matches!(prompt, Prompt::Task { .. });
        let r = &rounds[i];
        let stop = match cut_in[i] {
            Some(c) => c.ts.or(r.last),
            None => scan.turns.get(i + 1).map(|n| n.ts).or(r.last),
        };
        let stop = stop.unwrap_or(st.ts);
        let seconds = (stop - st.ts).num_seconds().clamp(0, MAX_SECONDS);
        prompts.push(PromptView {
            id: short_id(&key).to_string(),
            key,
            ts: st.ts,
            signoff: is_signoff(&st.turn.human),
            text: st.turn.human.clone(),
            prompt,
            model: r.model.clone(),
            usage: r.usage,
            seconds: u32::try_from(seconds).unwrap_or(0),
        });
    }
    let mut index = HashMap::with_capacity(prompts.len());
    for (i, p) in prompts.iter().enumerate() {
        index.entry(p.id.clone()).or_insert(i);
    }
    ChatView { source, session: session.to_string(), prompts, cards: scan.cards, index }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude_code::ClaudeCode;
    use crate::testing::{cc_ai, cc_user};
    use crate::{Source, GRADE_COMMAND, GRADE_SKILL_CODEX};
    use serde_json::json;

    const TS: &str = "2026-10-01T10:00:00Z";
    const SECRET: &[u8] = b"secret";

    /// The short id a card uses for the prompt whose record has this uuid.
    fn id(uuid: &str) -> String {
        short_id(&message_key("claude-code", "abc", Some(uuid), "", "")).to_string()
    }

    fn card(seal: &str, tasks: &serde_json::Value) -> String {
        let block = json!({"v":1,"k":seal,"tasks":tasks});
        cc_ai("claude-opus-5-5", TS, &format!("Report card: B\n\n```noactually\n{block}\n```\n"))
    }

    fn view_of(body: &str) -> ChatView {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("abc.jsonl");
        std::fs::write(&p, body).unwrap();
        ClaudeCode::code().view(&p).unwrap().unwrap()
    }

    fn chat() -> String {
        cc_user("u1", TS, "build the settings page with tabs")
            + &cc_ai("claude-opus-5-5", TS, "Built it.")
            + &cc_user("u2", TS, "use tailwind for the styles")
            + &cc_ai("claude-opus-5-5", TS, "Restyled.")
            + &cc_user("u3", TS, "thanks")
            + &cc_ai("claude-opus-5-5", TS, "Welcome.")
    }

    #[test]
    fn a_sign_off_is_marked_once_and_is_not_a_round() {
        let v = view_of(&chat());
        let marks: Vec<_> = v.prompts.iter().map(|p| p.signoff).collect();
        assert_eq!(marks, [false, false, true]);
        assert_eq!((v.rounds(0..3), v.rounds(0..=2), v.rounds(1..2), v.rounds(2..3), v.rounds(0..0)), (2, 2, 1, 0, 0));
        let long = cc_user("u1", TS, &["thanks"; 40].join(" "));
        assert!(!view_of(&long).prompts[0].signoff, "over the cap it never is one");
    }

    #[test]
    fn a_prompt_is_found_by_its_short_id_and_the_first_of_equal_ids_wins() {
        let v = view_of(&(chat() + &cc_user("u2", TS, "same record id again")));
        assert_eq!(v.index_of(&id("u1")), Some(0));
        assert_eq!(v.index_of(&id("u3")), Some(2));
        assert_eq!(v.index_of(&id("u2")), Some(1), "a repeated record id points at its first prompt");
        assert_eq!(v.index_of("deadbeef"), None);
        assert_eq!(v.index_of(""), None);
    }

    #[test]
    fn only_tasks_with_this_chats_seal_and_known_ids_in_order_are_sealed() {
        let seal = engine::seal(SECRET, "abc");
        let good = json!([{"at":id("u1"),"to":id("u3"),"waste":[[id("u2"),50]]}]);
        let two = json!([{"at":id("u1"),"to":id("u1")},{"at":id("u2"),"to":"deadbeef"},{"at":id("u3"),"to":id("u2")}]);
        let body = chat()
            + &card(&seal, &good)
            + &card(&engine::seal(b"other", "abc"), &good) // another chat's block
            + &card(&seal, &two) // one task fine, one unknown id, one backwards
            + &cc_ai("claude-opus-5-5", TS, "```noactually\nnot json\n```\n");
        let v = view_of(&body);
        assert_eq!(v.cards.len(), 4);
        let sealed = v.sealed_spans(SECRET);
        let spans: Vec<_> = sealed.tasks.iter().map(|t| (t.card, t.at, t.to)).collect();
        assert_eq!(spans, [(0, 0, 2), (2, 0, 0)]);
        assert_eq!(sealed.tasks[0].task.waste[0].id, id("u2"), "the task comes through as written");
        let why: Vec<_> = sealed.left_out.iter().map(|(w, n)| (*w, *n)).collect();
        assert_eq!(why, [("bad range", 1), ("bad seal", 1), ("not a record block", 1), ("unknown id", 1)]);
        assert_eq!(v.sealed_spans(b"nobody's secret").tasks, vec![], "a different key trusts nothing");
        assert_eq!(view_of(&chat()).sealed_spans(SECRET), Sealed::default());
    }

    #[test]
    fn only_claude_code_and_codex_can_carry_a_card() {
        let all = crate::all();
        let gradable: Vec<_> = all.iter().filter(|s| s.gradable()).map(|s| s.id()).collect();
        assert_eq!(gradable, ["claude-code", "codex"], "Cowork can't be turned on");
    }

    #[test]
    fn the_grade_names_are_what_people_type() {
        assert_eq!(GRADE_COMMAND, "/grade:me");
        assert_eq!(GRADE_SKILL_CODEX, "$grade-me");
    }
}
