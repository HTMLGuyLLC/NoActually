//! What makes the log safe to share: panics are written to it with quoted text cut out, and paths
//! have the home folder written as `~`.

use std::path::Path;

/// How much the log records.
pub(crate) const LOG_LEVEL: log::LevelFilter = log::LevelFilter::Info;

/// One line for the log: the panic's message and where it happened.
fn panic_line(info: &std::panic::PanicHookInfo<'_>) -> String {
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
        .unwrap_or("no message");
    let message = strip_quoted(message);
    match info.location() {
        Some(at) => format!("panic: {message} at {}:{}", at.file(), at.line()),
        None => format!("panic: {message}"),
    }
}

/// `message` with whatever sits inside double quotes or backticks replaced by `…`. A panic's text can
/// quote a path or a line of a chat (`unwrap()` on an error that holds one), and the log gets shared
/// in bug reports. An unclosed quote hides the rest.
fn strip_quoted(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut chars = message.chars();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '"' || c == '`' {
            let mut escaped = false;
            for d in chars.by_ref() {
                match d {
                    _ if escaped => escaped = false,
                    '\\' if c == '"' => escaped = true,
                    _ if d == c => break,
                    _ => {}
                }
            }
            out.push('…');
            out.push(c);
        }
    }
    out
}

/// `path` with the home folder written as `~`, for the log: it is shared in bug reports, and the
/// folder is the user's name.
fn shorten_home(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

/// `text` with the home folder written as `~` wherever it appears, for log lines that quote a path.
/// Whole folders only: `/Users/me` leaves `/Users/mellow` alone.
pub(crate) fn shorten_home_in(text: &str, home: Option<&Path>) -> String {
    let Some(home) = home.map(|h| h.display().to_string()).filter(|h| !h.is_empty()) else { return text.to_string() };
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(&home) {
        let after = &rest[at + home.len()..];
        let whole = after.chars().next().is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '-' || c == '.'));
        out.push_str(&rest[..at]);
        out.push_str(if whole { "~" } else { &home });
        rest = after;
    }
    out.push_str(rest);
    out
}

/// `path` for the log, with the home folder written as `~`.
pub(crate) fn home_short(path: &Path) -> String {
    shorten_home(path, std::env::home_dir().as_deref())
}

/// Writes every panic to the log, then runs the default hook so a panic still prints as before.
pub(crate) fn log_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("{}", panic_line(info));
        default(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::{panic_line, shorten_home, shorten_home_in, strip_quoted};
    use std::path::Path;
    use std::sync::Mutex;

    #[test]
    fn a_panic_line_has_the_message_and_the_place() {
        static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());
        let before = std::panic::take_hook();
        std::panic::set_hook(Box::new(|info| SEEN.lock().unwrap().push(panic_line(info))));
        let _ = std::panic::catch_unwind(|| panic!("bad file {}", 7));
        std::panic::set_hook(before);
        let seen = SEEN.lock().unwrap();
        let line = seen.iter().find(|l| l.contains("bad file 7")).expect("hook saw the panic");
        assert!(line.starts_with("panic: bad file 7 at ") && line.contains("logging.rs:"), "{line}");
    }

    #[test]
    fn quoted_text_is_cut_from_a_logged_panic() {
        let message = r#"called `Result::unwrap()` on an `Err` value: Io { path: "/Users/me/chat.jsonl", note: "say \"hi\" ok" } done"#;
        assert_eq!(strip_quoted(message), r#"called `…` on an `…` value: Io { path: "…", note: "…" } done"#);
        assert_eq!(strip_quoted("index out of bounds: the len is 3"), "index out of bounds: the len is 3");
        assert_eq!(strip_quoted(r#"bad line "never closed"#), r#"bad line "…""#, "an open quote hides the rest");
        assert_eq!(strip_quoted("don't panic"), "don't panic", "apostrophes are not quotes");
    }

    #[test]
    fn the_home_folder_is_written_as_a_tilde_in_the_log() {
        let home = Some(Path::new("/Users/me"));
        let sep = std::path::MAIN_SEPARATOR;
        let db = Path::new("/Users/me").join("Library").join("noactually.db");
        assert_eq!(shorten_home(&db, home), format!("~{sep}Library{sep}noactually.db"));
        assert_eq!(shorten_home(Path::new("/Users/me"), home), "~");
        assert_eq!(shorten_home(Path::new("/Users/mellow/x.db"), home), "/Users/mellow/x.db", "whole folders only");
        assert_eq!(shorten_home(&db, None), db.display().to_string());
    }

    #[test]
    fn the_home_folder_is_cut_from_any_text_that_quotes_a_path() {
        let home = Some(Path::new("/Users/me"));
        assert_eq!(shorten_home_in("can't read /Users/me/chat.jsonl: denied", home), "can't read ~/chat.jsonl: denied");
        assert_eq!(shorten_home_in("/Users/me and /Users/me/x", home), "~ and ~/x");
        assert_eq!(shorten_home_in("/Users/mellow/x /Users/me-too", home), "/Users/mellow/x /Users/me-too");
        assert_eq!(shorten_home_in("/Users/me/a", None), "/Users/me/a");
    }
}
