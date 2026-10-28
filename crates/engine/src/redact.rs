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

/// The sentence containing `span`, redacted and capped at 140 chars. Falls back to the opening.
pub fn snippet(text: &str, span: Option<(usize, usize)>) -> String {
    let (start, end) = match span {
        Some((s, e)) if e <= text.len() && text.is_char_boundary(s) && text.is_char_boundary(e) => {
            let start = text[..s].rfind(['.', '!', '?', '\n']).map_or(0, |i| i + 1);
            let end = text[e..].find(['.', '!', '?', '\n']).map_or(text.len(), |i| e + i + 1);
            (start, end)
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
    fn snippet_is_capped() {
        let long = "word ".repeat(100);
        let s = snippet(&long, None);
        assert!(s.chars().count() <= SNIPPET_CHARS);
        assert!(s.ends_with('…'));
    }
}
