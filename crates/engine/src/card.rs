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

/// The block version this engine reads.
const CARD_VERSION: u64 = 1;
/// Opens a record block. Also the cheap check before any parsing.
const CARD_FENCE: &str = "```noactually";
/// How much of the earlier work a follow-up made someone redo, in percent. Only these steps, so
/// the same chat gets the same answer every run.
pub(crate) const SHARES: [u8; 5] = [0, 25, 50, 75, 100];

const MAX_BYTES: usize = 4096;
const MAX_TASKS: usize = 20;
const MAX_WASTE: usize = 50;
/// The `missing` bit for Context: a fact about their setup or a preference. The signals take the
/// bits below it, in [`Signal::ALL`] order.
const CONTEXT_BIT: u8 = 1 << 5;

/// What a report card can say was left out, as a card writes it, with the prompt signal that measures
/// it (`None` for Context, which only a report card can tell). The order is the order of the `missing`
/// bits and of every list of habits: the store, the digest and the app all read this one table.
pub const HABITS: [(&str, Option<Signal>); 6] = [
    ("target", Some(Signal::Spot)),
    ("evidence", Some(Signal::Proof)),
    ("finish", Some(Signal::Done)),
    ("guardrails", Some(Signal::Limits)),
    ("purpose", Some(Signal::Why)),
    ("context", None),
];

/// The `missing` bit of one row of [`HABITS`].
fn bit_of(signal: Option<Signal>) -> u8 {
    signal.map_or(CONTEXT_BIT, Signal::bit)
}

/// Each habit's name and its `missing` bit, in [`HABITS`] order.
pub fn habit_bits() -> [(&'static str, u8); 6] {
    HABITS.map(|(name, signal)| (name, bit_of(signal)))
}

/// The `missing` bit for a habit name as a report card writes it.
pub fn habit_bit(name: &str) -> Option<u8> {
    HABITS.iter().find(|(n, _)| *n == name).map(|(_, signal)| bit_of(*signal))
}

/// The name a report card gives the habit that `signal` measures.
pub fn habit_name(signal: Signal) -> &'static str {
    HABITS.iter().find(|(_, s)| *s == Some(signal)).map_or("", |(name, _)| name)
}

/// A letter grade, from how many follow-ups made someone redo work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum LetterGrade {
    /// No extra back-and-forth.
    A,
    /// One.
    B,
    /// Two.
    C,
    /// Three, or the goal itself was never said.
    D,
    /// Four or more.
    F,
}

impl LetterGrade {
    /// Every grade, best first.
    pub const ALL: [LetterGrade; 5] = [LetterGrade::A, LetterGrade::B, LetterGrade::C, LetterGrade::D, LetterGrade::F];

    /// The grade for `avoidable` rounds. `wrong` (the goal itself was never said, so the first
    /// round went to the wrong thing) makes it a D at best: worse than a missing detail, but one
    /// vague message isn't an F on its own.
    pub fn of(avoidable: usize, wrong: bool) -> LetterGrade {
        let by_count = match avoidable {
            0 => LetterGrade::A,
            1 => LetterGrade::B,
            2 => LetterGrade::C,
            3 => LetterGrade::D,
            _ => LetterGrade::F,
        };
        if wrong {
            by_count.max(LetterGrade::D)
        } else {
            by_count
        }
    }

    /// The grade stored as `id` ("A".."F"), if it is one.
    pub fn from_id(id: &str) -> Option<LetterGrade> {
        LetterGrade::ALL.into_iter().find(|g| g.as_str() == id)
    }

    /// True for an A or a B: a task that took at most one follow-up.
    pub fn is_ab(self) -> bool {
        self <= LetterGrade::B
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

/// The habit in plain words, for someone new to this. The same words as the report card skill's table.
pub fn habit_plain(habit: &str) -> &'static str {
    match habit {
        "target" => "say which file, page or screen",
        "evidence" => "paste the error or what you saw",
        "finish" => "say what done looks like",
        "guardrails" => "say what not to touch",
        "purpose" => "say why, or who it's for",
        _ => "say what you already know about your setup or taste",
    }
}

/// A follow-up the person could have put in their first message, and how much work it redid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waste {
    /// The follow-up's short id.
    pub id: String,
    /// Percent of the task's work up to it that had to be redone: 0, 25, 50, 75 or 100.
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

/// True when two graded tasks cover the same work: they start at the same prompt, or their spans
/// share more than an endpoint. A model that ends one task and starts the next on the same prompt
/// doesn't make them overlap, so a later card for the second can't replace the first. Spans are
/// (first, last) prompt positions or times; `same_first` is whether the two start at the same
/// prompt. The store keeps the same rule in SQL (`current_report_cards`) and the helper uses this
/// one for "graded already", and a test holds the two together.
pub fn tasks_overlap<T: Ord>(a: (T, T), b: (T, T), same_first: bool) -> bool {
    same_first || (a.0 < b.1 && b.0 < a.1)
}

/// A parsed record block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    /// The chat's seal, as written. Check it with [`seal`] before trusting anything else.
    pub seal: String,
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

/// The bodies of every record block in `text`, in order. A fence opens at the start of a line (a
/// mid-line one is a quote, not a block); an unclosed block runs to the end of the text.
pub fn find_cards(text: &str) -> Vec<&str> {
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
        found.push(text[body_start..body_end].trim());
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
    Ok(Card { seal: raw.k, tasks })
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
        let t = &card.tasks[0];
        assert_eq!(t.waste.len(), 2);
        assert_eq!(t.avoidable(), 1, "a 0% follow-up is a late addition, not lost work");
        assert_eq!(t.grade(), LetterGrade::B);
        assert_eq!(t.missing, habit_bit("evidence").unwrap() | habit_bit("finish").unwrap(), "unknown names dropped");
    }

    #[test]
    fn finds_every_block_valid_or_not() {
        let text = format!("{}And more.\n{CARD_FENCE}\nnot json\n```\nEnd", card_text());
        assert_eq!(find_cards(&text), vec![BODY, "not json"]);
        assert_eq!(find_cards("no card"), Vec::<&str>::new());
    }

    #[test]
    fn a_fence_mid_line_is_a_quote_not_a_block() {
        let text = format!("the docs show `{CARD_FENCE}` blocks");
        assert_eq!(find_cards(&text), Vec::<&str>::new());
    }

    #[test]
    fn an_unclosed_block_runs_to_the_end() {
        let text = format!("hi\n{CARD_FENCE}\n{BODY}");
        assert_eq!(find_cards(&text), vec![BODY]);
    }

    #[test]
    fn one_table_names_the_habits_and_their_bits() {
        let bits = habit_bits();
        assert_eq!(bits.map(|(n, _)| n), ["target", "evidence", "finish", "guardrails", "purpose", "context"]);
        assert_eq!(bits.map(|(_, b)| b), [1, 2, 4, 8, 16, 32], "one bit each, in table order");
        for (i, (_, signal)) in HABITS.iter().enumerate() {
            if let Some(s) = signal {
                assert_eq!(Signal::ALL[i], *s, "the signals come first, in their own order");
            }
        }
        assert_eq!(habit_bit("finish"), Some(Signal::Done.bit()));
        assert_eq!(habit_bit("context"), Some(CONTEXT_BIT));
        assert_eq!(habit_bit("shoes"), None);
        assert_eq!(habit_name(Signal::Limits), "guardrails");
        assert!(Signal::ALL.iter().all(|s| habit_bit(habit_name(*s)) == Some(s.bit())));
    }

    #[test]
    fn letter_grades_round_trip_and_a_b_is_good() {
        assert!(LetterGrade::ALL.iter().all(|g| LetterGrade::from_id(g.as_str()) == Some(*g)));
        assert_eq!(LetterGrade::from_id("E"), None);
        assert_eq!(LetterGrade::from_id("a"), None);
        let good: Vec<_> = LetterGrade::ALL.iter().map(|g| g.is_ab()).collect();
        assert_eq!(good, [true, true, false, false, false]);
    }

    #[test]
    fn tasks_overlap_when_they_share_more_than_an_endpoint_or_a_first_prompt() {
        assert!(tasks_overlap((0, 10), (5, 15), false), "a stretch in common");
        assert!(tasks_overlap((0, 10), (2, 4), false), "one inside the other");
        assert!(!tasks_overlap((0, 10), (10, 20), false), "A's last prompt is B's first");
        assert!(!tasks_overlap((10, 20), (0, 10), false), "and the other way round");
        assert!(!tasks_overlap((0, 10), (11, 20), false), "apart");
        assert!(tasks_overlap((0, 0), (0, 9), true), "the same first prompt, even for a one-prompt task");
        assert!(!tasks_overlap((5, 5), (0, 5), false), "a one-prompt task at the end of another");
        assert!(tasks_overlap((5, 5), (0, 9), false), "a one-prompt task inside another");
    }

    #[test]
    fn grades_follow_the_count() {
        let g: Vec<_> = (0..6).map(|n| LetterGrade::of(n, false).as_str()).collect();
        assert_eq!(g, ["A", "B", "C", "D", "F", "F"]);
        assert_eq!(LetterGrade::of(1, true), LetterGrade::D, "goal never said: a D at best");
        assert_eq!(LetterGrade::of(0, true), LetterGrade::D);
        assert_eq!(LetterGrade::of(5, true), LetterGrade::F, "still an F when it's that bad anyway");
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
            r#"{"v":1,"k":"9c41e0aa","tasks":[{"at":"7f3a9c21","to":"e44a1b90","waste":[["7f3a9c21",100],["b81e04d2",25],["b81e04d2",75]]}]}"#,
        )
        .unwrap();
        assert_eq!(card.tasks[0].waste, vec![Waste { id: "b81e04d2".into(), share: 25 }]);
    }

    #[test]
    fn a_field_this_version_doesnt_use_is_ignored() {
        let body = r#"{"v":1,"k":"9c41e0aa","auto":true,"tasks":[{"at":"7f3a9c21","to":"e44a1b90"}]}"#;
        assert_eq!(parse_card(body).unwrap().tasks.len(), 1);
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
