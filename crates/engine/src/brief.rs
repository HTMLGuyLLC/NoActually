//! How you brief the AI. [`read_prompt`] says whether a message starts a task (a brief), steers
//! the task already under way, or neither; for a brief it lists which specifics it carries.
//! Same rules as [`crate::classify`]: pure, deterministic, every decision names its rule.
//!
//! A brief's grade counts its [`Signal`]s, never its length, so padding a prompt earns nothing.
//! Questions aren't briefs: asking how something works is not handing over a task.

use crate::rules::{first, rule, rule_unless, Rule};
use crate::{head_chars, normalize_quotes, Label, Turn, Verdict};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

const HEAD: usize = 300; // chars checked for steering and task openers
const BRIEF: usize = 4000; // chars of a brief searched for signals (a long paste rarely adds more)

/// One kind of specific a brief can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Signal {
    /// Names the place: a file, function, link or `code`.
    Spot,
    /// Shows the problem: an error, the output, an example.
    Proof,
    /// Says what done looks like: the expected result, a test, a format.
    Done,
    /// Sets limits: don't, only, keep, without.
    Limits,
    /// Says why: the goal or the context.
    Why,
}

impl Signal {
    /// Every signal, in display order.
    pub const ALL: [Signal; 5] = [Signal::Spot, Signal::Proof, Signal::Done, Signal::Limits, Signal::Why];

    /// Stable id, as serialized.
    pub fn as_str(self) -> &'static str {
        match self {
            Signal::Spot => "spot",
            Signal::Proof => "proof",
            Signal::Done => "done",
            Signal::Limits => "limits",
            Signal::Why => "why",
        }
    }

    /// Display title ("Points at it").
    pub fn title(self) -> &'static str {
        match self {
            Signal::Spot => "Points at it",
            Signal::Proof => "Shows it",
            Signal::Done => "Says what done is",
            Signal::Limits => "Sets limits",
            Signal::Why => "Says why",
        }
    }

    /// What counts, in a few words.
    pub fn hint(self) -> &'static str {
        match self {
            Signal::Spot => "a file, function, link or code",
            Signal::Proof => "an error, the output or an example",
            Signal::Done => "the expected result, a test or a format",
            Signal::Limits => "don't, only, keep, without",
            Signal::Why => "the goal or the context",
        }
    }

    /// This signal's bit in a stored [`Signals`] mask. Never renumber: masks are stored.
    pub const fn bit(self) -> u8 {
        1 << self as u8
    }

    fn rules(self) -> &'static [Rule] {
        match self {
            Signal::Spot => &SPOT,
            Signal::Proof => &PROOF,
            Signal::Done => &DONE,
            Signal::Limits => &LIMITS,
            Signal::Why => &WHY,
        }
    }
}

/// The signals one brief carries, as a bit mask (stored as is).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Signals(pub u8);

impl Signals {
    /// True if the brief carries `s`.
    pub fn has(self, s: Signal) -> bool {
        self.0 & s.bit() != 0
    }

    /// Adds `s`.
    pub fn insert(&mut self, s: Signal) {
        self.0 |= s.bit();
    }

    /// How many signals it carries.
    pub fn count(self) -> u32 {
        Signal::ALL.iter().filter(|s| self.has(**s)).count() as u32
    }
}

/// How specific a brief is, from how many signals it carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grade {
    /// One signal or none: the AI has to guess.
    Thin,
    /// Two signals.
    Solid,
    /// Three or more.
    Sharp,
}

impl Grade {
    /// Every grade, lowest first.
    pub const ALL: [Grade; 3] = [Grade::Thin, Grade::Solid, Grade::Sharp];

    /// Signals a brief needs to be sharp.
    pub const SHARP: u32 = 3;

    /// The grade these signals earn.
    pub fn of(signals: Signals) -> Grade {
        match signals.count() {
            n if n >= Self::SHARP => Grade::Sharp,
            2 => Grade::Solid,
            _ => Grade::Thin,
        }
    }

    /// Stable id, as serialized.
    pub fn as_str(self) -> &'static str {
        match self {
            Grade::Thin => "thin",
            Grade::Solid => "solid",
            Grade::Sharp => "sharp",
        }
    }

    /// Display title.
    pub fn title(self) -> &'static str {
        match self {
            Grade::Thin => "Thin",
            Grade::Solid => "Solid",
            Grade::Sharp => "Sharp",
        }
    }
}

/// Why a task needed another message: the AI was wrong, or the brief left something out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Steer {
    /// You corrected it (any catch).
    AiWrong,
    /// You added a requirement after the fact ("also…", "I forgot to mention…").
    Added,
    /// You explained what you meant ("I meant…", "to clarify…").
    Clarified,
    /// You threw the work away ("start over", "revert that").
    Redo,
}

impl Steer {
    /// Every cause, in display order.
    pub const ALL: [Steer; 4] = [Steer::AiWrong, Steer::Added, Steer::Clarified, Steer::Redo];

    /// Stable id, as stored and serialized.
    pub fn as_str(self) -> &'static str {
        match self {
            Steer::AiWrong => "ai_wrong",
            Steer::Added => "added",
            Steer::Clarified => "clarified",
            Steer::Redo => "redo",
        }
    }

    /// Display title.
    pub fn title(self) -> &'static str {
        match self {
            Steer::AiWrong => "It got it wrong",
            Steer::Added => "You added more",
            Steer::Clarified => "You clarified",
            Steer::Redo => "You started over",
        }
    }

    /// True when the brief, not the AI, caused it.
    pub fn is_on_you(self) -> bool {
        !matches!(self, Steer::AiWrong)
    }
}

/// What one message did to the work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Prompt {
    /// Started a task.
    Brief {
        /// The specifics it carries.
        signals: Signals,
        /// Ids of the rules that fired: what started the task, then one per signal.
        rules: Vec<&'static str>,
    },
    /// Redirected the task under way.
    Steer {
        /// Why.
        cause: Steer,
        /// The rule that fired.
        rule: &'static str,
    },
    /// Anything else: answering the AI, approving, asking, chatting.
    Other,
}

/// Reads one message. `verdict` is [`crate::classify`]'s for the same turn: a catch is steering.
/// `has_task` says whether this chat already has a task; until it does, the first real request
/// (not a hello or a question) is the brief, wherever it falls.
pub fn read_prompt(turn: &Turn, verdict: &Verdict, has_task: bool) -> Prompt {
    let norm = normalize_quotes(turn.human.trim());
    let t = norm.as_str();
    if t.is_empty() || t.starts_with('<') || SLASH_COMMAND.is_match(t) {
        return Prompt::Other; // tool payloads and slash commands: nobody wrote these as a brief
    }
    let head = head_chars(t, HEAD);
    if verdict.label.is_some_and(Label::is_catch) {
        return Prompt::Steer { cause: Steer::AiWrong, rule: verdict.rules.first().copied().unwrap_or("catch") };
    }
    // A chat's very first message can't steer anything, so it's never read as steering.
    if turn.prev_ai.is_some() {
        let steer = [(Steer::Redo, &*REDO), (Steer::Clarified, &*CLARIFY), (Steer::Added, &*ADDED)]
            .into_iter()
            .find_map(|(cause, rules)| first(rules, head).map(|m| Prompt::Steer { cause, rule: m.rule }));
        if let Some(steer) = steer {
            // With no task yet there's nothing to steer ("I meant…" about an answer): not a brief either.
            return if has_task { steer } else { Prompt::Other };
        }
    }
    if !has_task || turn.prev_ai.is_none() {
        if first(&CHATTER, t).is_some() || is_question(t) {
            return Prompt::Other;
        }
        return brief(t, "task.first");
    }
    if let Some(m) = first(&NEW_TASK, head) {
        return brief(t, m.rule);
    }
    Prompt::Other
}

fn brief(t: &str, started_by: &'static str) -> Prompt {
    let mut signals = Signals::default();
    let mut rules = vec![started_by];
    // Pasted material shows the problem, but its words aren't yours: the rest is graded without it.
    let own = PASTED.replace_all(head_chars(t, BRIEF), " ");
    if own.len() < head_chars(t, BRIEF).len() {
        signals.insert(Signal::Proof);
        rules.push("brief.proof.pasted");
    }
    let text = own.as_ref();
    for s in Signal::ALL {
        if signals.has(s) {
            continue;
        }
        if let Some(m) = first(s.rules(), text) {
            signals.insert(s);
            rules.push(m.rule);
        }
    }
    Prompt::Brief { signals, rules }
}

/// "What does this do?" and "explain X": asking, not handing over work.
fn is_question(t: &str) -> bool {
    first(&EXPLAIN, t).is_some() || (t.ends_with('?') && first(&QUESTION, t).is_some())
}

/// "/review", "/code-review high": a command, not a path like "/Users/me/x.ts".
static SLASH_COMMAND: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^/[\w:-]+(?:\s|$)").expect("slash"));

/// Pasted blocks and fenced code (an unclosed fence runs to the end of the text).
static PASTED: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?s)<pasted_content\b.*?(?:</pasted_content>|$)|```.*?(?:```|$)").expect("pasted")
});

/// Wrapping-up steps ("also commit and push", "make sure to run the tests"): routine, not
/// something the brief left out.
const WORKFLOW: &str = r"(?:please\s+)?(?:commit|push|merge|deploy|ship|release|publish|run (?:the )?(?:tests?|build|lint|checks?)|test (?:it|this|that)|open (?:a |the )?(?:pr|pull request)|create (?:a |the )?(?:pr|pull request))\b";

/// An optional acknowledgement before the words that matter: "ok, also…", "thanks! now…".
const ACK: &str = r"^(?:(?:ok|okay|k|great|thanks|thank you|thx|perfect|nice|cool|awesome|good|lgtm|looks good|sweet|excellent|amazing|got it|alright|all right|yes|yep|yeah|sure|done|works|that works|it works|love it)\s*[,.!:;—–-]*\s+)?";

/// A whole message that is only a greeting, an approval or "keep going".
static CHATTER: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![rule(
        "chat.ack",
        r"^(?:hi|hello|hey|yo|thanks?|thank you|thx|ok|okay|k|yes|yep|yeah|no|nope|sure|y|n|continue|go on|keep going|go ahead|proceed|resume|carry on|do it|go|lgtm|ship it|test|testing|ping|\.)\W*$",
    )]
});

static QUESTION: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![rule_unless(
        "chat.question",
        r"^(?:what|why|how|when|where|which|who|whose|is|are|was|were|does|do|did|has|have|should i|would it|will it|what'?s|whats|how'?s|where'?s|who'?s)\b",
        // "why don't you…?" and "how about we…?" are requests.
        r"(?:why (?:don'?t|not) (?:you|we)|how about (?:you|we)|is it possible (?:to|for you))\b",
    )]
});

static EXPLAIN: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![rule(
        "chat.explain",
        r"^(?:explain|tell me (?:about|what|why|how)|describe|define|summari[sz]e|tldr|eli5|(?:can|could|would) you (?:explain|tell me|help me understand|walk me through|describe|summari[sz]e|clarify))\b",
    )]
});

/// You threw the work away.
static REDO: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![rule_unless(
        "steer.redo",
        &format!(
            r"{ACK}(?:(?:no|ugh|hmm|actually|nah)[,.!]*\s+)?(?:let'?s |please |just )?(?:start (?:over|again|from scratch|fresh)|scrap (?:that|this|it|all|everything)|throw (?:that|this|it) (?:away|out)|undo (?:that|this|it|everything|all|those|these|the|your)|revert (?:that|this|it|everything|all|those|these|the|your)|roll (?:it |that |this |everything )?back|back (?:that|it|this) out|discard (?:that|this|it|those|these|the|your)|forget (?:that|it|all that|what i said|about (?:it|that))|go back to (?:what|how|where) (?:it|we|you|i|they)\b|go back to (?:the )?(?:old|previous|original|last|earlier|prior) |go back to before|try (?:a different|another) (?:approach|way)|let'?s try (?:something else|a different|another)|wrong approach)\b"
        ),
        // "forget it, it works": the problem went away, nothing was thrown out.
        &format!(
            r"{ACK}(?:forget (?:it|that))\b[,.!]*\s*(?:(?:it|that|this|i|we)(?:'?s| is| was)?\s+)?(?:works?|working|fine|ok|okay|good|got it|found it|figured|fixed|sorted|solved)"
        ),
    )]
});

/// You explained what you meant.
static CLARIFY: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        rule(
            "steer.clarify.meant",
            &format!(
                r"{ACK}(?:(?:no|sorry|oh|ah|nope|wait|hmm)[,.!]*\s+)?(?:i meant|what i meant|i was (?:asking|talking) about|i'?m (?:asking|talking) about|i was referring|i'?m referring|my point (?:is|was)|let me (?:clarify|rephrase|be (?:more )?clear(?:er)?)|to (?:clarify|be (?:more )?(?:clear|specific)|rephrase)|in other words|sorry,? (?:i wasn'?t clear|that was unclear|i should have been clearer|for the confusion)|not (?:that|this) one|the other one)"
            ),
        ),
        rule(
            "steer.clarify.unclear",
            r"\bi should have been (?:clearer|more (?:clear|specific))\b|\bi wasn'?t clear\b",
        ),
    ]
});

/// You added a requirement the brief left out.
static ADDED: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        rule_unless(
            "steer.added.also",
            &format!(
                r"{ACK}(?:also|oh,? and|ah,? and|and also|btw|by the way|fyi|one more thing|one other thing|another thing|additionally|in addition|plus,|p\.?s\.?)(?:\W|$)"
            ),
            &format!(r"{ACK}(?:also|and also|plus,)\W*{WORKFLOW}"),
        ),
        rule(
            "steer.added.forgot",
            r"\bi (?:forgot|neglected) to (?:mention|say|tell|add|include|note|specify)\b|\bforgot to mention\b|\bi should have (?:mentioned|said|told|added|specified)\b|\bi didn'?t mention\b",
        ),
        rule_unless(
            "steer.added.make_sure",
            &format!(
                r"{ACK}(?:make sure|don'?t forget|remember to|be sure to|please also|can you also|could you also|you should also|also make sure)\b"
            ),
            &format!(
                r"{ACK}(?:make sure|don'?t forget|remember to|be sure to|please also|can you also|could you also|you should also|also make sure)\W*(?:(?:to|you|that you)\s+)?{WORKFLOW}"
            ),
        ),
    ]
});

/// A fresh request partway through a chat.
static NEW_TASK: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        rule(
            "task.now",
            &format!(
                r"{ACK}(?:now|next|and now|ok now)[,:]?\s+(?:let'?s|can you|could you|would you|please|i want|i'?d like|we need|i need|add|make|write|create|implement|build|update|change|remove|delete|refactor|move|rename|set up|wire|hook|do|fix|handle|look|check|give|show|help|try|switch|convert|clean|split|draft|design|plan|review|audit|document|research|find|investigate|port|migrate|upgrade|bump|optimi[sz]e|improve|extract|generate)\b"
            ),
        ),
        rule(
            "task.new",
            &format!(
                r"{ACK}(?:new|next|another|different|separate|unrelated) (?:task|thing|feature|bug|issue|request|item|topic|step|project|job|ask|change)\b"
            ),
        ),
        rule(
            "task.moving_on",
            &format!(
                r"{ACK}(?:moving on|next up|switching (?:gears|topics?)|changing (?:gears|topics?)|unrelated[,:]|on (?:another|a different|an unrelated) note|separately[,:]|different topic|let'?s (?:move on|switch)|on to (?:the next|something))"
            ),
        ),
    ]
});

/// Names the place.
static SPOT: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        rule(
            "brief.spot.file",
            r"\b[\w-]+\.(?:rs|ts|tsx|js|jsx|mjs|cjs|py|go|java|kt|kts|swift|rb|php|cs|cpp|cc|c|h|hpp|css|scss|sass|less|html?|md|mdx|json|jsonl|ya?ml|toml|ini|cfg|conf|sql|sh|bash|zsh|ps1|vue|svelte|astro|lock|env|txt|csv|tsv|xml|gradle|tf|proto|graphql|gql|ipynb|plist|xcconfig|dart|lua|ex|exs|scala)\b|(?:^|[\s(`'])\.(?:env|gitignore|npmrc|nvmrc|eslintrc|prettierrc|editorconfig|github|vscode)\b|(?-i:\b(?:Dockerfile|Makefile|Justfile|justfile|Gemfile|Procfile|README|CHANGELOG)\b)",
        ),
        rule("brief.spot.dir", r"(?:^|[\s(\[`'])(?:~|\.{1,2})?/?[A-Za-z_][\w.-]*/[\w.-]+/"),
        rule("brief.spot.mention", r"(?:^|\s)@[\w-]*[/.][\w./-]+"),
        rule("brief.spot.url", r"\bhttps?://\S+|\blocalhost:\d+|\bwww\.\S+\.\w+"),
        rule("brief.spot.code", r"`[^`\n]{2,80}`"),
        rule_unless(
            "brief.spot.ident",
            r"(?-i:\b[a-z][a-z0-9]+(?:[A-Z][a-z0-9]+)+\b|\b[A-Z][a-z0-9]+(?:[A-Z][a-z0-9]+)+\b|\b[a-z][a-z0-9]*_[a-z0-9_]+\b|\b[A-Z][A-Z0-9]*_[A-Z0-9_]+\b)|\w\(\)|\.\w+\(",
            // An error's name ("TypeError") shows the problem, and a product's name ("GitHub") is
            // a word; neither points at a place in the code.
            r"(?-i:[A-Z]\w*(?:Error|Exception)\b|(?:TypeScript|JavaScript|CoffeeScript|GitHub|GitLab|BitBucket|YouTube|LinkedIn|WhatsApp|PowerPoint|PostgreSQL|MySQL|MongoDB|DynamoDB|WordPress|PayPal|ChatGPT|OpenAI|DeepSeek|FaceTime|AirPods|AirDrop|DevTools|VSCode|IntelliJ|PyCharm|WebStorm|TikTok|HubSpot|QuickBooks|SharePoint|OneDrive|DropBox|DoorDash|StackOverflow|CloudFlare|DigitalOcean|SoundCloud|MacBook|AppleScript|PlayStation)\b)",
        ),
        rule("brief.spot.line", r"\blines? \d+\b"),
        rule(
            "brief.spot.cli",
            r"(?:^|\s)--[a-z][\w-]+|\b(?:npm|pnpm|yarn|bun|cargo|git|pip3?|brew|docker|kubectl|npx|uv|gh|terraform|rustup|deno) (?:install|i|run|test|build|add|remove|publish|init|ci|exec|start|dev|update|upgrade|status|log|diff|commit|push|pull|rebase|merge|checkout|switch|branch|stash|reset|clone|fetch|tag|cherry-pick|bisect|blame|compose|ps|logs|apply|get|describe|plan|fmt|clippy|check|doc|bench|lint|pr|issue|release|login|tap|outdated|audit|why|dlx|link)\b|\b(?:pytest|jest|vitest) ",
        ),
    ]
});

/// Shows the problem.
static PROOF: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        rule(
            "brief.proof.error",
            r"(?-i:\b[A-Z]\w*(?:Error|Exception)\b)|(?m:^\s*(?:error|warning|fatal|panic|exception|traceback|caused by|err!?)\b[^\n]{0,40}:)|\berror\[E\d{3,4}\]|\bTS\d{4}\b",
        ),
        rule(
            "brief.proof.trace",
            r"\bat [\w.<>$]+ \(?[^\s()]+:\d+(?::\d+)?\)?|\bpanicked at\b|\btraceback \(most recent call last\)|\bline \d+, in\b|\bexit (?:code|status) \d+|\bstatus (?:code )?[1-5]\d\d\b|\b[45]\d\d (?:error|response|bad request|not found|unauthorized|forbidden)\b|\bsegmentation fault\b|\bcommand not found\b|\bno such file or directory\b|\bcannot find module\b|\bundefined is not\b|\bis not defined\b|\bis not a function\b|\bnull pointer\b|\bpermission denied\b",
        ),
        rule(
            "brief.proof.expected",
            r"\bexpected\b[^.\n]{1,120}\b(?:but|got|instead)\b|\bbut (?:it |instead )?(?:got|gets|shows|returns|says|prints|gives|outputs|displays|renders|throws)\b|\b(?:actual|current) (?:result|output|behaviou?r)\b",
        ),
        rule(
            "brief.proof.example",
            r"\be\.g\.|\b(?:for example|for instance|such as|like this|example:|sample:|here'?s (?:an|the|my) (?:example|output|error|log|screenshot|trace)|here is (?:an|the|my) (?:example|output|error|log|screenshot|trace))",
        ),
        rule("brief.proof.quote", r#""[^"\n]{6,200}""#),
        rule(
            "brief.proof.attached",
            r"\[(?:image|pasted text) #\d+|\b(?:see|attached|in) (?:the )?(?:screenshot|image|screen ?shot|logs?|output|trace)\b",
        ),
    ]
});

/// Says what done looks like.
static DONE: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        rule_unless(
            "brief.done.should",
            r"\b(?:should|must|needs? to|has to)\b",
            // "should I…?" asks; "should be fine" shrugs. Neither says what done is.
            // "has to do with" and "needs to be fixed" say nothing about the result either.
            r"(?:should|must|needs? to|has to)\s+(?:i|we|you)\b|(?:should|must)\s+(?:probably\s+)?be\s+(?:fine|ok|okay|good|easy|simple)\b|has to do with\b|needs? to be (?:fixed|done|changed|updated|looked at|addressed|sorted|handled)\b",
        ),
        rule(
            "brief.done.criteria",
            r"\b(?:done when|is done when|definition of done|acceptance criteria|success (?:looks like|means|criteria)|the result should|end result|expected (?:output|result|behaviou?r)|desired (?:output|result|behaviou?r)|so (?:that )?(?:i|we|users?|people) can)\b",
        ),
        rule(
            "brief.done.tests",
            r"\b(?:add|write|update|include|with) (?:a |an |some |the |new )?(?:unit |integration |e2e |regression |snapshot )?tests?\b|\btests? (?:pass|passes|passing|green|still pass)\b|\b(?:build|ci|lint|typecheck|type-check|checks?) (?:pass|passes|is green|are green|succeeds?|clean)\b|\bmake sure (?:it|that|the|all|tests?|nothing|everything)\b|\bverify (?:that|it|the)\b|\bconfirm (?:that|it)\b|\buntil (?:it|the tests?|all|everything) (?:pass|passes|works|is green)\b",
        ),
        rule(
            "brief.done.format",
            r"\bas an? (?:table|list|bulleted list|json|csv|markdown|diagram|chart|pr|pull request|single commit|commit|script|function|component|class|hook|module|cli|one-liner|checklist|summary)\b|\bin (?:json|yaml|csv|markdown|a table|one file|a single file|bullet points)\b|\b(?:one|a single|separate) (?:pr|pull request|commit|file|function|component)\b|\b(?:under|less than|fewer than|at most|no more than|max(?:imum)?|exactly|within) \d+(?:\.\d+)?\b|\b\d+(?:\.\d+)? ?(?:ms|px|rem|kb|mb|gb|fps)\b",
        ),
        // Two list items close together: a list of requirements.
        rule(
            "brief.done.list",
            r"(?m)^[ \t]*(?:[-*•]|\d{1,2}[.)])[ \t]+\S[^\n]*\n(?:[^\n]*\n){0,5}?[ \t]*(?:[-*•]|\d{1,2}[.)])[ \t]+\S",
        ),
    ]
});

/// Sets limits.
static LIMITS: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        rule_unless(
            "brief.limits.dont",
            r"(?m)(?:^|[.!?;:,(—–-]\s*|\b(?:please|but|and|just|also|then|so)\s+)(?:don'?t|do not|never|avoid|no need to)\s",
            // "…, don't know why": a shrug, not a rule.
            r"[^a-z]*(?:\w+\s+)?(?:don'?t|do not)\s+(?:know|understand|think|remember|seem|get it|work(?:\s*(?:[.!,;)]|$)|\s+(?:anymore|right|properly|well|at all|for me)))",
        ),
        rule(
            "brief.limits.dont_want",
            r"\bi (?:don'?t|do not) want (?:any|you to|it to|a |an |the |new|extra|to)|\b(?:don'?t|do not) (?:want|need) (?:any|new|extra|another)\b",
        ),
        rule(
            "brief.limits.only",
            r"\bonly (?:change|touch|modify|edit|update|use|return|show|include|fix|add|affect|apply|target)\b|\b(?:change|touch|modify|edit|fix|update) only\b|\bjust (?:the|this|that) (?:one|file|function|part|section|page|line)\b|\bnothing else\b",
        ),
        rule(
            "brief.limits.keep",
            r"\b(?:keep|preserve|retain|maintain|leave) (?:the|it|this|that|existing|current|all|everything|any|its|our|my|them|backwards?|api|tests?|behaviou?r|style|format|formatting|layout|order|names?|signatures?)\b|\bleave [^.\n]{1,30} alone\b|\bas[- ]is\b|\bunchanged\b|\bbackwards?[- ]compatib",
        ),
        rule(
            "brief.limits.without",
            r"\bwithout (?:breaking|changing|touching|adding|removing|using|modifying|affecting|introducing|rewriting|losing|a new|new|any|extra|another)\b",
        ),
        rule(
            "brief.limits.no",
            r"\bno (?:new |extra |additional |external |third[- ]party )?(?:dependencies|deps|packages|libraries|libs|frameworks|breaking changes|migrations|comments|emojis?|placeholders|mocks|todos?)\b|\b(?:must not|mustn'?t|should not|shouldn'?t|cannot|can'?t|may not) (?:change|break|touch|use|add|remove|affect|modify|rely|depend|exceed|call|import|be)\b|\b(?:out of scope|scope (?:it|this) to|limit(?:ed)? (?:it |this |the change )?to|stick (?:to|with)|stay within|no more than)\b",
        ),
    ]
});

/// Says why.
static WHY: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        rule(
            "brief.why.because",
            r"\b(?:because|since (?:we|i|they|users?|our \w+) (?:need|want|can'?t|cannot|don'?t|use|have|rely|depend|ship|support)|so that|in order to|that way)\b",
        ),
        rule(
            "brief.why.goal",
            r"\b(?:the|my|our) (?:goal|point|reason|idea|aim|purpose|plan|intent|use case|problem|issue) (?:is|was|here)\b|(?m:^\s*(?:goal|context|background|why|problem|motivation|use case)\s*:)|\b(?:i'?m|we'?re) trying to\b|\bi want (?:to be able|users|people|it to)\b|\bwe (?:want|need) (?:this|it|to)\b|\bi need (?:this|it) (?:to|for|so)\b",
        ),
        rule(
            "brief.why.who",
            r"\b(?:our|my) (?:users?|customers?|clients?|team|boss|manager|designer|pm|stakeholders?|visitors|readers|audience)\b|\b(?:users?|customers?|people) (?:need|want|can'?t|cannot|keep|report|complain|expect)\b|\bfor (?:the|our|a|my) (?:demo|launch|release|client|customer|presentation|meeting|deadline|portfolio|interview)\b",
        ),
        rule("brief.why.now", r"\b(?:currently|at the moment|as of now)\b"),
    ]
});

/// How well your briefs land: of the tasks in the last [`WINDOW_DAYS`] that went past their first
/// message, the share you never had to add to, clarify or redo. A one-message task can't need
/// steering, so it doesn't count either way. The AI getting it wrong doesn't count against you
/// (that's a catch). Ordered lowest first.
///
/// The floors come from one heavy user's history (tasks that continued landed 83–90% of the time
/// over 28 days, typically 86%). Public chat logs can't set them: casual chats almost never steer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BriefTier {
    /// Under 70% landed.
    VibesOnly,
    /// 70% or more.
    TicketWriter,
    /// 80% or more.
    TechLead,
    /// 90% or more.
    Architect,
}

/// Days of tasks a brief tier looks at: long enough that one "also…" doesn't swing it.
pub const WINDOW_DAYS: i64 = 28;

/// Tasks that continued, in the window, needed before a tier is given; fewer says too little.
pub const MIN_BRIEFS: i64 = 10;

impl BriefTier {
    /// Every tier, ascending by floor.
    pub const ALL: [BriefTier; 4] =
        [BriefTier::VibesOnly, BriefTier::TicketWriter, BriefTier::TechLead, BriefTier::Architect];

    /// Percent of the window's continued tasks that must land.
    pub const fn floor(self) -> i64 {
        match self {
            BriefTier::VibesOnly => 0,
            BriefTier::TicketWriter => 70,
            BriefTier::TechLead => 80,
            BriefTier::Architect => 90,
        }
    }

    /// Stable id, as stored and serialized.
    pub const fn id(self) -> &'static str {
        match self {
            BriefTier::VibesOnly => "vibes_only",
            BriefTier::TicketWriter => "ticket_writer",
            BriefTier::TechLead => "tech_lead",
            BriefTier::Architect => "architect",
        }
    }

    /// Display name.
    pub const fn name(self) -> &'static str {
        match self {
            BriefTier::VibesOnly => "Vibes only",
            BriefTier::TicketWriter => "Ticket writer",
            BriefTier::TechLead => "Tech lead",
            BriefTier::Architect => "Architect",
        }
    }

    /// The answer to "Do you make it guess?" at this tier.
    pub const fn verdict(self) -> &'static str {
        match self {
            BriefTier::VibesOnly => "Constantly.",
            BriefTier::TicketWriter => "Often.",
            BriefTier::TechLead => "Sometimes.",
            BriefTier::Architect => "Rarely.",
        }
    }

    /// The tier reached when `landed` of `tasks` landed, or `None` below [`MIN_BRIEFS`] tasks.
    pub fn for_tasks(landed: i64, tasks: i64) -> Option<BriefTier> {
        (tasks >= MIN_BRIEFS)
            .then(|| BriefTier::ALL.into_iter().rev().find(|t| landed * 100 >= t.floor() * tasks))
            .flatten()
    }

    /// The tier above this one, if any.
    pub fn next(self) -> Option<BriefTier> {
        BriefTier::ALL.into_iter().find(|t| *t > self)
    }
}

impl std::fmt::Display for BriefTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for BriefTier {
    type Err = crate::UnknownTier;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        BriefTier::ALL.into_iter().find(|t| t.id() == s).ok_or_else(|| crate::UnknownTier(s.to_string()))
    }
}

#[cfg(test)]
#[path = "brief_tests.rs"]
mod tests;
