//! Redaction and snippets: the only text kept at the "snippet" retention level.

use crate::head_chars;
use regex::Regex;
use std::sync::LazyLock;

/// The longest snippet kept, in chars.
pub const SNIPPET_CHARS: usize = 140;

static REDACTIONS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    let r = |p: &str| Regex::new(p).unwrap();
    vec![
        // A private key block, to its END line or, if cut short, to the end of the text.
        (r(r"(?s)-----BEGIN [A-Z0-9 ]+-----.*?(?:-----END [A-Z0-9 ]+-----|\z)"), "[secret]"),
        // Links go before the secret rules, so no rule can insert a `]` that ends the link early and
        // leaves its tail (a key, an email, a private path) behind. A link with a login in it
        // (postgres://admin:pw@localhost/app) goes whole, whatever its scheme.
        (r(r"\b[A-Za-z][A-Za-z0-9+.-]*://[^\s/@]*@\S+"), "[link]"),
        (r(r"\bhttps?://[^\s)>\]]+"), "[link]"),
        // Windows and WSL paths, before the generic home rule below can cut them in half.
        (r(r"(?i)\b[A-Z]:[\\/]Users[\\/][^\\/\s]+"), "~"),
        (r(r"/mnt/[a-z]/Users/[^/\s]+"), "~"),
        (r(r#"(?i)\b[A-Z]:[\\/][^\s"'<>|]+"#), "[path]"),
        (r(r"\\\\[A-Za-z0-9._$-]+\\\S+"), "[path]"),
        // Links typed without a scheme: a dotted host followed by a path ("example.com/page").
        (r(r"\b[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}/[^\s)>\]]*"), "[link]"),
        (r(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"), "[email]"),
        (
            r(
                r"\b(sk-[A-Za-z0-9_-]{16,}|(?:sk|rk|pk)_(?:live|test)_[A-Za-z0-9]{8,}|github_pat_[A-Za-z0-9_]{20,}|gh[pousr]_[A-Za-z0-9]{20,}|glpat-[A-Za-z0-9_-]{10,}|npm_[A-Za-z0-9]{20,}|xox[abpr]-[A-Za-z0-9-]{10,}|AKIA[0-9A-Z]{16}|AIza[0-9A-Za-z_-]{30,}|eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,})",
            ),
            "[secret]",
        ),
        (r(r"(?i)\bauthorization\s*[:=]\s*(?:(?:bearer|basic|token)\s+)?\S+"), "Authorization: [secret]"),
        (r(r"(?i)\bbearer\s+[A-Za-z0-9._~+/=-]{20,}"), "Bearer [secret]"),
        // A name that ends in a secret word (DB_PASSWORD, client_secret, "api_key"), an optional closing
        // quote, then the value: quoted (it may hold spaces) or bare.
        (
            r(
                r#"(?i)\b([A-Za-z0-9_.-]*(?:password|passwd|secret|token|api[_-]?key|access[_-]?key|private[_-]?key))["']?\s*[:=]\s*(?:"[^"]*"|'[^']*'|\S+)"#,
            ),
            "$1=[secret]",
        ),
        // "my password is hunter2" takes the next word, even when it is only "too" (better lost than leaked).
        (r(r"(?i)\b(passwd|pass(?:word|code|phrase))\s+(?:is|was)\s+\S+"), "$1 [secret]"),
        (r(r"(/Users|/home)/[^/\s]+"), "~"),
        (r(r"(^|[^\w/.-])/root(?:/\S*|\b)"), "${1}~"),
        (r(r"/(private|var|tmp|Volumes|opt|mnt)/\S+"), "[path]"),
        (r(r"\b\d{1,3}(\.\d{1,3}){3}\b"), "[ip]"),
        (r(r"\+?\d[\d\s().-]{8,}\d"), "[number]"),
    ]
});

/// Removes secrets, emails, links, home and drive paths, IPs and long numbers. Applied to everything stored.
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

    /// Redacts and checks that none of `leaks` survives.
    fn clean(input: &str, leaks: &[&str]) -> String {
        let s = redact(input);
        for leak in leaks {
            assert!(!s.contains(leak), "{leak:?} survived in {s:?} (from {input:?})");
        }
        s
    }

    #[test]
    fn the_audits_real_samples_are_all_clean() {
        let samples: [(&str, &[&str]); 15] = [
            ("DB_PASSWORD=hunter2xyz", &["hunter2xyz"]),
            ("AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI/K7MDENG", &["wJalr"]),
            ("client_secret=abc123def456", &["abc123def456"]),
            (r#"{"password": "s3cr3tpw"}"#, &["s3cr3tpw"]),
            (r#"{"api_key": "k3y-value-here"}"#, &["k3y-value"]),
            ("Authorization: Bearer q9Zr7opaqueToken", &["q9Zr7"]),
            ("sk_live_abcdefghijklmnopqrst", &["abcdefghij"]),
            ("postgres://admin:pw1234@localhost/app", &["pw1234", "admin"]),
            (
                "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkq\nhkiG9w0BAQEF\n-----END PRIVATE KEY-----",
                &["MIIEv", "hkiG9"],
            ),
            (r"D:\Users\pat\code", &["pat"]),
            ("/root/.ssh/id_rsa", &["id_rsa", ".ssh"]),
            ("export OPENAI_API_KEY=sk-proj-abcdef0123456789", &["abcdef0123"]),
            ("my password is correcthorse", &["correcthorse"]),
            ("glpat-abcdefghij0123456789", &["abcdefghij"]),
            (r"\\server\share\pat", &["server", "share", r"\pat"]),
        ];
        for (input, leaks) in samples {
            clean(input, leaks);
        }
        clean("github_pat_11ABCDEFG0abcdefghij_klmnopqrstuvwxyz0123", &["11ABC", "klmnop"]);
        clean("npm_abcdefghijklmnopqrstuvwxyz0123456789", &["abcdefgh", "0123456789"]);
        assert_eq!(redact("npm_abcdefghijklmnopqrstuvwxyz0123456789"), "[secret]");
    }

    #[test]
    fn names_with_a_prefix_and_quoted_json_are_caught() {
        assert_eq!(redact("export OPENAI_API_KEY=abc123"), "export OPENAI_API_KEY=[secret]");
        assert_eq!(redact("DB_PASSWORD: 'two words'"), "DB_PASSWORD=[secret]");
        assert_eq!(redact(r#"{"password": "two words", "user": "pat"}"#), r#"{"password=[secret], "user": "pat"}"#);
        // Near misses: the word without a value, or a different word.
        let prose = "the password field, token count: 5, max input_tokens: 9, a secret garden";
        assert_eq!(redact(prose), prose);
    }

    #[test]
    fn bearer_tokens_are_caught_but_not_the_bearer_of_bad_news() {
        assert_eq!(redact("Authorization: Bearer abc"), "Authorization: [secret]");
        assert_eq!(redact("header Bearer abcdefghijklmnopqrstu1234 sent"), "header Bearer [secret] sent");
        assert_eq!(redact("the bearer of bad news"), "the bearer of bad news");
    }

    #[test]
    fn vendor_keys_are_caught_and_lookalikes_are_not() {
        for key in [
            "sk_live_abcdefgh12345678",
            "sk_test_abcdefgh12345678",
            "rk_live_abcdefgh12345678",
            "ghp_abcdefghijklmnopqrst1234",
            "glpat-abcdefghij0123456789",
        ] {
            assert_eq!(redact(&format!("key {key} end")), "key [secret] end", "{key}");
        }
        let prose = "the task_live_ view, a desk_test_case, sk_ prefix, npm_ scripts, github_pat_ is a prefix";
        assert_eq!(redact(prose), prose);
    }

    #[test]
    fn private_key_blocks_go_across_lines_or_to_the_end() {
        assert_eq!(
            redact("a -----BEGIN RSA PRIVATE KEY-----\nAAAA\nBBBB\n-----END RSA PRIVATE KEY----- b"),
            "a [secret] b"
        );
        assert_eq!(redact("cut off -----BEGIN PRIVATE KEY-----\nAAAA\nBBBB"), "cut off [secret]");
        assert_eq!(redact("-----BEGIN is just dashes"), "-----BEGIN is just dashes");
    }

    #[test]
    fn links_with_a_login_or_a_secret_inside_go_whole() {
        for url in [
            "https://x.io/sk-abcdefghijklmnopqrstuv/private-path",
            "https://x.io/repo?user=pat@corp.com&project=secret",
            "https://admin:pw@host.example/x",
            "postgres://admin:pw@localhost/app",
            "redis://:pw@localhost:6379",
        ] {
            assert_eq!(redact(&format!("see {url} now")), "see [link] now", "{url}");
        }
        // No login: a plain local address is not a credentialed link.
        assert_eq!(
            redact("run on localhost:3000 or postgres://localhost/app"),
            "run on localhost:3000 or postgres://localhost/app"
        );
    }

    #[test]
    fn passwords_said_in_words_are_caught() {
        assert_eq!(redact("my password is hunter2, ok"), "my password [secret] ok");
        // Decision: the next word goes even when it is only prose. Better lost than leaked.
        assert_eq!(redact("my password is too short"), "my password [secret] short");
        assert_eq!(redact("the password field is empty"), "the password field is empty");
    }

    #[test]
    fn drive_unc_wsl_and_root_paths_are_caught() {
        assert_eq!(redact(r"D:\Users\pat\code"), r"~\code");
        assert_eq!(redact("c:/Users/pat/code"), "~/code");
        assert_eq!(redact("/mnt/c/Users/pat/code"), "~/code");
        assert_eq!(redact(r"open E:\work\acme\notes.txt now"), "open [path] now");
        assert_eq!(redact(r"\\server\share\pat\file"), "[path]");
        assert_eq!(redact("cat /root/.ssh/id_rsa"), "cat ~");
        assert_eq!(redact("in /root, then"), "in ~, then");
        assert_eq!(redact("mount /mnt/data/backup"), "mount [path]");
        // Near misses.
        let prose = "option C: the drive, a/b, /rootfs and /usr/root, the C:";
        assert_eq!(redact(prose), prose);
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
