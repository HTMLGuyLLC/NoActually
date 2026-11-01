//! Behavior tests for prompts and steering. Examples mirror real prompts without copying anyone's.

use super::*;
use crate::{classify, AiMsg, PromptTier};

fn ai(text: &str) -> AiMsg {
    AiMsg { text: text.into(), model: Some("claude-opus-5-5".into()) }
}

/// A message with the AI reply before it (`None`: the chat's first message) and after it.
fn read(prev: Option<&str>, human: &str, next: &str) -> Prompt {
    let turn = Turn {
        human: human.into(),
        prev_ai: prev.map(ai),
        next_ai: Some(ai(next)),
        later_ai: Vec::new(),
        chat_model: None,
    };
    read_prompt(&turn, &classify(&turn), prev.is_some())
}

fn first_message(human: &str) -> Prompt {
    read(None, human, "On it.")
}

fn signals(human: &str) -> Vec<Signal> {
    match first_message(human) {
        Prompt::Task { signals, .. } => Signal::ALL.into_iter().filter(|s| signals.has(*s)).collect(),
        other => panic!("not a prompt: {human:?} -> {other:?}"),
    }
}

fn steer(prev: &str, human: &str) -> Option<Steer> {
    match read(Some(prev), human, "Okay.") {
        Prompt::Steer { cause, .. } => Some(cause),
        _ => None,
    }
}

use Signal::{Done, Limits, Proof, Spot, Why};

/// (prompt, the signals it carries). Each row is one way a person writes; the set must be exact.
const EXAMPLES: &[(&str, &[Signal])] = &[
    // Vague: the AI has to guess.
    ("fix it", &[]),
    ("make the login page better", &[]),
    ("clean this up", &[]),
    ("add dark mode", &[]),
    ("the build is broken", &[]),
    ("I don't know why but the page is slow", &[]),
    ("I keep getting errors", &[]),
    ("set up TypeScript and a GitHub action", &[]),
    ("Git is acting weird and docker is slow", &[]),
    ("send the report at 9 a.m.", &[]),
    ("use pnpm instead of npm", &[]),
    ("I'm 100% sure this has to do with caching", &[]),
    // Where to look once it's done isn't proof of what's wrong.
    (
        "Fix the thing in index.php, but don't touch index2.php and you'll know you fixed it when there's no error in the logs",
        &[Spot, Done, Limits],
    ),
    ("it's done once the page loads in under a second", &[Done]),
    ("we'll know it works if the checkout email arrives", &[Done]),
    ("see the logs, it keeps crashing", &[Proof]),
    // A route points at a page, like a URL does.
    ("fix the bubble at the top of /hi", &[Spot]),
    ("the /api/users endpoint returns 500 error responses", &[Spot, Proof]),
    ("make it short and/or simple, ok?", &[]),
    ("it broke on 10/29", &[]),
    ("make the signup emails friendlier, we want them to come back", &[Why]),
    ("I want you to rename it", &[]),
    ("the login needs to be fixed", &[]),
    ("when the user clicks save, the client sends a bad request", &[]),
    ("Since the last update the app is slow", &[]),
    // Target.
    ("fix the crash in src/auth/session.ts", &[Spot]),
    ("rename getUser to fetchUser", &[Spot]),
    ("bump NODE_ENV handling in the config", &[Spot]),
    ("update `retention_days` in the store", &[Spot]),
    ("look at @app/src/main.ts", &[Spot]),
    ("check https://example.com/docs/setup for the steps", &[Spot]),
    ("the error is on line 42", &[Spot]),
    ("run cargo test and fix what fails", &[Spot]),
    ("the Dockerfile is too slow to build", &[Spot]),
    ("read the .env and wire up the keys", &[Spot]),
    ("rename UserProfile to Account", &[Spot]),
    // Evidence.
    // Pasted blocks show the problem; their words aren't graded as yours.
    ("fix this:\n```\nthread 'main' panicked at src/lib.rs:10, don't panic\n```", &[Proof]),
    ("is this phishing? <pasted_content id=\"1\">Your account.ts must be verified because</pasted_content>", &[Proof]),
    ("```\nunclosed paste that should not count", &[Proof]),
    ("getting TypeError when I click save", &[Proof]),
    ("the page shows a 404 not found after login", &[Proof]),
    ("it says \"Payment method declined\" every time", &[Proof]),
    ("expected the total to update but it stays at zero", &[Proof]),
    ("make the dates look like this: Oct 28, 2026", &[Proof]),
    ("see the screenshot, the header overlaps", &[Proof]),
    ("look at the logs for reference", &[Proof]),
    ("check the output, it's off by one", &[Proof]),
    // Finish line.
    ("the button should redirect to the dashboard", &[Done]),
    ("You should fix it.", &[]),
    ("you must probably look into the crash", &[]),
    ("it needs to be debugged, you have to investigate", &[]),
    ("it should handle empty input", &[Done]),
    ("the total should update when the cart changes", &[Done]),
    (
        "Something here is not right. You should fix it. Don't forget the monkey. Also, I think we need this because it's red. Look at the logs for reference.",
        &[Proof, Limits, Why],
    ),
    ("add a unit test for the parser", &[Done]),
    ("make sure the tests pass", &[Done]),
    ("return the results as a table", &[Done]),
    ("keep the summary under 50 words", &[Done, Limits]),
    ("load in under 200ms", &[Done]),
    ("plan:\n- add the flag\n- wire it to settings\n- document it", &[Done]),
    // Guardrails.
    ("refactor the parser, don't change the public API", &[Limits]),
    ("Do not add new dependencies.", &[Limits]),
    ("only change the CSS", &[Limits]),
    ("speed it up without breaking the cache", &[Limits]),
    ("keep the existing names", &[Limits]),
    ("no new dependencies please", &[Limits]),
    ("I don't want any new dependencies", &[Limits]),
    ("Don't work on main, use a branch", &[Limits]),
    // Purpose.
    ("cache the results because the API is rate limited", &[Why]),
    ("our users can't find the export button", &[Why]),
    ("cache it since we rely on a slow API", &[Why]),
    ("I'm trying to cut the bundle size for the launch", &[Why]),
    ("Goal: one place to see every invoice", &[Why]),
    ("currently the report takes a minute to open", &[Why]),
    // Specific: several at once.
    (
        "In src/checkout.ts the total should include tax. Don't touch the cart API, our customers depend on it.",
        &[Spot, Done, Limits, Why],
    ),
    (
        "Add a test for parse_date in dates.py. It returns None for \"2026-10-28T00:00Z\" but should return a datetime.",
        &[Spot, Proof, Done],
    ),
];

#[test]
fn every_example_carries_exactly_its_signals() {
    let mut wrong = Vec::new();
    for (prompt, want) in EXAMPLES {
        let got = signals(prompt);
        if got != *want {
            wrong.push(format!("{prompt:?}\n   want {want:?}\n   got  {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "{} examples off:\n{}", wrong.len(), wrong.join("\n"));
}

#[test]
fn grades_count_signals_not_words() {
    let g = |p: &str| match first_message(p) {
        Prompt::Task { signals, .. } => Grade::of(signals),
        _ => panic!("not a prompt"),
    };
    assert_eq!(g("fix it"), Grade::Thin);
    let padded = "please please please make the whole thing a lot better and nicer and faster for everyone ".repeat(20);
    assert_eq!(g(&padded), Grade::Thin, "length alone earns nothing");
    assert_eq!(g("only change header.css"), Grade::Solid);
    assert_eq!(g(EXAMPLES[EXAMPLES.len() - 1].0), Grade::Sharp);
    assert_eq!(Grade::of(Signals(0b11111)), Grade::Sharp);
}

#[test]
fn signal_bits_never_move() {
    // Masks are stored; renumbering would regrade history.
    let bits: Vec<u8> = Signal::ALL.iter().map(|s| s.bit()).collect();
    assert_eq!(bits, [1, 2, 4, 8, 16]);
    let mut s = Signals::default();
    s.insert(Done);
    s.insert(Why);
    assert_eq!((s.0, s.count(), s.has(Done), s.has(Spot)), (20, 2, true, false));
}

#[test]
fn until_a_chat_has_a_task_its_first_request_is_the_prompt() {
    let msg = |human: &str, has_task: bool| {
        let turn = Turn {
            human: human.into(),
            prev_ai: Some(ai("Hi, here is the overview.")),
            next_ai: None,
            later_ai: Vec::new(),
            chat_model: None,
        };
        read_prompt(&turn, &classify(&turn), has_task)
    };
    // After a hello or a question, the real request is the prompt.
    assert!(matches!(msg("add a CSV export to the report page", false), Prompt::Task { .. }));
    assert_eq!(msg("add a CSV export to the report page", true), Prompt::Other, "mid-task, it's part of the task");
    assert_eq!(msg("thanks", false), Prompt::Other);
    assert_eq!(msg("I meant the other file", false), Prompt::Other, "nothing to steer, and not a request");
    assert!(matches!(first_message("make sure the tests pass after the upgrade"), Prompt::Task { .. }));
    // A correction is still a correction, task or not.
    assert!(matches!(msg("that's wrong", false), Prompt::Steer { cause: Steer::AiWrong, .. }));
}

#[test]
fn a_chats_first_message_is_its_prompt() {
    assert!(matches!(first_message("add a CSV export"), Prompt::Task { .. }));
    let Prompt::Task { rules, .. } = first_message("add a test for parse_date") else { panic!() };
    assert_eq!(rules, ["task.first", "prompt.spot.ident", "prompt.done.tests"], "says what started it, then why");
}

#[test]
fn only_a_chats_first_request_says_so() {
    assert!(first_message("add a CSV export").is_first_request());
    let p = read(Some("Done."), "now add a JSON export too", "On it.");
    assert!(matches!(p, Prompt::Task { .. }) && !p.is_first_request(), "a new task is not the first request");
    assert!(!Prompt::Other.is_first_request());
}

#[test]
fn hellos_questions_and_payloads_are_not_prompts() {
    for p in [
        "hi",
        "continue",
        "Thanks!",
        "what does the sync step do?",
        "How do I run the tests?",
        "is there a way to skip the hook?",
        "explain the retention levels",
        "can you explain what this regex matches",
        "<command-name>/clear</command-name>",
        "/review",
        "   ",
    ] {
        assert_eq!(first_message(p), Prompt::Other, "{p:?}");
    }
    // Requests phrased as questions are still requests.
    for p in [
        "can you add a dark mode toggle?",
        "why don't you split it into two files?",
        "could you fix the footer",
        "Can I get a script that renames files?",
        "Is it possible to add dark mode?",
        "/Users/me/app/src/a.ts is broken",
    ] {
        assert!(matches!(first_message(p), Prompt::Task { .. }), "{p:?}");
    }
}

#[test]
fn corrections_are_the_ais_fault() {
    let p = read(Some("Fixed the layout."), "still not working on safari", "Checking.");
    assert_eq!(p, Prompt::Steer { cause: Steer::AiWrong, rule: "you.still" });
    assert_eq!(steer("Here's the query.", "that's wrong, it should group by day"), Some(Steer::AiWrong));
}

#[test]
fn a_chats_first_message_cant_be_a_correction_and_keeps_its_task() {
    let human = "still broken: the login page in src/app.ts shows a blank screen";
    let turn = |prev: Option<&str>| Turn {
        human: human.into(),
        prev_ai: prev.map(ai),
        next_ai: Some(ai("Looking now.")),
        later_ai: Vec::new(),
        chat_model: None,
    };
    let first = turn(None);
    assert_eq!(classify(&first).label, None, "nothing came before it to correct");
    assert!(matches!(read_prompt(&first, &classify(&first), false), Prompt::Task { .. }));
    // Near miss: the same words after a reply are a correction, and steer that task.
    let later = turn(Some("Fixed the login page."));
    assert!(classify(&later).label.is_some_and(Label::is_correction));
    assert_eq!(steer("Fixed the login page.", human), Some(Steer::AiWrong));
}

#[test]
fn steering_that_is_on_you() {
    let cases = [
        ("Done, added the export.", "also, it needs to work on Windows", Steer::Added),
        ("Done.", "oh and the date column should be ISO", Steer::Added),
        ("Here it is.", "I forgot to mention we use Postgres", Steer::Added),
        ("Done.", "ok, make sure it handles empty files too", Steer::Added),
        ("Done.", "can you also add a --json flag", Steer::Added),
        ("I changed the header.", "I meant the footer, not the header", Steer::Clarified),
        ("Updated the chart.", "sorry, I wasn't clear: weekly, not daily", Steer::Clarified),
        ("Updated the copy.", "to clarify, only the hero text", Steer::Clarified),
        ("Here's the refactor.", "scrap that, let's keep the old structure", Steer::Redo),
        ("Done.", "revert that", Steer::Redo),
        ("Done.", "ugh, start over with a simpler approach", Steer::Redo),
        ("Done.", "let's try a different approach", Steer::Redo),
        ("Done.", "go back to the old version", Steer::Redo),
        ("Done.", "also make it work offline", Steer::Added),
    ];
    for (prev, human, want) in cases {
        assert_eq!(steer(prev, human), Some(want), "{human:?}");
        assert!(want.is_on_you());
    }
    assert!(!Steer::AiWrong.is_on_you());
}

#[test]
fn ordinary_replies_are_not_steering() {
    for (prev, human) in [
        ("Should I ship it today?", "yes, go ahead"),
        ("Want me to add tests?", "yes please"),
        ("Done.", "thanks, that works"),
        ("Done.", "never mind, it works now"),
        ("Done.", "nvm, I figured it out"),
        ("Done.", "nevermind"),
        ("Done.", "forget it, it works"),
        ("Done.", "update the docs to be more clear"),
        ("Done.", "That's exactly what I meant, thanks!"),
        ("Done.", "yes, that's what I was asking for"),
        ("Done.", "perfect, that's what i meant"),
        ("Done.", "I mean, it works great"),
        ("Done.", "also commit and push"),
        ("Done.", "can you also run the tests"),
        ("Done.", "make sure to commit"),
        ("Done.", "go back to the README and add a usage section"),
        ("Done.", "ok then make it blue"),
        ("Done.", "now run the tests"),
        ("Which database do you use?", "Postgres 16"),
        ("Done.", "what does the second flag do?"),
        ("Done.", "looks good"),
        ("Done.", "I mostly agree with the plan"),
    ] {
        assert_eq!(read(Some(prev), human, "Okay."), Prompt::Other, "{human:?}");
    }
}

#[test]
fn a_new_task_partway_through_a_chat() {
    let starts = |human: &str| match read(Some("Done, the export works."), human, "On it.") {
        Prompt::Task { rules, .. } => Some(rules[0]),
        _ => None,
    };
    assert_eq!(starts("great, now add a settings page"), Some("task.now"));
    assert_eq!(starts("Next: write the release notes"), Some("task.now"));
    assert_eq!(starts("new task: the importer drops rows"), Some("task.new"));
    assert_eq!(starts("moving on, the login page needs work"), Some("task.moving_on"));
    // Without a marker, a mid-chat message is part of the task under way, however long.
    let long = "The invoices page in src/pages/invoices.tsx loads every row at once. It should paginate at 50 rows \
                because our biggest customers have thousands of invoices and the page freezes for them.";
    assert_eq!(starts(long), None);
    assert_eq!(starts("now it shows a blank page"), None, "a report, not a new task");
}

#[test]
fn prompt_tiers_follow_the_share_of_tasks_that_landed() {
    let at = PromptTier::for_tasks;
    assert_eq!(at(9, 9), None, "too few tasks to say");
    assert_eq!(at(0, 10), Some(PromptTier::VibesOnly));
    assert_eq!(at(6, 10), Some(PromptTier::VibesOnly));
    assert_eq!(at(7, 10), Some(PromptTier::TicketWriter));
    assert_eq!(at(86, 100), Some(PromptTier::TechLead), "a typical heavy user");
    assert_eq!(at(9, 10), Some(PromptTier::Architect));
    assert!(PromptTier::ALL.windows(2).all(|w| w[0] < w[1] && w[0].floor() < w[1].floor()));
    for t in PromptTier::ALL {
        assert_eq!(t.id().parse(), Ok(t));
        assert_eq!(serde_json::to_string(&t).unwrap(), format!("\"{}\"", t.id()));
    }
    assert_eq!((PromptTier::TechLead.next(), PromptTier::Architect.next()), (Some(PromptTier::Architect), None));
}

#[test]
fn a_vetoed_match_lets_a_later_one_count() {
    assert_eq!(signals("getting TypeError in getUser"), [Spot, Proof]);
    assert_eq!(signals("I don't know why. Don't touch the router though"), [Limits]);
}

#[test]
fn a_sign_off_is_the_whole_message() {
    for t in
        ["thanks", "Thank you!", "perfect, thanks 🙏", "lgtm", "Looks good to me.", "that works, thx", "great work"]
    {
        assert!(is_signoff(t), "{t}");
    }
    // Answers mid-task, more work, or thanks with a new ask aren't sign-offs.
    for t in [
        "yes",
        "ok",
        "sure",
        "commit it",
        "thanks, now add tests",
        "perfect, also fix the header",
        "great question",
        "",
    ] {
        assert!(!is_signoff(t), "{t}");
    }
    // Past the cap it never is one, even if every word would be.
    assert!(is_signoff(&["thanks"; 20].join(" ")));
    assert!(!is_signoff(&["thanks"; 40].join(" ")), "over 200 bytes: not a sign-off");
}

#[test]
fn a_finish_line_isnt_ticked_by_a_sentence_still_being_typed() {
    let done = |text: &str| check(text).signals.has(Signal::Done);
    for text in ["We need to", "we need to f", "we need to fi", "we need to fix", "we need to fix it.", "it must"] {
        assert!(!done(text), "{text}");
    }
    for text in ["the page should load in a second", "it should be blue", "tests must pass", "it has to go live"] {
        assert!(done(text), "{text}");
    }
}
