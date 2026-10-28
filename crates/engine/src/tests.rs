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
    assert_eq!(cat("we use epson", "You're right, let me confirm."), Some(Category::JustWrong));
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
