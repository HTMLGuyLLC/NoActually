//! Behavior tests for the rule book. Examples are written to mirror real catches without
//! copying anyone's chats.

use super::*;

fn turn(prev: &str, human: &str, next: &str) -> Turn {
    Turn {
        human: human.into(),
        prev_ai: Some(AiMsg { text: prev.into(), model: Some("claude-opus-5-5".into()) }),
        next_ai: Some(AiMsg { text: next.into(), model: Some("claude-opus-5-5".into()) }),
        chat_model: None,
    }
}

fn label(prev: &str, human: &str, next: &str) -> Option<Label> {
    classify(&turn(prev, human, next)).label
}

#[test]
fn confirmed_when_both_sides_agree() {
    let v = classify(&turn(
        "The settings live at /admin/teams.",
        "that's not a real page",
        "You're right, I said that carelessly. That route doesn't exist.",
    ));
    assert_eq!(v.label, Some(Label::Confirmed));
    assert_eq!(v.category, Some(Category::MadeItUp));
    assert_eq!(v.rules, vec!["ai.youre_right", "you.wrong"]);
}

#[test]
fn admitted_when_only_the_ai_concedes() {
    assert_eq!(
        label(
            "I'll add a new Button component.",
            "don't create a new component, we have one already",
            "You're right, I duplicated it."
        ),
        Some(Label::Admitted)
    );
    assert_eq!(label("Done.", "look it up", "Found it. I was wrong earlier about the lock."), Some(Label::Admitted));
}

#[test]
fn claimed_when_you_push_back_alone() {
    assert_eq!(
        label("Fixed the layout.", "still not working on safari", "Checking the deploy first."),
        Some(Label::Claimed)
    );
    assert_eq!(label("Updated.", "you missed the other social links", "Adding them now."), Some(Label::Claimed));
}

#[test]
fn answering_a_question_is_not_a_claim() {
    assert_eq!(label("Should I ship it today?", "no, wait until tomorrow", "Okay, holding."), None);
}

#[test]
fn bare_no_is_not_a_rejection() {
    assert_eq!(label("Done.", "no one had android installed, so skip it", "Skipping."), None);
    assert_eq!(label("Done.", "no promise their features stay the same", "Noted."), None);
}

#[test]
fn context_is_never_a_catch() {
    assert_eq!(label("Here's the script.", "also, I'm on Python 3.8", "You're right to mention it."), None);
    assert_eq!(label("Here it is.", "I forgot to mention we use Postgres", "Good catch, switching."), None);
    assert_eq!(label("Here it is.", "thanks, that works", "Great."), None);
}

#[test]
fn humbled_when_the_ai_corrects_you() {
    let v = classify(&turn("Done.", "this tool has no ui right?", "Actually it does. There's a web UI flag."));
    assert_eq!(v.label, Some(Label::Humbled));
    assert_eq!(v.category, None);
}

#[test]
fn status_questions_are_not_humbling() {
    assert_eq!(label("Working on it.", "did you finish?", "Not quite, two tests left."), None);
}

#[test]
fn tool_payloads_and_pastes_are_skipped() {
    assert_eq!(label("x", "<agent-message>You're right</agent-message>", "You're right."), None);
    let paste = format!("no, that's wrong {}", "spec text ".repeat(300));
    assert_eq!(label("x", &paste, "Thanks for the spec."), None);
}

#[test]
fn curly_quotes_match() {
    assert_eq!(label("x", "that’s not right", "You’re right, fixing."), Some(Label::Confirmed));
}

#[test]
fn categories() {
    let cat = |h: &str, n: &str| classify(&turn("x", h, n)).category;
    assert_eq!(cat("I said use the existing table", "You're right."), Some(Category::IgnoredInstructions));
    assert_eq!(cat("it's still broken", "You're right, the error persists."), Some(Category::StillBroken));
    assert_eq!(cat("you forgot the tests", "My mistake."), Some(Category::MissedSomething));
    assert_eq!(cat("why that one?", "My mistake, I misread your request."), Some(Category::Misread));
    assert_eq!(cat("we use postgres", "You're right, let me confirm."), Some(Category::JustWrong));
}

#[test]
fn the_ais_admission_does_not_pick_the_category() {
    let cat = |h: &str, n: &str| classify(&turn("x", h, n)).category;
    // "error" in "My error" is the apology, not a broken build.
    assert_eq!(cat("that's wrong", "My error, I used the old name."), Some(Category::JustWrong));
    assert_eq!(cat("that's wrong", "I made an error in the sum."), Some(Category::JustWrong));
    // The rest of the reply still counts.
    assert_eq!(cat("that's wrong", "My error, the build still fails."), Some(Category::StillBroken));
    assert_eq!(cat("that's wrong", "Good catch, I misread the ticket."), Some(Category::Misread));
}

#[test]
fn misread_is_reachable() {
    let cat = |h: &str, n: &str| classify(&turn("x", h, n)).category;
    assert_eq!(cat("that's not what i meant", "I misread your request."), Some(Category::Misread));
    assert_eq!(cat("that's not what i wanted", "You're right."), Some(Category::Misread));
    assert_eq!(cat("no, I meant the other file", "You're right."), Some(Category::Misread));
    assert_eq!(cat("that's not what i asked for", "You're right."), Some(Category::IgnoredInstructions));
}

#[test]
fn approvals_are_not_claims() {
    for h in [
        "No, that's fine, ship it",
        "No, that works",
        "no, that's perfect",
        "no that's perfect",
        "Nope, it works now",
        "No - you're right",
        "no, this is totally fine",
        "Actually, that works",
        "actually it's fine",
        "actually, no worries",
        "Actually no, that's okay",
        "that's not a problem",
        "this isn't bad at all.",
    ] {
        assert_eq!(label("Should be done.", h, "Great, moving on."), None, "{h}");
    }
    // Negations and complaints after the opener still count.
    for h in [
        "No, that's wrong",
        "no, that's not fine",
        "no, it's not working",
        "no, not good",
        "Actually, it still fails",
        "that's not the problem",
        "that's not a problem with the code, it's the config",
        // Past tense: it used to be fine, and the AI broke it.
        "No, it worked before you changed it",
        "No, it was working before your change",
        "No, that was correct before, you broke it",
        "No, it was fine until you touched it",
        "Actually, it worked before",
    ] {
        assert_eq!(label("Should be done.", h, "Checking."), Some(Label::Claimed), "{h}");
    }
}

type Table = (&'static str, &'static [rules::Rule]);

fn tables() -> [Table; 5] {
    [
        ("AI_ADMIT", rules::AI_ADMIT.as_slice()),
        ("USER_OPEN", rules::USER_OPEN.as_slice()),
        ("USER_ANY", rules::USER_ANY.as_slice()),
        ("CONTEXT", rules::CONTEXT.as_slice()),
        ("AI_PUSHBACK", rules::AI_PUSHBACK.as_slice()),
    ]
}

/// (rule id, text where that exact rule fires first in its table, near miss it must not match).
const EXAMPLES: &[(&str, &str, &str)] = &[
    ("ai.youre_right", "You're absolutely right, fixing it.", "You're almost right, but it starts at one."),
    ("ai.you_are_right", "You are right, the path was off.", "You are partly right about the path."),
    ("ai.good_catch", "Good catch, the loop was off by one.", "It is good to catch errors early."),
    ("ai.my_mistake", "My mistake, updating now.", "I keep a list of my mistakes."),
    ("ai.i_was_wrong", "I was wrong about the default.", "I was told the default was wrong."),
    ("ai.apologize", "I apologize for the confusion.", "No need to apologize."),
    ("ai.sorry", "Sorry about that, fixed.", "Sorry to hear the build is slow."),
    ("ai.on_me", "That's on me.", "That's on the menu."),
    ("ai.should_have", "I should have checked the config.", "You should have access now."),
    ("ai.incorrectly", "I incorrectly assumed v2.", "It was incorrectly set upstream."),
    ("ai.that_was_wrong", "That was wrong of me.", "That was the wrong branch?"),
    ("ai.didnt_check", "I didn't check the logs.", "I didn't change the logs."),
    ("you.no_that", "no, that's the wrong file", "no, that's fine, ship it"),
    ("you.no", "nope.", "no problem"),
    ("you.wrong", "not quite, the year is off", "that's not a problem"),
    ("you.its_wrong", "it's still broken on mobile", "it's still loading"),
    ("you.still", "still seeing the old header", "still, nice try"),
    ("you.you_missed", "you forgot the footer", "you found it, thanks"),
    ("you.i_said", "I told you to keep the old name", "I saved the file"),
    ("you.i_asked", "I asked for a list", "I asked around and nobody knows"),
    ("you.why_did_you", "why did you delete that?", "why does this need a key?"),
    ("you.doesnt_work", "doesn't work on firefox", "doesn't matter, keep going"),
    ("you.same_error", "same error as before", "same here"),
    ("you.actually", "actually, that's the old API", "actually, that works now"),
    ("you.any.doesnt_exist", "the flag doesn't exist in v2", "the flag didn't exit cleanly"),
    ("you.any.not_working", "the login is not working", "I'm not worried about it"),
    ("you.any.still", "the page is still broken", "I still like it"),
    ("you.any.you_missed", "looks like you skipped step two", "you didn't need to"),
    ("you.any.thats_wrong", "hmm, that's wrong", "that's not wrong"),
    ("you.any.made_up", "I think you made that up", "you made that work"),
    ("you.any.i_said", "but I told you the port", "I sold it"),
    ("you.any.not_what_i", "this is not what I wanted", "not what I expected, but nice"),
    ("ctx.also", "also, use tabs", "almost there"),
    ("ctx.forgot_to_mention", "oh, I forgot to mention we use yarn", "I forgot to save"),
    ("ctx.ack", "thanks, that works", "nicer colors please"),
    ("ai.push.actually", "Actually, it already exists.", "It actually works."),
    ("ai.push.not_right", "Hmm, that's not quite right.", "That's not quite done yet."),
    ("ai.push.dont_think", "I don't think that's correct.", "I don't think that's needed."),
    ("ai.push.not_the_case", "That isn't actually the case.", "This is not the end."),
    ("ai.push.push_back", "I'd push back on that.", "I'll push the branch."),
    ("ai.push.already_exists", "It already exists in utils.", "It already failed twice."),
    ("ai.push.not_a_bug", "Hmm, that's not a bug, it's intended.", "That's a bug."),
];

#[test]
fn every_rule_has_a_positive_and_a_near_miss() {
    let all: Vec<(&str, &rules::Rule, &[rules::Rule])> =
        tables().iter().flat_map(|(_, t)| t.iter().map(move |r| (r.id, r, *t))).collect();

    let mut ids: Vec<&str> = all.iter().map(|(id, ..)| *id).collect();
    ids.sort_unstable();
    let n = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), n, "rule ids must be unique");

    for (id, ..) in &all {
        assert!(EXAMPLES.iter().any(|(e, ..)| e == id), "{id} has no example in EXAMPLES");
    }
    let mut seen = Vec::new();
    for &(id, pos, neg) in EXAMPLES {
        assert!(!seen.contains(&id), "{id} is listed twice in EXAMPLES");
        seen.push(id);
        let (_, r, table) = all.iter().find(|(i, ..)| *i == id).unwrap_or_else(|| panic!("{id} is not a rule"));
        assert_eq!(rules::first(table, pos).map(|m| m.rule), Some(id), "positive for {id}: {pos:?}");
        assert!(r.find(neg).is_none(), "near miss for {id} matched: {neg:?}");
    }
}

#[test]
fn blame_goes_to_the_reply_you_corrected() {
    let t = Turn {
        human: "that's not a real page".into(),
        prev_ai: Some(AiMsg { text: "Go to /admin".into(), model: Some("gpt-5.1".into()) }),
        next_ai: Some(AiMsg { text: "You're right.".into(), model: Some("gpt-5.1-mini".into()) }),
        chat_model: None,
    };
    let v = classify(&t);
    assert_eq!(v.blamed_model(&t), Some("gpt-5.1"));
}

#[test]
fn spans_point_at_the_match() {
    let t = turn("x", "no, that's wrong", "Hmm. You're right about that.");
    let v = classify(&t);
    let (s, e) = v.ai_span.unwrap();
    assert_eq!(&t.next_ai.as_ref().unwrap().text[s..e], "You're right");
}

#[test]
fn multibyte_text_never_panics() {
    let h = "😀".repeat(3000);
    let n = "é".repeat(400);
    let _ = classify(&turn(&"ü".repeat(500), &h, &n));
    let _ = classify(&turn("", "nope — that's wrong 😀", "You’re right 😀"));
}

#[test]
fn placeholder_models_are_never_blamed() {
    let t = Turn {
        human: "i don't see an import button".into(),
        prev_ai: Some(AiMsg { text: "API Error: overloaded".into(), model: Some("<synthetic>".into()) }),
        next_ai: Some(AiMsg {
            text: "You're right, there's no import button.".into(),
            model: Some("claude-opus-5-5".into()),
        }),
        chat_model: None,
    };
    let v = classify(&t);
    assert_eq!(v.blamed_model(&t), Some("claude-opus-5-5"));
}

#[test]
fn falls_back_to_the_chats_model() {
    let t = Turn {
        human: "first message, never answered".into(),
        chat_model: Some("claude-opus-5".into()),
        ..Default::default()
    };
    assert_eq!(classify(&t).blamed_model(&t), Some("claude-opus-5"));
    let none = Turn { human: "x".into(), chat_model: Some("<synthetic>".into()), ..Default::default() };
    assert_eq!(classify(&none).blamed_model(&none), None);
}

#[test]
fn head_and_tail_count_chars() {
    assert_eq!(head_chars("héllo", 2), "hé");
    assert_eq!(head_chars("hi", 5), "hi");
    assert_eq!(tail_chars("héllo", 4), "éllo");
    assert_eq!(tail_chars("hi", 5), "hi");
    assert_eq!(tail_chars("", 0), "");
}

#[test]
fn ids_round_trip() {
    for c in Category::ALL {
        assert_eq!(Category::from_id(c.as_str()), Some(c));
    }
    assert_eq!(Category::from_id("nope"), None);
    assert_eq!(Label::ALL.iter().filter(|l| l.is_catch()).count(), 3);
}
