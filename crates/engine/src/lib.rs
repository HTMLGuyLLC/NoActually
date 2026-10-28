//! Deterministic catch detection. Pure functions only: no I/O, no clock, no network,
//! so the same history always produces the same score (and this crate builds for WASM).

mod model;
mod redact;
mod rules;

pub use model::{normalize_model, Model};
pub use redact::{redact, snippet};

use serde::{Deserialize, Serialize};

/// One AI message: its text and the raw model id that wrote it, when the source records one.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiMsg {
    pub text: String,
    pub model: Option<String>,
}

/// One human message with the AI reply before it and the AI reply after it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Turn {
    pub human: String,
    pub prev_ai: Option<AiMsg>,
    pub next_ai: Option<AiMsg>,
    /// The model this chat was using nearby, for when neither neighboring reply names one
    /// (a first message that was never answered, or replies the tool didn't tag).
    #[serde(default)]
    pub chat_model: Option<String>,
}

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
    pub fn is_catch(self) -> bool {
        !matches!(self, Label::Humbled)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Label::Confirmed => "confirmed",
            Label::Admitted => "admitted",
            Label::Claimed => "claimed",
            Label::Humbled => "humbled",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "confirmed" => Label::Confirmed,
            "admitted" => Label::Admitted,
            "claimed" => Label::Claimed,
            "humbled" => Label::Humbled,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    MadeItUp,
    IgnoredInstructions,
    Misread,
    MissedSomething,
    StillBroken,
    JustWrong,
}

impl Category {
    pub const ALL: [Category; 6] = [
        Category::MadeItUp,
        Category::IgnoredInstructions,
        Category::Misread,
        Category::MissedSomething,
        Category::StillBroken,
        Category::JustWrong,
    ];
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
    pub label: Option<Label>,
    pub category: Option<Category>,
    pub rules: Vec<&'static str>,
    pub human_span: Option<(usize, usize)>,
    pub ai_span: Option<(usize, usize)>,
}

impl Verdict {
    /// The model to charge: the reply you corrected for a catch, the reply that corrected you
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
pub const RULES_VERSION: u32 = 2;

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
        // The AI's admission often names the mistake, so categories look at both sides.
        let both = format!("{body}\n{next}");
        v.category = Some(rules::category(&both));
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

pub(crate) fn head_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

fn tail_chars(s: &str, n: usize) -> &str {
    let count = s.chars().count();
    if count <= n {
        return s;
    }
    let (i, _) = s.char_indices().nth(count - n).unwrap();
    &s[i..]
}

#[cfg(test)]
mod tests;
