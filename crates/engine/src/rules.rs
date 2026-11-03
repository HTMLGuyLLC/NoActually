//! The rule book. Every rule has a stable id so stored events can say why they were labeled.
//! Changing a pattern changes scores, so pair every edit with a test in `tests.rs`.

use crate::Category;
use regex::Regex;
use std::sync::LazyLock;

pub struct Rule {
    pub id: &'static str,
    re: Regex,
    /// Checked from the start of each match: when it also matches, that match doesn't count and
    /// the next one is tried. The regex crate has no lookahead, so this is how a rule says "but
    /// not when ...".
    unless: Option<Regex>,
    /// Checked on the text before each match, the same way: "but not right after ...".
    not_after: Option<Regex>,
}

impl Rule {
    pub fn find(&self, text: &str) -> Option<Match> {
        let vetoed = |start: usize| {
            self.unless.as_ref().is_some_and(|u| u.is_match(&text[start..]))
                || self.not_after.as_ref().is_some_and(|b| b.is_match(&text[..start]))
        };
        let m = self.re.find_iter(text).find(|m| !vetoed(m.start()))?;
        Some(Match { rule: self.id, span: (m.start(), m.end()) })
    }

    /// Adds the words that leave an admission open: "let me check if I missed it" admits nothing.
    fn not_when_unsure(self) -> Rule {
        Rule { not_after: Some(re(self.id, &format!("(?:{UNSURE})$"))), ..self }
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

pub(crate) fn rule(id: &'static str, pattern: &str) -> Rule {
    Rule { id, re: re(id, pattern), unless: None, not_after: None }
}

/// A rule that doesn't fire when `unless` (anchored at the match start) also matches.
pub(crate) fn rule_unless(id: &'static str, pattern: &str, unless: &str) -> Rule {
    Rule { id, re: re(id, pattern), unless: Some(re(id, &format!("^(?:{unless})"))), not_after: None }
}

/// A rule that doesn't fire when the text before the match ends with `before`.
pub(crate) fn rule_not_after(id: &'static str, pattern: &str, before: &str) -> Rule {
    Rule { id, re: re(id, pattern), unless: None, not_after: Some(re(id, &format!("(?:{before})$"))) }
}

pub fn first(rules: &[Rule], text: &str) -> Option<Match> {
    rules.iter().find_map(|r| r.find(text))
}

/// "not necessary", "not needed": nothing left to do, not a complaint. A macro so `concat!` can
/// build [`APPROVAL`] from it.
macro_rules! not_needed {
    () => {
        r"not\s+(really\s+|strictly\s+)?(necessary|needed|required|important|urgent)\b"
    };
}

/// "that's fine", "it works", "you're right": a subject plus an approval word. Appended to an
/// opener like "no, " it marks approval ("No, that's fine, ship it"), not pushback. Present tense
/// only: "No, it worked before you changed it" and "it was fine until…" are corrections. "That's
/// not necessary" and "that's all for now" close a task politely; "that's all wrong" doesn't.
const APPROVAL: &str = concat!(
    r"(that|this|it|you)('s|'re|s|re|\s+is|\s+are|\s+looks?|\s+sounds?|\s+seems?)?(\s+(all|also|actually|just|totally|perfectly|pretty|really|now))?\s+((fine|good|great|ok|okay|perfect|right|correct|works?|working|alright|all right|cool)\b|",
    not_needed!(),
    r"|all(\s+for\s+now)?\s*([,.!;:—–-]|$|thanks\b|thank you\b))"
);

/// Openers that add what the AI couldn't have known: "also", "by the way", "p.s.". Not a bare
/// "ps": "ps aux" and a "PS C:\>" prompt are pastes. Shared by
/// [`CONTEXT`] and the prompt grader's "you added more", so the two never disagree.
pub(crate) const ADDED_OPENER: &str = r"also|oh,? and|ah,? and|oh,? also|ah,? also|and also|btw|by the way|fyi|one more thing|one other thing|another thing|additionally|in addition|plus,|p\.s\.|ps[:,]";

/// "I forgot to mention", "I should have said": a requirement added late, on you.
pub(crate) const ADDED_FORGOT: &str = r"\bi (?:forgot|neglected) to (?:mention|say|tell|add|include|note|specify)\b|\bforgot to mention\b|\bi should have (?:mentioned|said|told|added|specified)\b|\bi didn'?t mention\b";

/// Right before an admission, these leave it open: "let me check if you're right", "I don't think
/// you're right", "that might be a good catch".
const UNSURE: &str = r"\b(if|whether|unless|in case|not( sure| certain)?|don'?t think|do not think|doubt|maybe|perhaps|possibly|(might|could|may|would) be|(let me|let's|i'll|i will|to) (check|see|find out|figure out|look at|review|verify) (what|where))\s+((that's|that is|this is|it's)\s+)?(a\s+)?";

/// Separators after a phrase of praise, as in "Good catch, thanks — ".
const GAP: &str = r"[\s,.!;:—–-]*";

/// "Good catch — let me check what's running": praise, then a promise to go and look. It says the
/// question was smart, not that you were right. "Good catch — if that's so" hedges the same way.
const WILL_CHECK: &str = r"(let me|let's|lemme|i'?ll|i will|i'?m going to|i need to|i should|i want to|i'?d like to|going to)\s+((first|just|quickly|go|also|actually)\s+)*(double[- ]?|re-?)?(check|verify|confirm|look|see|investigate|inspect|review|dig|examine|trace|read|find out|take a look|have a look)\b|(checking|verifying|confirming|looking|investigating|digging|inspecting|reviewing|examining)\b|if (so|true|that|this|it|you)\b";

/// "You're right to ask": the question was fair, which doesn't make the claim true.
const RIGHT_TO_ASK: &str = r"you('?re| are) (absolutely |totally |completely )?(right|correct) to (ask|question|wonder|check|double-check|verify|confirm|be (suspicious|skeptical|sceptical|cautious|careful|concerned|curious|wary|worried))\b";

/// The AI admits it was wrong (checked at the start of its next reply).
pub static AI_ADMIT: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    let good_catch = format!(
        r"(good|great|nice|sharp) catch(\s+(there|on\s[^.!?,;:\n—–]{{1,40}}))?{GAP}((thanks|thank you|and|but|so|first|now|ok|okay|hmm|hold on|one sec|one moment)\b{GAP})*({WILL_CHECK})"
    );
    vec![
        rule_unless("ai.youre_right", r"\byou'?re (absolutely |totally |completely )?(right|correct)\b", RIGHT_TO_ASK),
        rule_unless("ai.you_are_right", r"\byou (are|were) (absolutely |totally |completely )?(right|correct)\b", RIGHT_TO_ASK),
        rule_unless("ai.good_catch", r"\b(good|great|nice|sharp) catch\b", &good_catch),
        rule_unless(
            "ai.my_mistake",
            r"\bmy (mistake|bad|apologies|error)\b",
            // Code and habits, not an admission: "my error handling", "my bad habit".
            r"my (error (handling|message|log|path|code|type|boundary|page)|bad (habit|luck|idea))",
        ),
        rule(
            "ai.i_was_wrong",
            r"\bi (was wrong|got (that|it) wrong|missed|misread|overlooked|misunderstood|made (a|an) (mistake|error)|used the wrong)\b",
        ),
        rule("ai.apologize", r"\bi apologi[sz]e\b"),
        rule_unless(
            "ai.sorry",
            r"\bsorry,? (about that|for (the|that))\b",
            // A slow reply is manners, not a mistake.
            r"sorry,? for (the|that) (long |short )?(delay|wait|interruption)\b",
        ),
        rule("ai.on_me", r"\bthat'?s on me\b"),
        rule_unless(
            "ai.should_have",
            r"\bi should have\b",
            // What the AI expects to have, not what it failed to do: "I should have enough context
            // now", "I should have a fix ready in a minute", "I should have this done shortly".
            r"i should have\s+(enough|everything|all|what|which|the (context|info|information|data|access|files?|details?|answer|results?|rest)|access|a (good|clear|better|full|complete)|more (time|context|info|information|data|details?|access)|some|plenty|sufficient|both|time|permission|(it|this|that|these|those|them|(a|the|an) (fix|patch|draft|pr|change|update|version|build|plan|answer))\s+((all|fully|mostly)\s+)?(now|soon|shortly|ready|done|working|fixed|finished|sorted|up|back|in a (minute|moment|sec|second|bit|few)))\b",
        ),
        rule("ai.incorrectly", r"\bi incorrectly\b"),
        rule("ai.that_was_wrong", r"\bthat was (wrong|incorrect|a mistake)\b"),
        rule_unless(
            "ai.didnt_check",
            r"\bi (didn'?t|did not) (actually )?(check|verify|read|catch|correction)\b",
            // "I didn't catch that" can mean "I didn't hear you" (but "didn't catch that bug" admits it).
            r"i (didn'?t|did not) (actually )?catch\s+(that|this|what you (said|meant|mean|wrote|typed)|you|your|the (last|previous|whole|rest))\b(\s+(last|part|message|one|again|bit))?\s*([,.!?;:—–-]|$|\s+(can|could|would|please|come|one more|again|sorry|what)\b)",
        ),
    ]
    .into_iter()
    .map(Rule::not_when_unsure)
    .collect()
});

/// Credits the person, rather than only owning up. "Good catch" isn't here: that far into a reply
/// it's mostly about a test or the AI's own check.
pub(crate) const YOU_WERE_RIGHT: [&str; 2] = ["ai.youre_right", "ai.you_are_right"];

/// The admissions that say the person was right, for a later message in a reply.
pub fn you_were_right(text: &str) -> Option<Match> {
    AI_ADMIT.iter().filter(|r| YOU_WERE_RIGHT.contains(&r.id)).find_map(|r| r.find(text))
}

/// You open your message by pushing back.
pub static USER_OPEN: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        // "Not needed" is approval ("No, that's not necessary."), unless it says to undo the work.
        rule(
            "you.unneeded",
            &format!(
                r"^((no|nope|nah)\s*[,.!—–-]+\s*)?((that|this|it)('s|\s+is)?\s+)?{}[^\n]{{0,60}}?\b(remove|revert|undo|delete|take (it |that |this )?out|roll (it |that |this )?back|i (said|asked))\b",
                not_needed!()
            ),
        ),
        rule_unless(
            "you.no_that",
            r"^(no|nope|nah)\s*[,.!—–-]+\s*(that|this|it|you|not|the|we|i said|still|wrong)\b",
            &format!(r"(no|nope|nah)\s*[,.!—–-]+\s*({APPROVAL}|{})", not_needed!()),
        ),
        rule("you.no", r"^(no|nope|nah|wrong)[.!]*$"),
        rule_unless(
            "you.wrong",
            r"^(wrong|incorrect|not quite|not true|that'?s not|that is not|that isn'?t|this is not|this isn'?t)\b",
            // "that's not a problem", "this isn't bad": a negated complaint is approval. "That's not
            // needed" says there's nothing more to do.
            r"(that'?s not|that is not|that isn'?t|this is not|this isn'?t)\s+((really|strictly)\s+)?(a (big )?deal|a problem|an issue|bad|wrong|necessary|needed|required|important|urgent)(\s+(at all|right now|now|yet|anymore|for now|today))?\s*([,.!;:\n]|$)",
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
        rule_unless(
            "you.doesnt_work",
            r"^(doesn'?t|didn'?t|does not|did not|isn'?t|not) work",
            // "Not working on the API today": a plan, not a complaint. "Not working on Safari" still is one.
            r"not working on\b[^.!?\n]*\b(today|tonight|this (week|morning|afternoon)|for now|focus)\b",
        ),
        rule_unless(
            "you.same_error",
            r"^(same|still the same) (error|issue|problem|thing)\b",
            // "Same thing for the footer please" asks for more of the same.
            r"(same|still the same) thing\s+(for|with|on|in|to|here|please)\b",
        ),
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

/// You added information the AI couldn't have known, or you're just acknowledging. Never a correction.
pub static CONTEXT: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        rule("ctx.also", &format!(r"^(?:{ADDED_OPENER})(?:\W|$)")),
        // At the start only: "Why did you add a footer? I didn't mention a footer." is pushback.
        rule(
            "ctx.forgot_to_mention",
            &format!(r"^(?:(?:oh|ah|oops|sorry|wait|also|and|ok|okay|btw)[,.!]*\s+)*(?:{ADDED_FORGOT})"),
        ),
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

/// Doubt inside a question: "are you sure?", "isn't it 5433?". That's pushing back, so a "you're
/// right" after it still admits a mistake; after a plain question it only confirms what you asked.
pub static DOUBT: LazyLock<Regex> = LazyLock::new(|| {
    re(
        "doubt",
        r"\breally\?|\b(are you (sure|certain|positive)|you sure|is (that|this|it) (really )?(right|correct|true|accurate)|did you (actually |really )?(check|test|verify|confirm|run|read|look)|(shouldn'?t|won'?t|can'?t|wouldn'?t|isn'?t|aren'?t|doesn'?t|don'?t|wasn'?t|didn'?t) \w+|what about|hold on|wait)\b",
    )
});

/// A request inside a question: "can you fix that?" asks for work, not information.
static REQUEST: LazyLock<Regex> = LazyLock::new(|| re("request", r"\b((can|could|would|will) you|please|pls|plz)\b"));

/// Sentence ends before the last one.
static SENTENCE_END: LazyLock<Regex> = LazyLock::new(|| re("sentence", r"[.!?]+\s"));

/// Longer than this (in chars), a question carries more than a quick check.
const PLAIN_QUESTION: usize = 120;

/// One short question asking for information, and nothing else: "Is the default port 5432?". Not
/// a statement then a question ("It crashes. Can you fix it?"), a request, or doubt.
pub fn plain_question(t: &str) -> bool {
    t.ends_with('?')
        && t.chars().count() <= PLAIN_QUESTION
        && !SENTENCE_END.is_match(t)
        && !REQUEST.is_match(t)
        && !DOUBT.is_match(t)
}

/// Short "are you done?" style questions: an AI "Not quite" there is a status, not a correction.
pub static STATUS_Q: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(finish|done|left|still going|status|ready)\b").unwrap());

/// (category, words read in your message, words read in the AI's reply).
type CategoryWords = (Category, Option<Regex>, Option<Regex>);

/// Each category's words, per side. "I said", "you missed" and "there is no" tell your side; in the
/// AI's reply they're about something else ("I already changed it", "there is no reason to keep
/// it"). "I misread" is the AI's side.
static CATEGORIES: LazyLock<Vec<CategoryWords>> = LazyLock::new(|| {
    let re = |p: &str| (!p.is_empty()).then(|| Regex::new(&format!("(?i){p}")).unwrap());
    let c = |cat, yours: &str, ais: &str| (cat, re(yours), re(ais));
    let made_up = r"\b(doesn'?t|does not) exist\b|\bmade (that|it) up\b|\bhallucinat|\bno such\b|\bnot a real\b|\bthat'?s not a thing\b";
    let missing = r"\bmissing\b|\bwhat about\b|\bleft out\b";
    let still =
        r"\bstill\b|\bsame (error|issue|problem)\b|\b(doesn'?t|didn'?t|not) work|\bbroken\b|\berror\b|\bcrash|\bfail";
    vec![
        c(Category::MadeItUp, &format!(r"{made_up}|\bthere is no\b"), made_up),
        c(
            Category::IgnoredInstructions,
            r"\bi (said|told you|asked|already)\b|\blike i said\b|\bnot what i (asked|said)\b|\byou ignored\b|\bre-?read\b",
            "",
        ),
        c(
            Category::Misread,
            r"\bnot what i (meant|wanted)\b|\bi meant\b",
            r"\bi (misread|misunderstood|misinterpreted)\b|\bi (read|took) .{0,40} as\b",
        ),
        c(
            Category::MissedSomething,
            &format!(r"\byou (missed|forgot|skipped|didn'?t|did not|never)\b|{missing}"),
            missing,
        ),
        c(Category::StillBroken, still, still),
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
    CATEGORIES
        .iter()
        .find(|(_, yours, ais)| {
            yours.as_ref().is_some_and(|r| r.is_match(human)) || ais.as_ref().is_some_and(|r| r.is_match(&ai))
        })
        .map_or(Category::JustWrong, |(c, ..)| *c)
}
