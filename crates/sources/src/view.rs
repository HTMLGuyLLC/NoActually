//! The chat view: every prompt in one file with what it cost, for the digest and the Stop hook.

use crate::pair::holds_card;
use crate::{pair, CardSeen, Event};
use chrono::{DateTime, Utc};
use engine::{classify, message_key, read_prompt, short_id, Prompt, Usage};

/// A round that ran longer than this counts as this many minutes (the person went away).
const MAX_MINUTES: i64 = 30;

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
    /// How it reads: a task, steering or neither. Whether the chat has a task yet counts only
    /// earlier prompts in this file.
    pub prompt: Prompt,
    /// The raw model id that answered this round, if any.
    pub model: Option<String>,
    /// Tokens the AI used from this prompt until the next one.
    pub usage: Usage,
    /// Whole minutes from this prompt to the next (or to the round's last record), at most 30.
    pub minutes: u32,
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
/// everything after a cut (given, or a reply holding a card) until the next prompt.
pub(crate) fn build(
    source: &'static str,
    session: &str,
    events: Vec<Event>,
    samples: Vec<Sample>,
    mut cuts: Vec<Cut>,
    end: u64,
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
    let mut has_task = false;
    let mut prompts = Vec::with_capacity(scan.turns.len());
    for (i, st) in scan.turns.iter().enumerate() {
        let key = message_key(source, &st.session, st.key.as_deref(), &st.ts.to_rfc3339(), &st.turn.human);
        let prompt = read_prompt(&st.turn, &classify(&st.turn), has_task);
        has_task |= matches!(prompt, Prompt::Task { .. });
        let r = &rounds[i];
        let stop = match cut_in[i] {
            Some(c) => c.ts.or(r.last),
            None => scan.turns.get(i + 1).map(|n| n.ts).or(r.last),
        };
        let stop = stop.unwrap_or(st.ts);
        let minutes = (stop - st.ts).num_minutes().clamp(0, MAX_MINUTES);
        prompts.push(PromptView {
            id: short_id(&key).to_string(),
            key,
            ts: st.ts,
            text: st.turn.human.clone(),
            prompt,
            model: r.model.clone(),
            usage: r.usage,
            minutes: u32::try_from(minutes).unwrap_or(0),
        });
    }
    ChatView { source, session: session.to_string(), prompts, cards: scan.cards }
}
