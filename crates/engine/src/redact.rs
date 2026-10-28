//! Redaction and snippets: the only text kept at the "snippet" retention level.

use crate::head_chars;
use regex::Regex;
use std::sync::LazyLock;

pub const SNIPPET_CHARS: usize = 140;

static REDACTIONS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    let r = |p: &str| Regex::new(p).unwrap();
    vec![
        // Secrets first, so a key inside a URL is still caught.
        (
            r(
                r"\b(sk-[A-Za-z0-9_-]{16,}|sk-ant-[A-Za-z0-9_-]{16,}|gh[pousr]_[A-Za-z0-9]{20,}|xox[abpr]-[A-Za-z0-9-]{10,}|AKIA[0-9A-Z]{16}|AIza[0-9A-Za-z_-]{30,}|eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,})",
            ),
            "[secret]",
        ),
        (r(r"(?i)\b(password|passwd|secret|token|api[_-]?key)\s*[:=]\s*\S+"), "$1=[secret]"),
        (r(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"), "[email]"),
        (r(r"\bhttps?://[^\s)>\]]+"), "[link]"),
        // Links typed without a scheme: a dotted host followed by a path ("example.com/page").
        (r(r"\b[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}/[^\s)>\]]*"), "[link]"),
        (r(r"(/Users|/home)/[^/\s]+"), "~"),
        (r(r"/(private|var|tmp|Volumes|opt)/\S+"), "[path]"),
        (r(r"(?i)\bC:\\Users\\[^\\\s]+"), "~"),
        (r(r"\b\d{1,3}(\.\d{1,3}){3}\b"), "[ip]"),
        (r(r"\+?\d[\d\s().-]{8,}\d"), "[number]"),
    ]
});

/// Removes secrets, emails, links, home paths, IPs and long numbers. Applied to everything stored.
pub fn redact(text: &str) -> String {
    REDACTIONS.iter().fold(text.to_string(), |acc, (re, with)| re.replace_all(&acc, *with).into_owned())
}

/// A sentence ends at a newline, or at . ! ? followed by whitespace or the end of the text, so
/// the dots inside "pat@corp.com" or "example.com/page" never cut an address in half (which
/// would hide it from redaction).
fn ends_sentence(text: &str, i: usize, c: char) -> bool {
    c == '\n'
        || (matches!(c, '.' | '!' | '?') && text[i + c.len_utf8()..].chars().next().is_none_or(char::is_whitespace))
}

fn sentence_start(text: &str, before: usize) -> usize {
    text[..before].char_indices().rev().find(|&(i, c)| ends_sentence(text, i, c)).map_or(0, |(i, c)| i + c.len_utf8())
}

fn sentence_end(text: &str, after: usize) -> usize {
    text[after..]
        .char_indices()
        .find(|&(i, c)| ends_sentence(text, after + i, c))
        .map_or(text.len(), |(i, c)| after + i + c.len_utf8())
}

/// The sentence containing `span`, redacted and capped at 140 chars. Falls back to the opening.
pub fn snippet(text: &str, span: Option<(usize, usize)>) -> String {
    let (start, end) = match span {
        Some((s, e)) if e <= text.len() && text.is_char_boundary(s) && text.is_char_boundary(e) => {
            (sentence_start(text, s), sentence_end(text, e))
        }
        _ => (0, text.len()),
    };
    let sentence = redact(text[start..end].trim());
    let sentence = sentence.split_whitespace().collect::<Vec<_>>().join(" ");
    if sentence.chars().count() > SNIPPET_CHARS {
        format!("{}…", head_chars(&sentence, SNIPPET_CHARS - 1).trim_end())
    } else {
        sentence
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_private_bits() {
        let tmp = redact("see /var/folders/n6/zj/T/shot.png now");
        assert_eq!(tmp, "see [path] now");
        let s = redact("mail me at pat@example.com, key sk-ant-abc123def456ghi789jkl, see https://x.io/a and /Users/pat/code, call 555-123-4567");
        assert!(!s.contains("pat@"), "{s}");
        assert!(!s.contains("sk-ant"), "{s}");
        assert!(!s.contains("x.io"), "{s}");
        assert!(!s.contains("/Users/pat"), "{s}");
        assert!(!s.contains("4567"), "{s}");
        assert!(s.contains("[email]") && s.contains("[secret]") && s.contains("[link]") && s.contains("~/code"), "{s}");
    }

    #[test]
    fn snippet_is_the_matched_sentence() {
        let text = "Let me look. You're right, that page doesn't exist. I'll fix the link now.";
        let i = text.find("You're").unwrap();
        assert_eq!(snippet(text, Some((i, i + 12))), "You're right, that page doesn't exist.");
    }

    #[test]
    fn dots_inside_addresses_never_cut_the_snippet() {
        let s = snippet("No, my email is pat@corp-internal.com and not that one. Next sentence.", Some((0, 2)));
        assert_eq!(s, "No, my email is [email] and not that one.");
        let text = "See example.com/secret-project now. You're right about it.";
        let i = text.find("You're").unwrap();
        assert_eq!(snippet(text, Some((i, i + 6))), "You're right about it.");
        assert_eq!(snippet(text, Some((0, 3))), "See [link] now.");
        assert_eq!(redact("open github.com/acme/secret-repo please"), "open [link] please");
    }

    #[test]
    fn snippet_is_capped() {
        let long = "word ".repeat(100);
        let s = snippet(&long, None);
        assert!(s.chars().count() <= SNIPPET_CHARS);
        assert!(s.ends_with('…'));
    }
}
