//! Tips: one line an AI ends a reply with when a follow-up could have been in the first message.
//! The person reads it there, and the app counts it from the transcript. The app keeps only what
//! the bracket says, never the words.
//!
//! ```text
//! Tip from No, Actually...: next time, say it only breaks on Safari when you first ask. [evidence, redid 50%]
//! Tip from No, Actually...: next time, say which page you mean when you first ask. [target, had to ask]
//! ```

use crate::card::{habit_bit, SHARES};

/// What the AI is told, once per chat. `tests/tips.rs` holds it to the same rules as the report card skill.
pub const TIP_RULES: &str = include_str!("tip_rules.txt");
/// Said instead of the rules to a chat that may still have them after tips were turned off.
pub const TIP_STOP: &str =
    "If earlier instructions in this chat asked you to add 'Tip from No, Actually...' lines, stop adding them.";

/// How [`TIP_RULES`] starts, so a hook that prints a document quoting them doesn't read as giving them.
const RULES_START: &str =
    "No, Actually... is an app this person uses to learn to ask for things clearly the first time.";
/// Starts a tip line.
const PREFIX: &str = "Tip from No, Actually...:";
/// The same, with the dots typed as one character.
const PREFIX_ELLIPSIS: &str = "Tip from No, Actually…:";
/// Before the share in the bracket.
const REDID: &str = "redid ";
/// In the bracket when the AI only looked around and asked, so nothing was redone.
const ASKED: &str = "had to ask";
/// [`Tip::redid`] when the AI had to ask.
pub const HAD_TO_ASK: u8 = 0;

/// A tip, as the app keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tip {
    /// The habit that would have prevented it, as its [`habit_bit`].
    pub habit: u8,
    /// Percent of the work so far that had to be redone: 25, 50, 75 or 100, or [`HAD_TO_ASK`].
    pub redid: u8,
}

/// The tip that ends `text`, if there is one. Only the last line counts, outside a code block and
/// in the exact form, so a reply that quotes or explains a tip isn't read as one.
pub fn parse_tip(text: &str) -> Option<Tip> {
    find_tip(text).map(|(_, tip)| tip)
}

/// `text` without the tip that ends it, and without the space before it. A last line that starts
/// like a tip goes even if the app can't read its bracket, so a tip's words are never stored. Text
/// with no such line comes back as it was, so a chat without tips scores as it did before they existed.
pub fn strip_tip(text: &str) -> &str {
    find_line(text).map_or(text, |(start, _)| text[..start].trim_end())
}

/// True when `text`, the context a hook gave a chat, is the tip rules.
pub fn is_tip_rules(text: &str) -> bool {
    text.trim_start().starts_with(RULES_START)
}

/// True when `text`, the context a hook gave a chat, told it to stop giving tips.
pub fn is_tip_stop(text: &str) -> bool {
    text.trim() == TIP_STOP
}

/// Where the tip that ends `text` starts, and what it says.
fn find_tip(text: &str) -> Option<(usize, Tip)> {
    let (start, rest) = find_line(text)?;
    read_rest(rest).map(|tip| (start, tip))
}

/// Where the last line of `text` starts, and what follows the prefix, if it starts like a tip and
/// isn't in a code block. Bold or italics around it are fine: models add them.
fn find_line(text: &str) -> Option<(usize, &str)> {
    let end = text.trim_end().len();
    let start = text[..end].rfind('\n').map_or(0, |n| n + 1);
    let line = text[start..end].trim().trim_matches(is_emphasis);
    let rest = line.strip_prefix(PREFIX).or_else(|| line.strip_prefix(PREFIX_ELLIPSIS))?;
    (!leaves_fence_open(&text[..start])).then_some((start, rest))
}

/// Reads what follows the prefix.
fn read_rest(rest: &str) -> Option<Tip> {
    let (words, bracket) = rest.rsplit_once('[')?;
    let (habit, cost) = bracket.strip_suffix(']')?.split_once(", ")?;
    let says_something = !words.trim_matches(|c: char| is_emphasis(c) || c.is_whitespace()).is_empty();
    says_something.then_some(Tip { habit: habit_bit(habit)?, redid: read_cost(cost)? })
}

/// What the bracket says the follow-up cost.
fn read_cost(cost: &str) -> Option<u8> {
    if cost == ASKED {
        return Some(HAD_TO_ASK);
    }
    let redid = cost.strip_prefix(REDID)?.strip_suffix('%')?.parse().ok()?;
    // 0 is a real share on a report card, but work that was only added gets no tip.
    (redid > 0 && SHARES.contains(&redid)).then_some(redid)
}

fn is_emphasis(c: char) -> bool {
    c == '*' || c == '_'
}

/// True when `text` ends inside a code block.
fn leaves_fence_open(text: &str) -> bool {
    let fences = text.lines().map(str::trim_start).filter(|l| l.starts_with("```") || l.starts_with("~~~")).count();
    fences % 2 == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str =
        "Tip from No, Actually...: next time, say it only breaks on Safari when you first ask. [evidence, redid 50%]";

    fn tip(habit: &str, redid: u8) -> Tip {
        Tip { habit: habit_bit(habit).unwrap(), redid }
    }

    fn with(line: &str) -> String {
        format!("Fixed it, and the tests pass.\n\n{line}")
    }

    #[test]
    fn reads_the_line_that_ends_a_reply() {
        assert_eq!(parse_tip(&with(LINE)), Some(tip("evidence", 50)));
        assert_eq!(parse_tip(LINE), Some(tip("evidence", 50)), "a text record can be the line alone");
        assert_eq!(parse_tip(&format!("{}\n\n  ", with(LINE))), Some(tip("evidence", 50)));
        assert_eq!(parse_tip(&with(LINE).replace('\n', "\r\n")), Some(tip("evidence", 50)));
    }

    #[test]
    fn reads_every_habit_and_share() {
        for habit in ["target", "evidence", "finish", "guardrails", "purpose", "context"] {
            for redid in [25, 50, 75, 100] {
                let line = format!("Tip from No, Actually...: say more. [{habit}, redid {redid}%]");
                assert_eq!(parse_tip(&line), Some(tip(habit, redid)), "{line}");
            }
        }
    }

    #[test]
    fn reads_a_tip_where_the_ai_had_to_ask() {
        for habit in ["target", "evidence", "finish", "guardrails", "purpose", "context"] {
            let line = format!("Tip from No, Actually...: say more. [{habit}, had to ask]");
            assert_eq!(parse_tip(&with(&line)), Some(tip(habit, HAD_TO_ASK)), "{line}");
            assert_eq!(strip_tip(&with(&line)), "Fixed it, and the tests pass.");
        }
    }

    #[test]
    fn only_the_last_line_counts() {
        assert_eq!(parse_tip(&format!("{LINE}\n\nAnything else?")), None);
        assert_eq!(parse_tip(&format!("Here's a tip line: {LINE}")), None, "quoted mid-line");
        assert_eq!(parse_tip(&format!("> {LINE}")), None, "quoted");
    }

    #[test]
    fn a_code_block_is_an_example_not_a_tip() {
        assert_eq!(parse_tip(&format!("Looks like this:\n```\n{LINE}")), None);
        assert_eq!(parse_tip(&format!("~~~\n{LINE}")), None);
        assert_eq!(parse_tip(&format!("```\n{LINE}\n```")), None);
        assert_eq!(
            parse_tip(&format!("```js\nlet a = 1;\n```\n\n{LINE}")),
            Some(tip("evidence", 50)),
            "a closed block before it"
        );
    }

    #[test]
    fn the_form_must_be_exact() {
        for bad in [
            "Tip from No, Actually...: say more. [shoes, redid 50%]",
            "Tip from No, Actually...: say more. [evidence, redid 60%]",
            "Tip from No, Actually...: say more. [evidence, redid 0%]",
            "Tip from No, Actually...: say more. [evidence, Had to ask]",
            "Tip from No, Actually...: say more. [evidence, had to ask 50%]",
            "Tip from No, Actually...: say more. [evidence had to ask]",
            "Tip from No, Actually...: say more. [had to ask]",
            "Tip from No, Actually...: say more. [evidence, redid 50]",
            "Tip from No, Actually...: say more. [evidence, 50%]",
            "Tip from No, Actually...: say more. [Beleg, 50 % wiederholt]",
            "Tip from No, Actually...: say more. (evidence, redid 50%)",
            "Tip from No, Actually...: say more.",
            "Tip from No, Actually...: [evidence, redid 50%]",
            "Tip from No, Actually...: ** [evidence, redid 50%]",
            "Tip from No Actually: say more. [evidence, redid 50%]",
            "Tip: say more. [evidence, redid 50%]",
        ] {
            assert_eq!(parse_tip(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_tip_the_app_cant_read_still_loses_its_words() {
        let line = "Tip from No, Actually...: say more. [evidence, redid 60%]";
        assert_eq!(strip_tip(&with(line)), "Fixed it, and the tests pass.");
        assert_eq!(strip_tip(&with(line.trim_end_matches("[evidence, redid 60%]"))), "Fixed it, and the tests pass.");
        let other = "Tip: say more. [evidence, redid 50%]";
        assert_eq!(strip_tip(other), other, "not ours");
    }

    #[test]
    fn the_words_can_be_anything() {
        assert_eq!(
            parse_tip("Tip from No, Actually...: nächstes Mal sag es gleich. [context, redid 75%]"),
            Some(tip("context", 75))
        );
        assert_eq!(
            parse_tip("Tip from No, Actually...: 次回は最初に伝えてください。 [finish, redid 25%]"),
            Some(tip("finish", 25))
        );
        assert_eq!(
            parse_tip("Tip from No, Actually...: say `users[0]` is empty. [target, redid 100%]"),
            Some(tip("target", 100))
        );
    }

    #[test]
    fn bold_italics_and_a_typed_ellipsis_are_fine() {
        for line in [
            "*Tip from No, Actually...: say more. [finish, redid 25%]*",
            "_Tip from No, Actually...: say more. [finish, redid 25%]_",
            "**Tip from No, Actually...:** say more. [finish, redid 25%]",
            "**Tip from No, Actually...: say more. [finish, redid 25%]**",
            "Tip from No, Actually…: say more. [finish, redid 25%]",
        ] {
            assert_eq!(parse_tip(&with(line)), Some(tip("finish", 25)), "{line}");
            assert_eq!(strip_tip(&with(line)), "Fixed it, and the tests pass.", "{line}");
        }
    }

    #[test]
    fn stripping_leaves_the_rest_of_the_reply() {
        assert_eq!(strip_tip(&with(LINE)), "Fixed it, and the tests pass.");
        assert_eq!(strip_tip(LINE), "", "a record that was only the tip leaves nothing");
    }

    #[test]
    fn text_without_a_tip_comes_back_as_it_was() {
        for text in ["", "  ", "All done.\n\n", "Line one\r\nLine two  \n"] {
            assert_eq!(strip_tip(text), text);
        }
    }

    #[test]
    fn the_rules_read_as_the_rules_and_the_stop_sentence_does_not() {
        assert!(is_tip_rules(TIP_RULES));
        assert!(!is_tip_rules(TIP_STOP));
        assert!(!is_tip_rules(LINE));
        assert!(!is_tip_rules(&format!("Our docs say: {TIP_RULES}")), "a quote isn't the rules");
        assert!(is_tip_stop(TIP_STOP) && !is_tip_stop(TIP_RULES));
    }

    #[test]
    fn the_rules_show_the_form_the_parser_reads() {
        let form = TIP_RULES.lines().find(|l| l.starts_with(PREFIX)).expect("the rules show the line");
        assert!(form.ends_with("[<habit>, redid <N>%]"), "{form}");
        let filled =
            form.replace("<habit>", "target").replace("<N>", "50").replace("<the specific thing>", "which page");
        assert_eq!(parse_tip(&filled), Some(tip("target", 50)));
        let asked = filled.replace("redid 50%", ASKED);
        assert!(TIP_RULES.contains(&format!("\"{ASKED}\"")), "the rules give the other form");
        assert_eq!(parse_tip(&asked), Some(tip("target", HAD_TO_ASK)));
    }
}
