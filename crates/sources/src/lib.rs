//! Source adapters. Each one knows where an AI tool keeps its history and how to read it.
//! Adapters never decide what counts as a correction; they only produce `Turn`s for the engine.
//!
//! Every adapter implements [`Source`]. [`all`] lists them in display order, [`find`] looks one
//! up by id. Shared reading helpers live in `jsonl` (append-only logs) and `export` (one-off
//! `conversations.json` files); [`pair`] turns a stream of [`Event`]s into turns.
//!
//! Tests: each module keeps its unit tests inline in `mod tests`, writing real files to a temp
//! dir. Fixture builders shared with other crates live in [`testing`] (feature `test-support`).

#![warn(missing_docs)]

mod chatgpt;
mod claude_ai;
mod claude_code;
mod codex;
mod export;
mod jsonl;
mod pair;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
mod view;

pub use pair::{pair, Event};
pub use view::{ChatView, PromptView};

use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

/// What can go wrong reading a source. File errors always carry the path.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The file couldn't be opened or read.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file being read.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// A whole-file export isn't the JSON it should be.
    #[error("{}: not a valid export: {source}", path.display())]
    Json {
        /// The file being read.
        path: PathBuf,
        /// The underlying error.
        source: serde_json::Error,
    },
    /// A compressed rollout couldn't be decompressed.
    #[error("{}: can't decompress: {source}", path.display())]
    Zstd {
        /// The file being read.
        path: PathBuf,
        /// The underlying error.
        source: ruzstd::decoding::errors::FrameDecoderError,
    },
    /// No adapter has this id.
    #[error("unknown source '{0}'")]
    UnknownSource(String),
}

impl Error {
    pub(crate) fn io(path: &Path) -> impl FnOnce(std::io::Error) -> Error + '_ {
        move |source| Error::Io { path: path.to_path_buf(), source }
    }
}

/// Shorthand for results from this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// A human message ready to classify, plus where it came from.
#[derive(Debug, Clone)]
pub struct SourceTurn {
    /// The adapter's id.
    pub source: &'static str,
    /// The chat this message belongs to, as the tool names it.
    pub session: String,
    /// When the person sent it.
    pub ts: DateTime<Utc>,
    /// Stable id of the human message, used to skip copies (resumed or forked sessions).
    ///
    /// Each adapter keeps the shape it shipped with: Claude Code uses the bare record uuid, Codex
    /// `<session>:<id>`, exports `chatgpt:<id>` / `claude-ai:<id>`. The store hashes it together
    /// with the source id, so the shapes never collide. Changing one changes its hash and would
    /// count history twice, so a change needs an `engine::RULES_VERSION` bump (which rebuilds).
    pub key: Option<String>,
    /// The turn the engine classifies.
    pub turn: engine::Turn,
}

/// What one pass over a file produced.
#[derive(Debug, Default)]
pub struct FileScan {
    /// Complete turns, in file order.
    pub turns: Vec<SourceTurn>,
    /// Where the next pass should resume. Only ever past turns that are complete.
    pub checkpoint: u64,
    /// Record blocks found in AI replies, in file order. A pass that re-reads a reply (after a
    /// checkpoint rewind) can report its blocks again; whoever stores them dedupes.
    pub cards: Vec<CardSeen>,
}

/// A record block (a fenced `noactually` block) found in an AI reply. Not yet validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardSeen {
    /// The adapter's id.
    pub source: &'static str,
    /// The chat the reply belongs to, as the tool names it.
    pub session: String,
    /// The tool's own id for the reply, if it gave one.
    pub key: Option<String>,
    /// When the reply was written, if the tool said.
    pub ts: Option<DateTime<Utc>>,
    /// Where the reply starts in the file.
    pub pos: u64,
    /// The text between the fences, as written.
    pub body: String,
}

/// The start of the message the Stop hook sends to have a finished task graded. Matched as a
/// prefix; the task id follows. Messages like it are the tool talking, not the person.
pub const AUTO_GRADE_PROMPT: &str = "Use the grade:me skill in short mode for task ";

/// True for a message that asks for a report card rather than being the person's own words:
/// the grade prompt, bare or as a Stop hook's feedback. Another hook's feedback is not one.
pub(crate) fn is_grade_request(text: &str) -> bool {
    let t = text.trim_start();
    let t = t.strip_prefix("Stop hook feedback:").map_or(t, str::trim_start);
    t.starts_with(AUTO_GRADE_PROMPT)
}

/// How a source's files arrive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Files the tool keeps appending to; tracked live.
    Live,
    /// A one-off export the user picks; imported once.
    Import,
}

/// One AI tool's history format.
pub trait Source: Send + Sync {
    /// Stable id, stored with every count ("claude-code").
    fn id(&self) -> &'static str;
    /// Display name ("Claude Code").
    fn name(&self) -> &'static str;
    /// Live (tracked) or import (picked once).
    fn kind(&self) -> Kind;
    /// Folders this source reads, shown to the user before they allow it.
    fn roots(&self) -> Vec<PathBuf>;
    /// Every history file under a root.
    fn files(&self, root: &Path) -> Vec<PathBuf>;
    /// Read `path` starting at `from` (a byte offset for append-only files, 0 for whole files).
    /// `finalize` emits a trailing message that never got a reply (the file has gone quiet).
    fn scan(&self, path: &Path, from: u64, finalize: bool) -> Result<FileScan>;
    /// Append-only sources resume from a byte offset; others are re-read whole when they change.
    fn append_only(&self, path: &Path) -> bool;
    /// The whole chat in `path` with every prompt, what it cost and any report cards. `None` for
    /// sources that can't say.
    fn view(&self, _path: &Path) -> Option<Result<ChatView>> {
        None
    }
}

/// Every adapter, in display order.
pub fn all() -> Vec<Box<dyn Source>> {
    vec![
        Box::new(claude_code::ClaudeCode::code()),
        Box::new(claude_code::ClaudeCode::cowork()),
        Box::new(codex::Codex),
        Box::new(chatgpt::ChatGptExport),
        Box::new(claude_ai::ClaudeAiExport),
    ]
}

/// The adapter with this id among `all`.
pub fn find<'a>(all: &'a [Box<dyn Source>], id: &str) -> Result<&'a dyn Source> {
    all.iter().find(|s| s.id() == id).map(AsRef::as_ref).ok_or_else(|| Error::UnknownSource(id.to_string()))
}

/// Claude Code rooted somewhere other than `~/.claude/projects`.
pub fn claude_code_at(root: PathBuf) -> Box<dyn Source> {
    Box::new(claude_code::ClaudeCode::code_at(root))
}

/// The home folder. `None` when the OS can't say, so a source never reads a relative path.
pub(crate) fn home() -> Option<PathBuf> {
    dirs::home_dir().filter(|p| p.is_absolute())
}

/// Per-user app data: ~/Library/Application Support, %APPDATA%, or ~/.local/share.
pub(crate) fn data_dir() -> Option<PathBuf> {
    dirs::data_dir().filter(|p| p.is_absolute()).or_else(home)
}

/// Recursively collects files under `root` whose name passes `keep`. Skips unreadable dirs.
pub(crate) fn walk(root: &Path, keep: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(p),
                Ok(t) if t.is_file() && keep(&p) => out.push(p),
                _ => {}
            }
        }
    }
    out.sort();
    out
}

/// True if `needle` occurs anywhere in `hay`. The cheap pre-filter before any JSON parsing.
pub(crate) fn contains(hay: &[u8], needle: &[u8]) -> bool {
    memchr::memmem::find(hay, needle).is_some()
}

/// The file's name without its directory, or "" for a path that has none.
pub(crate) fn file_name(p: &Path) -> std::borrow::Cow<'_, str> {
    p.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
}

pub(crate) fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc))
}

pub(crate) fn epoch_ts(secs: f64) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp_millis((secs * 1000.0) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_names_unknown_ids() {
        let all = all();
        assert_eq!(find(&all, "codex").unwrap().name(), "Codex");
        let err = find(&all, "nope").err().unwrap();
        assert_eq!(err.to_string(), "unknown source 'nope'");
    }

    #[test]
    fn ids_are_unique_and_roots_absolute() {
        let all = all();
        let mut ids: Vec<_> = all.iter().map(|s| s.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), all.len());
        assert!(all.iter().flat_map(|s| s.roots()).all(|r| r.is_absolute()), "never a relative root");
    }

    #[test]
    fn contains_finds_needles() {
        assert!(contains(br#"{"type":"user"}"#, br#""type":"user""#));
        assert!(!contains(b"abc", b"abcd"));
    }
}
