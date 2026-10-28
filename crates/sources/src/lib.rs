//! Source adapters. Each one knows where an AI tool keeps its history and how to read it.
//! Adapters never decide what counts as a catch; they only produce `Turn`s for the engine.

mod chatgpt;
mod claude_ai;
mod claude_code;
mod codex;
mod pair;

pub use pair::{pair, Event};

use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

/// A human message ready to classify, plus where it came from.
#[derive(Debug, Clone)]
pub struct SourceTurn {
    pub source: &'static str,
    pub session: String,
    pub ts: DateTime<Utc>,
    /// Stable id of the human message, used to skip copies (resumed or forked sessions).
    pub key: Option<String>,
    pub turn: engine::Turn,
}

/// What one pass over a file produced.
#[derive(Debug, Default)]
pub struct FileScan {
    pub turns: Vec<SourceTurn>,
    /// Where the next pass should resume. Only ever past turns that are complete.
    pub checkpoint: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Files the tool keeps appending to; tracked live.
    Live,
    /// A one-off export the user picks; imported once.
    Import,
}

pub trait Source: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
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
}

pub fn all() -> Vec<Box<dyn Source>> {
    vec![
        Box::new(claude_code::ClaudeCode::code()),
        Box::new(claude_code::ClaudeCode::cowork()),
        Box::new(codex::Codex),
        Box::new(chatgpt::ChatGptExport),
        Box::new(claude_ai::ClaudeAiExport),
    ]
}

/// Claude Code rooted somewhere other than `~/.claude/projects`.
pub fn claude_code_at(root: PathBuf) -> Box<dyn Source> {
    Box::new(claude_code::ClaudeCode::code_at(root))
}

pub fn by_id(id: &str) -> Option<Box<dyn Source>> {
    all().into_iter().find(|s| s.id() == id)
}

pub(crate) fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

/// Per-user app data: ~/Library/Application Support, %APPDATA%, or ~/.local/share.
pub(crate) fn data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(home)
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

pub(crate) fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc))
}

pub(crate) fn epoch_ts(secs: f64) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp_millis((secs * 1000.0) as i64)
}
