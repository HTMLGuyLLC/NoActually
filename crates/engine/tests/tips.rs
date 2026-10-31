//! The tip rules and the report card skill judge the same thing, so they have to say the same
//! thing. The AI reads each on its own, and nothing else keeps them in step.

use engine::{habit_plain, HABITS, TIP_RULES};

const SKILL: &str = include_str!("../../../plugins/grade/skills/me/SKILL.md");

/// What counts against nobody. Worded differently in places, so these are the shared words.
const NEVER_THEIRS: [&str; 5] = [
    "learned from your work",
    "could have found by looking",
    "own mistakes when the request was clear",
    "only your work",
    "for a new reason",
];

#[test]
fn both_list_every_habit() {
    for (name, _) in HABITS {
        assert!(TIP_RULES.contains(name), "the tip rules leave out {name}");
        assert!(SKILL.contains(name), "the skill leaves out {name}");
    }
}

#[test]
fn both_use_the_same_shares() {
    assert!(TIP_RULES.contains("25, 50, 75 or 100"), "the tip rules");
    assert!(SKILL.contains("0, 25, 50, 75 or 100"), "the skill");
}

#[test]
fn both_agree_on_what_never_counts_against_the_person() {
    let (rules, skill) = (TIP_RULES.to_lowercase(), SKILL.to_lowercase());
    for words in NEVER_THEIRS {
        assert!(rules.contains(words), "the tip rules leave out: {words}");
        assert!(skill.contains(words), "the skill leaves out: {words}");
    }
}

#[test]
fn the_app_says_each_habit_in_the_skills_plain_words() {
    for (name, _) in HABITS {
        assert!(SKILL.contains(habit_plain(name)), "the skill words {name} differently");
    }
}
