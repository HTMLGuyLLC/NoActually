//! Deterministic correction detection. Pure functions only: no I/O, no clock, no network,
//! so the same history always produces the same score (and this crate builds for WASM).
//!
//! [`classify`] labels one [`Turn`]; [`normalize_model`] splits a raw model id into
//! provider / family / version; [`redact`] and [`snippet`] decide what text may be kept;
//! [`Tier`] and [`Answer`] turn a week's corrections into the headline. [`read_prompt`] reads the
//! same turn for how you prompt: a new task, steering, or neither, and a prompt's [`Signals`].
//!
//! Tests: each module keeps its unit tests inline in `mod tests`; the rule book's behavior
//! suite lives in `src/tests.rs` because it exercises the whole crate.

#![warn(missing_docs)]

mod model;
mod prompt;
mod redact;
mod rules;
mod tier;

pub use model::{normalize_model, Model};
pub use prompt::{
    check, read_prompt, Check, Grade, Prompt, PromptTier, Signal, Signals, Steer, MIN_PROMPTS, WINDOW_DAYS,
};
pub use redact::{redact, snippet, SNIPPET_CHARS};
pub use tier::{
    Answer, CorrectionTier, Score, Tier, UnknownTier, Window, CORRECTION_PROOF, DIRECTION_WEIGHT, HUMBLED_BASE,
    HUMBLED_COST,
};

use serde::{Deserialize, Serialize};

/// One AI message: its text and the raw model id that wrote it, when the source records one.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiMsg {
    /// What the AI wrote (sources may keep only the part the rules read).
    pub text: String,
    /// The raw model id, exactly as the tool recorded it.
    pub model: Option<String>,
}

/// One human message with the AI reply before it and the AI reply after it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Turn {
    /// What the person typed.
    pub human: String,
    /// The AI reply the person was answering.
    pub prev_ai: Option<AiMsg>,
    /// The AI reply that came next.
    pub next_ai: Option<AiMsg>,
    /// The model this chat was using nearby, for when neither neighboring reply names one
    /// (a first message that was never answered, or replies the tool didn't tag).
    #[serde(default)]
    pub chat_model: Option<String>,
}

/// What a turn was: one of three kinds of correction, or the AI catching you.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Label {
    /// You pushed back and the AI admitted the mistake.
    Confirmed,
    /// The AI admitted a mistake.
    Admitted,
    /// You pushed back; the AI didn't admit it.
    Claimed,
    /// The AI corrected you.
    Humbled,
}

impl Label {
    /// Every label.
    pub const ALL: [Label; 4] = [Label::Confirmed, Label::Admitted, Label::Claimed, Label::Humbled];

    /// True for the labels that count toward your score (everything but `Humbled`).
    pub fn is_correction(self) -> bool {
        !matches!(self, Label::Humbled)
    }

    /// Stable id, as stored and serialized.
    pub fn as_str(self) -> &'static str {
        match self {
            Label::Confirmed => "confirmed",
            Label::Admitted => "admitted",
            Label::Claimed => "claimed",
            Label::Humbled => "humbled",
        }
    }
}

/// What kind of mistake a correction was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Invented something: a page, function or fact that doesn't exist.
    MadeItUp,
    /// Did something other than what you asked.
    IgnoredInstructions,
    /// Misunderstood what you meant.
    Misread,
    /// Left something out.
    MissedSomething,
    /// Said it was fixed; it wasn't.
    StillBroken,
    /// Anything else.
    JustWrong,
}

impl Category {
    /// Every category, in display order.
    pub const ALL: [Category; 6] = [
        Category::MadeItUp,
        Category::IgnoredInstructions,
        Category::Misread,
        Category::MissedSomething,
        Category::StillBroken,
        Category::JustWrong,
    ];

    /// The category with this id, if any.
    pub fn from_id(id: &str) -> Option<Category> {
        Category::ALL.into_iter().find(|c| c.as_str() == id)
    }

    /// Stable id, as stored and serialized.
    pub fn as_str(self) -> &'static str {
        match self {
            Category::MadeItUp => "made_it_up",
            Category::IgnoredInstructions => "ignored_instructions",
            Category::Misread => "misread",
            Category::MissedSomething => "missed_something",
            Category::StillBroken => "still_broken",
            Category::JustWrong => "just_wrong",
        }
    }

    /// Display title ("Made it up").
    pub fn title(self) -> &'static str {
        match self {
            Category::MadeItUp => "Made it up",
            Category::IgnoredInstructions => "Ignored instructions",
            Category::Misread => "Misread you",
            Category::MissedSomething => "Missed something",
            Category::StillBroken => "Still broken",
            Category::JustWrong => "Flat-out wrong",
        }
    }
}

/// The result of classifying a turn. `rules` names every rule that fired, so a label can
/// always be explained; `human_span` / `ai_span` are the byte ranges that matched.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Verdict {
    /// `None` for an ordinary message.
    pub label: Option<Label>,
    /// Set for corrections only.
    pub category: Option<Category>,
    /// Ids of the rules that fired.
    pub rules: Vec<&'static str>,
    /// Byte range in the normalized, trimmed human text.
    pub human_span: Option<(usize, usize)>,
    /// Byte range in the normalized next AI reply.
    pub ai_span: Option<(usize, usize)>,
}

impl Verdict {
    /// The model to charge: the reply you corrected for a correction, the reply that corrected you
    /// for a humbled event.
    pub fn blamed_model<'a>(&self, turn: &'a Turn) -> Option<&'a str> {
        let real = |m: Option<&'a AiMsg>| m.and_then(|m| m.model.as_deref()).filter(|id| is_real_model(id));
        let (prev, next) = (real(turn.prev_ai.as_ref()), real(turn.next_ai.as_ref()));
        let pick = match self.label {
            Some(Label::Humbled) => next.or(prev),
            _ => prev.or(next),
        };
        pick.or(turn.chat_model.as_deref().filter(|id| is_real_model(id)))
    }
}

/// Bump whenever a change would label or attribute existing history differently; stores
/// rebuild their scores from the history files when it changes.
/// 2: placeholder replies are skipped and messages fall back to the chat's model.
/// 3: approvals ("No, that's fine") aren't pushback; the AI's admission no longer picks the category.
/// 4: prompts and steering are counted (see [`read_prompt`]), so history is re-read to fill them in.
/// 5: "you'll know it's fixed when…" says what done is; "in the logs" alone no longer shows proof;
///    "we want them to come back" says why.
/// 6: rule ids say prompt, not brief (history is re-read so stored ids match).
pub const RULES_VERSION: u32 = 6;

/// Tools write placeholder ids for replies no model wrote (Claude Code's "<synthetic>" for errors,
/// interruptions and "No response requested").
pub fn is_real_model(id: &str) -> bool {
    let id = id.trim();
    !id.is_empty() && !id.starts_with('<')
}

const HEAD: usize = 300; // chars of the human message checked for openers
const BODY: usize = 1500; // chars of the human message checked anywhere
const AI_HEAD: usize = 250; // chars of the next AI reply checked
const PASTE: usize = 2500; // longer human messages are pasted docs, not corrections
const ANSWER_TAIL: usize = 300; // a '?' this close to the end of the AI reply means you're answering it

/// Labels one turn. Same input, same verdict, always.
pub fn classify(turn: &Turn) -> Verdict {
    let human_norm = normalize_quotes(turn.human.trim());
    let t = human_norm.as_str();
    if t.is_empty() || t.starts_with('<') {
        return Verdict::default(); // tool / agent payloads, not typed by a person
    }
    let next_norm = normalize_quotes(turn.next_ai.as_ref().map_or("", |m| m.text.as_str()));
    let next = head_chars(&next_norm, AI_HEAD);
    let prev = turn.prev_ai.as_ref().map_or("", |m| m.text.as_str());

    let head = head_chars(t, HEAD);
    let body = head_chars(t, BODY);
    let pasted = t.chars().count() > PASTE;

    let ai_admit = rules::first(&rules::AI_ADMIT, next);
    let user_open = if pasted { None } else { rules::first(&rules::USER_OPEN, head) };
    let user_any = if pasted { None } else { rules::first(&rules::USER_ANY, body) };
    let context = rules::first(&rules::CONTEXT, head);
    let answering = tail_chars(prev, ANSWER_TAIL).contains('?');

    let label = match (ai_admit, user_open.or(user_any)) {
        (Some(_), Some(_)) => Some(Label::Confirmed),
        (Some(_), None) if context.is_none() => Some(Label::Admitted),
        (None, _) if user_open.is_some() && context.is_none() && !answering => Some(Label::Claimed),
        _ => None,
    };

    if let Some(label) = label {
        let mut v = Verdict { label: Some(label), ..Default::default() };
        if let Some(m) = ai_admit {
            v.rules.push(m.rule);
            v.ai_span = Some(m.span);
        }
        if let Some(m) = user_open.or(user_any) {
            v.rules.push(m.rule);
            v.human_span = Some(m.span);
        }
        // The rest of the AI's reply often names the mistake, so categories look at both sides.
        v.category = Some(rules::category(body, next, ai_admit.map(|m| m.span)));
        return v;
    }

    let status_question = t.chars().count() < 60 && rules::STATUS_Q.is_match(t);
    if !status_question {
        if let Some(m) = rules::first(&rules::AI_PUSHBACK, next) {
            return Verdict {
                label: Some(Label::Humbled),
                rules: vec![m.rule],
                ai_span: Some(m.span),
                ..Default::default()
            };
        }
    }
    Verdict::default()
}

/// Curly quotes become straight ones so "you’re right" and "you're right" match the same rule.
/// `Verdict` spans index into this normalized text (human side: after `trim`).
pub fn normalize_quotes(s: &str) -> String {
    s.replace(['\u{2018}', '\u{2019}'], "'").replace(['\u{201C}', '\u{201D}'], "\"")
}

/// The first `n` chars of `s` (not bytes, so multibyte text never splits).
pub fn head_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// The last `n` chars of `s`.
pub fn tail_chars(s: &str, n: usize) -> &str {
    let count = s.chars().count();
    if count <= n {
        return s;
    }
    s.char_indices().nth(count - n).map_or(s, |(i, _)| &s[i..])
}

#[cfg(test)]
mod tests;
