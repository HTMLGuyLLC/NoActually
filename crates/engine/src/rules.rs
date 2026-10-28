//! The rule book. Every rule has a stable id so stored events can say why they were labeled.
//! Changing a pattern changes scores, so pair every edit with a test in `tests.rs`.

use crate::Category;
use regex::Regex;
use std::sync::LazyLock;

pub struct Rule {
    pub id: &'static str,
    re: Regex,
    /// Checked from the start of the match: when it also matches, the rule doesn't fire.
    /// The regex crate has no lookahead, so this is how a rule says "but not when ...".
    unless: Option<Regex>,
}

impl Rule {
    pub fn find(&self, text: &str) -> Option<Match> {
        let m = self.re.find(text)?;
        if self.unless.as_ref().is_some_and(|u| u.is_match(&text[m.start()..])) {
            return None;
        }
        Some(Match { rule: self.id, span: (m.start(), m.end()) })
    }
}

#[derive(Clone, Copy)]
pub struct Match {
    pub rule: &'static str,
    pub span: (usize, usize),
}

fn re(id: &str, pattern: &str) -> Regex {
    Regex::new(&format!("(?i){pattern}")).expect(id)
}

fn rule(id: &'static str, pattern: &str) -> Rule {
    Rule { id, re: re(id, pattern), unless: None }
}

/// A rule that doesn't fire when `unless` (anchored at the match start) also matches.
fn rule_unless(id: &'static str, pattern: &str, unless: &str) -> Rule {
    Rule { id, re: re(id, pattern), unless: Some(re(id, &format!("^(?:{unless})"))) }
}

pub fn first(rules: &[Rule], text: &str) -> Option<Match> {
    rules.iter().find_map(|r| r.find(text))
}

/// "that's fine", "it works", "you're right": a subject plus an approval word. Appended to an
/// opener like "no, " it marks approval ("No, that's fine, ship it"), not pushback. Present tense
/// only: "No, it worked before you changed it" and "it was fine until…" are corrections.
const APPROVAL: &str = r"(that|this|it|you)('s|'re|s|re|\s+is|\s+are|\s+looks?|\s+sounds?|\s+seems?)?(\s+(all|also|actually|just|totally|perfectly|pretty|really|now))?\s+(fine|good|great|ok|okay|perfect|right|correct|works?|working)\b";

/// The AI admits it was wrong (checked at the start of its next reply).
pub static AI_ADMIT: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        rule("ai.youre_right", r"\byou'?re (absolutely |totally |completely )?right\b"),
        rule("ai.you_are_right", r"\byou are (absolutely )?right\b"),
        rule("ai.good_catch", r"\b(good|great|nice|sharp) catch\b"),
        rule("ai.my_mistake", r"\bmy (mistake|bad|apologies|error)\b"),
        rule(
            "ai.i_was_wrong",
            r"\bi (was wrong|got (that|it) wrong|missed|misread|overlooked|misunderstood|made (a|an) (mistake|error))\b",
        ),
        rule("ai.apologize", r"\bi apologi[sz]e\b"),
        rule("ai.sorry", r"\bsorry,? (about that|for (the|that))\b"),
        rule("ai.on_me", r"\bthat'?s on me\b"),
        rule("ai.should_have", r"\bi should have\b"),
        rule("ai.incorrectly", r"\bi incorrectly\b"),
        rule("ai.that_was_wrong", r"\bthat was (wrong|incorrect|a mistake)\b"),
        rule("ai.didnt_check", r"\bi (didn'?t|did not) (actually )?(check|verify|read|catch)\b"),
    ]
});

/// You open your message by pushing back.
pub static USER_OPEN: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        rule_unless(
            "you.no_that",
            r"^(no|nope|nah)\s*[,.!—–-]+\s*(that|this|it|you|not|the|we|i said|still|wrong)\b",
            &format!(r"(no|nope|nah)\s*[,.!—–-]+\s*{APPROVAL}"),
        ),
        rule("you.no", r"^(no|nope|nah|wrong)[.!]*$"),
        rule_unless(
            "you.wrong",
            r"^(wrong|incorrect|not quite|not true|that'?s not|that is not|that isn'?t|this is not|this isn'?t)\b",
            // "that's not a problem", "this isn't bad": a negated complaint is approval.
            r"(that'?s not|that is not|that isn'?t|this is not|this isn'?t)\s+(a (big )?deal|a problem|an issue|bad|wrong)(\s+at all)?\s*([,.!;:\n]|$)",
        ),
        rule(
            "you.its_wrong",
            r"^(it'?s|that'?s|this is) (still )?(wrong|incorrect|broken|not (right|correct|working))",
        ),
        rule(
            "you.still",
            r"^still (not|seeing|broken|getting|showing|can'?t|cannot|no|wrong|doesn'?t|isn'?t|the same|failing|happening|there)\b",
        ),
        rule("you.you_missed", r"^(you|u) (missed|forgot|didn'?t|did not|ignored|broke|skipped|never)\b"),
        rule("you.i_said", r"^(i said|i told you|like i said|as i said|i already (said|told|asked))\b"),
        rule("you.i_asked", r"^i asked (you )?(to|for|not)\b"),
        rule("you.why_did_you", r"^(why did you|why would you|what\??$|wtf|huh\??$)"),
        rule("you.doesnt_work", r"^(doesn'?t|didn'?t|does not|did not|isn'?t|not) work"),
        rule("you.same_error", r"^(same|still the same) (error|issue|problem|thing)"),
        rule_unless(
            "you.actually",
            r"^actually,? (it|that|this|you|the|no)\b",
            &format!(r"actually,?\s+({APPROVAL}|no\s+(worries|problem)\b|no\s*[,.!—–-]+\s*{APPROVAL})"),
        ),
    ]
});

/// Pushback anywhere in your message. Only counts when the AI also admits it.
pub static USER_ANY: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        rule("you.any.doesnt_exist", r"\b(doesn'?t|does not|didn'?t|did not) (exist|work)\b"),
        rule("you.any.not_working", r"\bnot working\b"),
        rule("you.any.still", r"\bstill (broken|failing|not|wrong|the same|showing|getting)\b"),
        rule(
            "you.any.you_missed",
            r"\byou (missed|forgot|ignored|broke|skipped|didn'?t (read|check|test|follow|do))\b",
        ),
        rule("you.any.thats_wrong", r"\b(that'?s|this is|it'?s) (wrong|incorrect|not right)\b"),
        rule("you.any.made_up", r"\bmade (that|it) up\b|\bhallucinat"),
        rule("you.any.i_said", r"\bi (said|told you|asked( you)? (to|for|not))\b"),
        rule("you.any.not_what_i", r"\bnot what i (asked|wanted|said|meant)\b"),
    ]
});

/// You added information the AI couldn't have known, or you're just acknowledging. Never a catch.
pub static CONTEXT: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        rule("ctx.also", r"^(also|oh and|btw|fyi|one more thing|additionally|and also|plus)\b"),
        rule("ctx.forgot_to_mention", r"\bi forgot to (mention|say|tell)\b"),
        rule("ctx.ack", r"^(ok|okay|great|thanks|perfect|cool|nice|yes|yep|sure)\b"),
    ]
});

/// The AI pushes back on you.
pub static AI_PUSHBACK: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        rule(
            "ai.push.actually",
            r"^\W*(actually|not quite|small correction|one correction|quick correction|to clarify)\b",
        ),
        rule("ai.push.not_right", r"\b(that'?s|this is|that is) not (quite )?(right|correct|accurate|true|how)\b"),
        rule("ai.push.dont_think", r"\bi (don'?t|do not) think (that'?s|that is|this is) (right|correct|accurate)\b"),
        rule("ai.push.not_the_case", r"\b(isn'?t|is not) (actually )?(the case|accurate|correct)\b"),
        rule("ai.push.push_back", r"\bi'?d push back\b"),
        rule("ai.push.already_exists", r"^\W*(it|that) (already|actually) (exists|works|does|is)\b"),
        rule("ai.push.not_a_bug", r"\bthat'?s not a bug\b"),
    ]
});

/// Short "are you done?" style questions: an AI "Not quite" there is a status, not a correction.
pub static STATUS_Q: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(finish|done|left|still going|status|ready)\b").unwrap());

static CATEGORIES: LazyLock<Vec<(Category, Regex)>> = LazyLock::new(|| {
    let c = |cat, p: &str| (cat, Regex::new(&format!("(?i){p}")).unwrap());
    vec![
        c(
            Category::MadeItUp,
            r"\b(doesn'?t|does not) exist\b|\bmade (that|it) up\b|\bhallucinat|\bno such\b|\bnot a real\b|\bthere is no\b|\bthat'?s not a thing\b",
        ),
        c(
            Category::IgnoredInstructions,
            r"\bi (said|told you|asked|already)\b|\blike i said\b|\bnot what i (asked|said)\b|\byou ignored\b|\bre-?read\b",
        ),
        c(
            Category::Misread,
            r"\bi (misread|misunderstood|misinterpreted)\b|\bi (read|took) .{0,40} as\b|\bnot what i (meant|wanted)\b|\bi meant\b",
        ),
        c(
            Category::MissedSomething,
            r"\byou (missed|forgot|skipped|didn'?t|did not|never)\b|\bmissing\b|\bwhat about\b|\bleft out\b",
        ),
        c(
            Category::StillBroken,
            r"\bstill\b|\bsame (error|issue|problem)\b|\b(doesn'?t|didn'?t|not) work|\bbroken\b|\berror\b|\bcrash|\bfail",
        ),
    ]
});

/// First matching category wins; anything else is plain wrong. `human` is your message, `ai` the
/// AI's reply, and `admit` the span of the AI's admission phrase: it is cut out first, because
/// "My error" or "I was wrong" says that a mistake happened, not what kind.
pub fn category(human: &str, ai: &str, admit: Option<(usize, usize)>) -> Category {
    let ai = match admit {
        Some((s, e)) => format!("{} {}", &ai[..s], &ai[e..]),
        None => ai.to_string(),
    };
    let text = format!("{human}\n{ai}");
    CATEGORIES.iter().find(|(_, re)| re.is_match(&text)).map_or(Category::JustWrong, |(c, _)| *c)
}
