//! Redaction and snippets: the only text kept at the "snippet" retention level.

use crate::head_chars;
use regex::{Captures, Regex};
use std::sync::LazyLock;

/// Bump when [`redact`] catches more, so text stored under older rules is cleaned once.
pub const REDACTION_VERSION: u32 = 2;

/// The longest snippet kept, in chars.
pub const SNIPPET_CHARS: usize = 140;

/// A value after a secret's name: escaped-quoted (in JSON inside a string), quoted (it may hold
/// spaces), already redacted (so a second pass changes nothing), or bare.
const VALUE: &str = r#"(?P<v>\\"[^"]*?\\"|"[^"]*"|'[^']*'|\[secret\]|\S+)"#;

/// What follows a home folder: more path, up to where prose or a list takes over again.
/// A folder's name can hold a few spaces or commas when another folder follows it.
const UNDER: &str = r#"(?:[\\/][^\s\\/,;)>"'\]]+(?:[ ,]+[^\s\\/,;)>"'\]]+){0,3})*[\\/][^\s,;)>"'\]]+"#;

/// Words that start ordinary prose or code, not a secret: "Forgot password: the reset flow is broken",
/// "password: string;", "the --token is required".
const NOT_A_VALUE: &[&str] = &[
    "a",
    "after",
    "all",
    "always",
    "an",
    "and",
    "any",
    "are",
    "argument",
    "as",
    "at",
    "be",
    "before",
    "boolean",
    "bool",
    "but",
    "by",
    "can",
    "cannot",
    "do",
    "does",
    "doesn't",
    "don't",
    "each",
    "empty",
    "every",
    "expired",
    "false",
    "field",
    "flag",
    "for",
    "from",
    "has",
    "have",
    "how",
    "i",
    "if",
    "in",
    "int",
    "invalid",
    "is",
    "isn't",
    "it",
    "it's",
    "just",
    "may",
    "missing",
    "must",
    "my",
    "needs",
    "never",
    "new",
    "nil",
    "no",
    "none",
    "not",
    "null",
    "number",
    "of",
    "old",
    "on",
    "option",
    "optional",
    "or",
    "our",
    "param",
    "parameter",
    "please",
    "required",
    "reset",
    "should",
    "so",
    "some",
    "str",
    "string",
    "that",
    "the",
    "their",
    "then",
    "there",
    "this",
    "to",
    "too",
    "true",
    "type",
    "undefined",
    "use",
    "used",
    "value",
    "variable",
    "was",
    "we",
    "were",
    "what",
    "when",
    "where",
    "which",
    "why",
    "will",
    "with",
    "works",
    "you",
    "your",
];

/// True when `value`, the words after a secret's name, reads as prose or code rather than a secret.
fn not_a_value(value: &str) -> bool {
    if value.starts_with('=') {
        return true; // `token == null`
    }
    let first = value.split_whitespace().next().unwrap_or("");
    let word = first.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_lowercase();
    NOT_A_VALUE.contains(&word.as_str())
}

struct Rule {
    re: Regex,
    with: &'static str,
}

static REDACTIONS: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    let r = |p: &str| Regex::new(p).unwrap();
    let rules: Vec<(Regex, &'static str)> = vec![
        // A private key block, to its END line or, if cut short, to the end of the text.
        (r(r"(?s)-----BEGIN [A-Z0-9 ]+-----.*?(?:-----END [A-Z0-9 ]+-----|\z)"), "[secret]"),
        // Links go before the secret rules, so no rule can insert a `]` that ends the link early and
        // leaves its tail (a key, an email, a private path) behind. A link with a login in it
        // (postgres://admin:pw@localhost/app) goes whole, whatever its scheme.
        (r(r"\b[A-Za-z][A-Za-z0-9+.-]*://[^\s@]*@\S+"), "[link]"),
        (r(r"\bhttps?://[^\s)>\]]+"), "[link]"),
        // Windows and WSL paths, before the generic home rule below can cut them in half. Folders under
        // home can name a client or a project, so they go too; only the home folder itself is "~".
        (r(&format!(r"(?i)\b[A-Z]:[\\/]Users[\\/][^\\/\s]+{UNDER}")), "~/[path]"),
        (r(r"(?i)\b[A-Z]:[\\/]Users[\\/][^\\/\s]+"), "~"),
        (r(&format!(r"/mnt/[a-z]/Users/[^/\s]+{UNDER}")), "~/[path]"),
        (r(r"/mnt/[a-z]/Users/[^/\s]+"), "~"),
        (r(r#"(?i)\b[A-Z]:[\\/][^\s"'<>|]+"#), "[path]"),
        (r(r"\\\\[A-Za-z0-9._$-]+\\\S+"), "[path]"),
        // Links typed without a scheme: a dotted host followed by a path ("example.com/page").
        (r(r"\b[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}/[^\s)>\]]*"), "[link]"),
        (r(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"), "[email]"),
        (
            r(
                r"\b(sk-[A-Za-z0-9_-]{16,}|(?:sk|rk|pk)_(?:live|test)_[A-Za-z0-9]{8,}|github_pat_[A-Za-z0-9_]{20,}|gh[pousr]_[A-Za-z0-9]{20,}|glpat-[A-Za-z0-9_-]{10,}|npm_[A-Za-z0-9]{20,}|xox[abpr]-[A-Za-z0-9-]{10,}|(?:AKIA|ASIA)[0-9A-Z]{16}|hf_[A-Za-z0-9]{20,}|gsk_[A-Za-z0-9]{20,}|whsec_[A-Za-z0-9]{16,}|SG\.[A-Za-z0-9_-]{16,}\.[A-Za-z0-9_-]{16,}|AIza[0-9A-Za-z_-]{30,}|eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,})",
            ),
            "[secret]",
        ),
        (r(r"(?i)\bauthorization\s*[:=]\s*(?:(?:bearer|basic|token)\s+)?\S+"), "Authorization: [secret]"),
        (r(r"(?i)\bbearer\s+[A-Za-z0-9._~+/=-]{20,}"), "Bearer [secret]"),
        (r(r"(?i)\b(set-cookie|cookie)\s*:[^\n]*=[^\n]*"), "$1: [secret]"),
        // A flag and its value, split by a space (`--password x`, `--api-key x`; not another flag).
        (
            r(
                r"(?i)(--[a-z0-9-]*(?:password|passwd|pwd|token|secret|api-?key|access-?key|private-?key))\s+(?P<v>[^\s-]\S*)",
            ),
            "$1 [secret]",
        ),
        // A password typed after a flag with no space or name: `mysql -u root -phunter2`, `sshpass -p x`,
        // `curl -u admin:pw`. Only after the command that reads it that way: elsewhere -p is a port or a path.
        (r(r"(?i)\b(mysql\w*\b[^\n|;&]*?\s)(?-i:-p)\S+"), "${1}-p[secret]"),
        (r(r#"(?i)\b(sshpass\b[^\n|;&]*?\s)(?-i:-p)\s*(?:'[^']*'|"[^"]*"|\S+)"#), "${1}-p [secret]"),
        (r(r#"(?i)\b(curl\b[^\n|;&]*?\s)(?-i:-u\s*|--user(?:\s+|=))(?:'[^']*'|"[^"]*"|\S+)"#), "${1}-u [secret]"),
        // "password: correct horse battery" is all one secret: the rest of the line goes.
        (
            r(r#"(?i)\b([A-Za-z0-9_.-]*(?:password|passwd|passphrase|passcode))["']?\s*:\s*(?P<v>[^\s"'\\\[][^\n]*)"#),
            "$1=[secret]",
        ),
        // A name that ends in a secret word (DB_PASSWORD, client_secret, "api_key", SECRET_KEY), an optional
        // closing quote, then the value.
        (
            r(&format!(
                r#"(?i)\b([A-Za-z0-9_.-]*(?:password|passwd|passphrase|passcode|pwd|secret|token|(?:secret|signing|encryption|private|api|auth|access)[_-]?key))(?:\\?["'])?\s*[:=]\s*{VALUE}"#
            )),
            "$1=[secret]",
        ),
        // Short names only count whole (`pw:`, `DB_PASS=`), so "bypass" and "compass" are left alone.
        (r(&format!(r#"(?i)\b((?:[A-Za-z0-9]+[_.-])*pw)(?:\\?["'])?\s*[:=]\s*{VALUE}"#)), "$1=[secret]"),
        (r(&format!(r#"(?i)\b((?:[A-Za-z0-9]+[_.-])*pass)(?:\\?["'])?\s*=\s*{VALUE}"#)), "$1=[secret]"),
        // "my password is hunter2" takes the next word, even when it is only "too" (better lost than leaked).
        (r(r"(?i)\b(passwd|pass(?:word|code|phrase))\s+(?:is|was)\s+\S+"), "$1 [secret]"),
        (r(&format!(r"(/Users|/home)/[^/\s]+{UNDER}")), "~/[path]"),
        (r(r"(/Users|/home)/[^/\s]+"), "~"),
        // Folders under a "~" typed by hand are just as private.
        (r(r#"(^|[^\w/.~-])~[\\/](?:\[path\]|[^\s,;)>"'\]]+)"#), "${1}~/[path]"),
        (r(r"(^|[^\w/.-])/root(?:/\S*|\b)"), "${1}~"),
        (r(r"/(private|var|tmp|Volumes|opt|mnt)/\S+"), "[path]"),
        (r(r"\b\d{1,3}(\.\d{1,3}){3}\b"), "[ip]"),
        (r(r"\+?\d[\d\s().-]{8,}\d"), "[number]"),
    ];
    rules.into_iter().map(|(re, with)| Rule { re, with }).collect()
});

/// Removes secrets, emails, links, home and drive paths, IPs and long numbers. Applied to everything stored.
pub fn redact(text: &str) -> String {
    REDACTIONS.iter().fold(text.to_string(), |acc, rule| {
        rule.re
            .replace_all(&acc, |c: &Captures<'_>| {
                if c.name("v").is_some_and(|v| not_a_value(v.as_str())) {
                    return c[0].to_string();
                }
                let mut out = String::new();
                c.expand(rule.with, &mut out);
                out
            })
            .into_owned()
    })
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
        assert!(
            s.contains("[email]") && s.contains("[secret]") && s.contains("[link]") && s.contains("~/[path]"),
            "{s}"
        );
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
    fn ordinary_sentences_and_code_are_left_alone() {
        for text in [
            "Forgot password: the reset flow is broken and the email never arrives",
            "Here's the secret: always test first",
            "Use the --api-key option to set it, and the --password flag too",
            "the --token is required",
            "if (token == null) return",
            "password: string;",
        ] {
            assert_eq!(redact(text), text);
        }
        assert_eq!(redact("password: correct horse battery"), "password=[secret]");
        assert_eq!(redact("--token abc123"), "--token [secret]");
        assert_eq!(redact("token=expired"), "token=expired", "a word that says what it is");
    }

    #[test]
    fn folders_with_spaces_and_old_windows_output_go_too() {
        assert_eq!(redact("/Users/pat/My Client/plan.md"), "~/[path]");
        assert_eq!(redact("/Users/pat/a,b/AcmeSecret"), "~/[path]");
        assert_eq!(redact(r"~\Documents\AcmeClient\plan.docx"), "~/[path]");
        assert_eq!(redact("/Users/pat/code, then stop"), "~/[path], then stop");
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
            (r"D:\Users\pat\code", &["Users", r"\pat"]),
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
        assert_eq!(redact(r"D:\Users\pat\code"), "~/[path]");
        assert_eq!(redact("c:/Users/pat/code"), "~/[path]");
        assert_eq!(redact("/mnt/c/Users/pat/code"), "~/[path]");
        assert_eq!(redact(r"D:\Users\pat"), "~");
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
    fn folders_under_home_go_with_the_home_folder() {
        assert_eq!(redact("open /Users/pat/Web/AcmeClient/plan.md now"), "open ~/[path] now");
        assert_eq!(redact("in /home/pat/clients/acme, then"), "in ~/[path], then");
        assert_eq!(redact(r"C:\Users\pat\Documents\AcmeClient\plan.docx"), "~/[path]");
        assert_eq!(redact("open ~/Web/AcmeClient/plan.md now"), "open ~/[path] now");
        assert_eq!(redact("/Users/pat"), "~");
        // Near misses: a bare tilde, and a path that isn't under home.
        let prose = "cd ~ then ~/ and a~/b, see /usr/local/bin";
        assert_eq!(redact(prose), prose);
    }

    #[test]
    fn secret_key_names_are_caught_and_plain_prose_about_keys_is_not() {
        for name in
            ["SECRET_KEY", "JWT_SECRET_KEY", "AWS_SECRET_KEY", "ENCRYPTION_KEY", "SIGNING_KEY", "api-key", "AUTH_KEY"]
        {
            assert_eq!(redact(&format!("{name}=abc123def")), format!("{name}=[secret]"), "{name}");
        }
        assert_eq!(redact("pwd=abc123"), "pwd=[secret]");
        assert_eq!(redact("DB_PASS=abc123"), "DB_PASS=[secret]");
        assert_eq!(redact("pw: abc123 ok"), "pw=[secret] ok");
        let prose = "the key to this is simple, pass the test, keyboard=us, bypass=true, compass: north, a turkey=1";
        assert_eq!(redact(prose), prose);
    }

    #[test]
    fn flags_with_a_space_before_the_value_are_caught() {
        assert_eq!(redact("run --password hunter2 now"), "run --password [secret] now");
        assert_eq!(redact("cli --token abc123 --verbose"), "cli --token [secret] --verbose");
        assert_eq!(redact("cli --api-key abc123"), "cli --api-key [secret]");
        assert_eq!(redact("mysql -u root -phunter2 db"), "mysql -u root -p[secret] db");
        assert_eq!(redact("sshpass -p hunter2 ssh host"), "sshpass -p [secret] ssh host");
        assert_eq!(redact("sshpass -p 'two words' ssh host"), "sshpass -p [secret] ssh host");
        assert_eq!(redact("curl -u admin:pw https://x.io/a"), "curl -u [secret] [link]");
        assert_eq!(redact("curl --user admin:pw -s"), "curl -u [secret] -s");
        // Near misses: -p is a port outside mysql and sshpass, and these flags aren't secrets.
        let prose = "psql -p 5432, mysql -P 3306 -u root, curl --user-agent foo, --token-count 5, ssh -p 22 host";
        assert_eq!(redact(prose), prose);
    }

    #[test]
    fn escaped_json_cookies_and_multi_word_passwords_are_caught() {
        assert_eq!(redact(r#"{\"password\": \"s3cr3t pw\"}"#), r#"{\"password=[secret]}"#);
        assert_eq!(redact("Cookie: sid=abc123; theme=dark"), "Cookie: [secret]");
        assert_eq!(redact("Set-Cookie: sid=abc123; Path=/; HttpOnly"), "Set-Cookie: [secret]");
        assert_eq!(redact("password: correct horse battery staple"), "password=[secret]");
        assert_eq!(redact("password: correct horse\nnext line"), "password=[secret]\nnext line");
        // Near misses: a cookie with no value, and talk about passwords.
        let prose = "cookie: the banner, the password field, a password manager";
        assert_eq!(redact(prose), prose);
    }

    #[test]
    fn more_vendor_keys_and_logins_with_a_slash_are_caught() {
        for key in [
            "hf_abcdefghijklmnopqrstuvwxyz",
            "gsk_abcdefghijklmnopqrstuvwxyz",
            "whsec_abcdefghijklmnop1234",
            // Split so secret scanners don't take this made-up sample for a real key.
            concat!("ASIA", "ABCDEFGHIJKLMNOP"),
            "SG.abcdefghijklmnopqrstuv.ABCDEFGHIJKLMNOPQRSTUV0123456789-_ab",
        ] {
            assert_eq!(redact(&format!("key {key} end")), "key [secret] end", "{key}");
        }
        for url in ["postgres://admin:pa/ss@localhost/app", "mysql://root:p/w/d@db:3306/x"] {
            assert_eq!(redact(&format!("see {url} now")), "see [link] now", "{url}");
        }
        let prose = "the hf_ prefix, hf_transformers_cache_dir, gsk_ keys, SG. numbers, ASIA is a region";
        assert_eq!(redact(prose), prose);
    }

    #[test]
    fn redacting_again_changes_nothing() {
        for input in [
            "SECRET_KEY=abc123 and --password hunter2 and pw: x",
            "mysql -u root -phunter2 db; sshpass -p 'a b' ssh h; curl -u a:b https://x.io",
            r#"{\"password\": \"s3cr3t\"} {"api_key": "two words", "user": "pat"}"#,
            "Cookie: a=b; Set-Cookie: c=d",
            "password: correct horse battery staple",
            "open /Users/pat/Web/AcmeClient/plan.md, ~/Web/Acme, C:\\Users\\pat\\x and /home/pat",
            "postgres://admin:pa/ss@localhost/app hf_abcdefghijklmnopqrstuvwxyz SG.abcdefghijklmnopqrstuv.ABCDEFGHIJKLMNOPQRSTUV0123456789",
            "my password is hunter2, Authorization: Bearer abc, cat /root/.ssh/id_rsa, call 555-123-4567, 10.0.0.1",
            "the key to this is simple, pass the test, keyboard=us",
        ] {
            let once = redact(input);
            assert_eq!(redact(&once), once, "{input:?} -> {once:?}");
        }
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
