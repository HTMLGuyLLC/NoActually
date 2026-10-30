//! Report card records: the one-line block a report card ends with, so the app can keep its
//! numbers. The model only says which prompts were avoidable and what was missing; everything
//! counted (rounds, grade, tokens, cost) is worked out from the transcript, never taken from it.
//!
//! ````text
//! ```noactually
//! {"v":1,"k":"9c41e0aa","tasks":[{"at":"7f3a9c21","to":"e44a1b90","waste":[["b81e04d2",50]],"miss":["finish"]}]}
//! ```
//! ````

use crate::{key::hmac_sha256, Signal, SHORT_ID};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// The block version this engine reads.
pub const CARD_VERSION: u64 = 1;
/// Opens a record block. Also the cheap check before any parsing.
pub const CARD_FENCE: &str = "```noactually";
/// How much of the earlier work a follow-up made someone redo, in percent. Only these steps, so
/// the same chat gets the same answer every run.
pub const SHARES: [u8; 5] = [0, 25, 50, 75, 100];

const MAX_BYTES: usize = 4096;
const MAX_TASKS: usize = 20;
const MAX_WASTE: usize = 50;
/// The `missing` bit for Context: a fact about their setup or a preference.
pub const CONTEXT_BIT: u8 = 1 << 5;

/// A letter grade, from how many follow-ups made someone redo work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum LetterGrade {
    /// No extra back-and-forth.
    A,
    /// One.
    B,
    /// Two.
    C,
    /// Three.
    D,
    /// Four or more, or the goal was never said.
    F,
}

impl LetterGrade {
    /// Every grade, best first.
    pub const ALL: [LetterGrade; 5] = [LetterGrade::A, LetterGrade::B, LetterGrade::C, LetterGrade::D, LetterGrade::F];

    /// The grade for `avoidable` rounds; `wrong` (the goal was never said) is always an F.
    pub fn of(avoidable: usize, wrong: bool) -> LetterGrade {
        if wrong {
            return LetterGrade::F;
        }
        match avoidable {
            0 => LetterGrade::A,
            1 => LetterGrade::B,
            2 => LetterGrade::C,
            3 => LetterGrade::D,
            _ => LetterGrade::F,
        }
    }

    /// "A".."F", as stored.
    pub fn as_str(self) -> &'static str {
        match self {
            LetterGrade::A => "A",
            LetterGrade::B => "B",
            LetterGrade::C => "C",
            LetterGrade::D => "D",
            LetterGrade::F => "F",
        }
    }
}

/// A follow-up the person could have put in their first message, and how much work it redid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waste {
    /// The follow-up's short id.
    pub id: String,
    /// Percent of the task's work up to it that had to be redone: one of [`SHARES`].
    pub share: u8,
}

/// One graded task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardTask {
    /// Short id of the prompt that started it.
    pub at: String,
    /// Short id of its last prompt.
    pub to: String,
    /// Follow-ups the person could have put up front.
    pub waste: Vec<Waste>,
    /// Missing habits: one bit per [`Signal`] in [`Signal::ALL`] order, plus [`CONTEXT_BIT`].
    pub missing: u8,
    /// The AI built the wrong thing because the goal itself was never said.
    pub wrong: bool,
}

impl CardTask {
    /// Follow-ups that redid some work. Pure additions (0%) are tips, not lost work.
    pub fn avoidable(&self) -> usize {
        self.waste.iter().filter(|w| w.share > 0).count()
    }

    /// Its letter grade.
    pub fn grade(&self) -> LetterGrade {
        LetterGrade::of(self.avoidable(), self.wrong)
    }
}

/// A parsed record block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    /// The chat's seal, as written. Check it with [`seal`] before trusting anything else.
    pub seal: String,
    /// Written by automatic grading, not asked for.
    pub auto: bool,
    /// The graded tasks.
    pub tasks: Vec<CardTask>,
}

/// Why a record block was ignored. Reasons only, never content, so they're safe to log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardError {
    /// Over 4 KB.
    TooBig,
    /// Not the JSON shape of a record block.
    BadJson,
    /// A version this engine doesn't read.
    UnknownVersion,
    /// No tasks, or more than 20.
    TaskCount,
    /// An id or the seal isn't 8 lowercase hex chars.
    BadId,
    /// A share that isn't 0, 25, 50, 75 or 100, or too many follow-ups.
    BadWaste,
}

/// The seal of chat `session`: the first 8 hex chars of HMAC-SHA256(`secret`, `session`). A block
/// copied from another chat, a web page or these docs carries the wrong one.
pub fn seal(secret: &[u8], session: &str) -> String {
    hmac_sha256(secret, session.as_bytes())[..SHORT_ID].to_string()
}

/// True when `text` may hold a record block: the fence, not just the word.
pub fn has_card(text: &str) -> bool {
    text.contains(CARD_FENCE)
}

/// The bodies of every record block in `text`, in order.
pub fn find_cards(text: &str) -> Vec<&str> {
    blocks(text).into_iter().map(|(_, body)| body).collect()
}

/// `text` with every record block removed, valid or not, so a report card can never trip the
/// correction rules.
pub fn strip_cards(text: &str) -> Cow<'_, str> {
    let found = blocks(text);
    if found.is_empty() {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    for ((start, end), _) in found {
        out.push_str(&text[from..start]);
        from = end;
    }
    out.push_str(&text[from..]);
    Cow::Owned(out)
}

/// Each block's byte range (fences included) and its body. A fence opens at the start of a line;
/// an unclosed block runs to the end of the text.
fn blocks(text: &str) -> Vec<((usize, usize), &str)> {
    let mut found = Vec::new();
    let mut search = 0;
    while let Some(i) = text[search..].find(CARD_FENCE).map(|i| i + search) {
        let line_start = text[..i].rfind('\n').map_or(0, |n| n + 1);
        if !text[line_start..i].trim().is_empty() {
            search = i + CARD_FENCE.len();
            continue; // mid-line: quoted, not a block
        }
        let body_start = text[i..].find('\n').map_or(text.len(), |n| i + n + 1);
        let close = text[body_start..].find("```").map(|n| body_start + n);
        let (body_end, end) = match close {
            Some(c) => (c, text[c..].find('\n').map_or(text.len(), |n| c + n + 1)),
            None => (text.len(), text.len()),
        };
        found.push(((line_start, end), text[body_start..body_end].trim()));
        search = end.max(i + CARD_FENCE.len());
    }
    found
}

#[derive(Deserialize)]
struct Head {
    v: u64,
}

#[derive(Deserialize)]
struct RawCard {
    k: String,
    #[serde(default)]
    auto: bool,
    tasks: Vec<RawTask>,
}

#[derive(Deserialize)]
struct RawTask {
    at: String,
    to: String,
    #[serde(default)]
    waste: Vec<(String, u8)>,
    #[serde(default)]
    miss: Vec<String>,
    #[serde(default)]
    wrong: bool,
}

/// Reads one block body. Unknown fields are ignored (later versions only add fields); unknown
/// habit names are dropped.
pub fn parse_card(body: &str) -> Result<Card, CardError> {
    if body.len() > MAX_BYTES {
        return Err(CardError::TooBig);
    }
    let head: Head = serde_json::from_str(body).map_err(|_| CardError::BadJson)?;
    if head.v != CARD_VERSION {
        return Err(CardError::UnknownVersion);
    }
    let raw: RawCard = serde_json::from_str(body).map_err(|_| CardError::BadJson)?;
    if raw.tasks.is_empty() || raw.tasks.len() > MAX_TASKS {
        return Err(CardError::TaskCount);
    }
    if !is_short_id(&raw.k) {
        return Err(CardError::BadId);
    }
    let tasks = raw.tasks.into_iter().map(task).collect::<Result<_, _>>()?;
    Ok(Card { seal: raw.k, auto: raw.auto, tasks })
}

fn task(raw: RawTask) -> Result<CardTask, CardError> {
    if !is_short_id(&raw.at) || !is_short_id(&raw.to) || !raw.waste.iter().all(|(id, _)| is_short_id(id)) {
        return Err(CardError::BadId);
    }
    let mut waste: Vec<Waste> = Vec::with_capacity(raw.waste.len());
    for (id, share) in raw.waste {
        if !SHARES.contains(&share) || waste.len() == MAX_WASTE {
            return Err(CardError::BadWaste);
        }
        // A task's first prompt can't be its own follow-up, and one follow-up counts once.
        if id != raw.at && !waste.iter().any(|w| w.id == id) {
            waste.push(Waste { id, share });
        }
    }
    let missing = raw.miss.iter().filter_map(|m| habit_bit(m)).fold(0, |acc, b| acc | b);
    Ok(CardTask { at: raw.at, to: raw.to, waste, missing, wrong: raw.wrong })
}

/// The `missing` bit for a habit name as a report card writes it.
pub fn habit_bit(name: &str) -> Option<u8> {
    let signal = match name {
        "target" => Signal::Spot,
        "evidence" => Signal::Proof,
        "finish" => Signal::Done,
        "guardrails" => Signal::Limits,
        "purpose" => Signal::Why,
        "context" => return Some(CONTEXT_BIT),
        _ => return None,
    };
    Signal::ALL.iter().position(|s| *s == signal).map(|i| 1 << i)
}

fn is_short_id(s: &str) -> bool {
    s.len() == SHORT_ID && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = r#"{"v":1,"k":"9c41e0aa","tasks":[{"at":"7f3a9c21","to":"e44a1b90","waste":[["b81e04d2",50],["c09f7713",0]],"miss":["evidence","finish","shoes"]}]}"#;

    fn card_text() -> String {
        format!("Report card: C\n\n{CARD_FENCE}\n{BODY}\n```\n")
    }

    #[test]
    fn finds_and_parses_a_block() {
        let text = card_text();
        assert!(has_card(&text));
        let bodies = find_cards(&text);
        assert_eq!(bodies, vec![BODY]);
        let card = parse_card(bodies[0]).unwrap();
        assert_eq!(card.seal, "9c41e0aa");
        assert!(!card.auto);
        let t = &card.tasks[0];
        assert_eq!(t.waste.len(), 2);
        assert_eq!(t.avoidable(), 1, "a 0% follow-up is a late addition, not lost work");
        assert_eq!(t.grade(), LetterGrade::B);
        assert_eq!(t.missing, habit_bit("evidence").unwrap() | habit_bit("finish").unwrap(), "unknown names dropped");
    }

    #[test]
    fn strips_every_block_and_keeps_the_rest() {
        let text = format!("{}And more.\n{CARD_FENCE}\nnot json\n```\nEnd", card_text());
        assert_eq!(strip_cards(&text), "Report card: C\n\nAnd more.\nEnd");
        assert_eq!(strip_cards("no card"), "no card");
        assert!(matches!(strip_cards("no card"), Cow::Borrowed(_)));
    }

    #[test]
    fn a_fence_mid_line_is_a_quote_not_a_block() {
        let text = format!("the docs show `{CARD_FENCE}` blocks");
        assert_eq!(find_cards(&text), Vec::<&str>::new());
        assert_eq!(strip_cards(&text), text);
    }

    #[test]
    fn an_unclosed_block_runs_to_the_end() {
        let text = format!("hi\n{CARD_FENCE}\n{BODY}");
        assert_eq!(find_cards(&text), vec![BODY]);
        assert_eq!(strip_cards(&text), "hi\n");
    }

    #[test]
    fn grades_follow_the_count() {
        let g: Vec<_> = (0..6).map(|n| LetterGrade::of(n, false).as_str()).collect();
        assert_eq!(g, ["A", "B", "C", "D", "F", "F"]);
        assert_eq!(LetterGrade::of(0, true), LetterGrade::F, "goal never said");
    }

    #[test]
    fn rejects_what_it_cant_trust() {
        let bad = |body: &str| parse_card(body).unwrap_err();
        assert_eq!(bad("nope"), CardError::BadJson);
        assert_eq!(bad(r#"{"v":2,"whatever":true}"#), CardError::UnknownVersion);
        assert_eq!(bad(r#"{"v":1,"k":"9c41e0aa","tasks":[]}"#), CardError::TaskCount);
        assert_eq!(bad(r#"{"v":1,"k":"9C41E0AA","tasks":[{"at":"7f3a9c21","to":"7f3a9c21"}]}"#), CardError::BadId);
        assert_eq!(bad(r#"{"v":1,"k":"9c41e0aa","tasks":[{"at":"7f3a9c2","to":"7f3a9c21"}]}"#), CardError::BadId);
        assert_eq!(
            bad(r#"{"v":1,"k":"9c41e0aa","tasks":[{"at":"7f3a9c21","to":"7f3a9c21","waste":[["b81e04d2",30]]}]}"#),
            CardError::BadWaste
        );
        assert_eq!(bad(&format!(r#"{{"v":1,"pad":"{}"}}"#, "x".repeat(MAX_BYTES))), CardError::TooBig);
    }

    #[test]
    fn a_follow_up_counts_once_and_never_as_its_own_start() {
        let card = parse_card(
            r#"{"v":1,"k":"9c41e0aa","auto":true,"tasks":[{"at":"7f3a9c21","to":"e44a1b90","waste":[["7f3a9c21",100],["b81e04d2",25],["b81e04d2",75]]}]}"#,
        )
        .unwrap();
        assert!(card.auto);
        assert_eq!(card.tasks[0].waste, vec![Waste { id: "b81e04d2".into(), share: 25 }]);
    }

    #[test]
    fn the_seal_depends_on_the_secret_and_the_chat() {
        let s = seal(b"secret", "chat-1");
        assert_eq!(s.len(), 8);
        assert_eq!(s, seal(b"secret", "chat-1"));
        assert_ne!(s, seal(b"secret", "chat-2"));
        assert_ne!(s, seal(b"other", "chat-1"));
    }
}
