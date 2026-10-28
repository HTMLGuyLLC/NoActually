//! Claude Code and Claude Cowork transcripts: append-only JSONL, one record per line.

use crate::{data_dir, home, pair, parse_ts, walk, Event, FileScan, Kind, Source};
use anyhow::Result;
use serde::Deserialize;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub struct ClaudeCode {
    id: &'static str,
    name: &'static str,
    roots: Vec<PathBuf>,
}

impl ClaudeCode {
    pub fn code() -> Self {
        Self { id: "claude-code", name: "Claude Code", roots: vec![home().join(".claude").join("projects")] }
    }
    /// Claude Code reading from `root` instead of the real home folder (tests, custom installs).
    pub fn code_at(root: PathBuf) -> Self {
        Self { roots: vec![root], ..Self::code() }
    }
    pub fn cowork() -> Self {
        Self {
            id: "claude-cowork",
            name: "Claude Cowork",
            roots: vec![data_dir().join("Claude").join("local-agent-mode-sessions")],
        }
    }
}

/// Messages Claude Code injects as the "user" that a person never typed.
const NOISE: &[&str] = &[
    "<command-",
    "<local-command",
    "<system-reminder",
    "<task-notification",
    "<bash-",
    "[Request interrupted",
    "Caveat:",
    "This session is being continued",
    "<user-prompt-submit-hook",
    "<side-sessions-event",
    "<ci-monitor-event",
    "[Artifact comment",
];

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    kind: Option<String>,
    uuid: Option<String>,
    timestamp: Option<String>,
    #[serde(rename = "isSidechain", default)]
    sidechain: bool,
    #[serde(rename = "isMeta", default)]
    meta: bool,
    message: Option<Message>,
    attachment: Option<Attachment>,
}

#[derive(Deserialize)]
struct Message {
    content: Option<Content>,
    model: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Content {
    Text(String),
    Blocks(Vec<Block>),
}

#[derive(Deserialize)]
struct Block {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct Attachment {
    #[serde(rename = "type")]
    kind: Option<String>,
    prompt: Option<serde_json::Value>,
}

fn text_of(content: &Content) -> Option<String> {
    match content {
        Content::Text(s) => Some(s.clone()),
        Content::Blocks(blocks) => {
            if blocks.iter().any(|b| b.kind == "tool_result") {
                return None;
            }
            let parts: Vec<&str> =
                blocks.iter().filter(|b| b.kind == "text").filter_map(|b| b.text.as_deref()).collect();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
    }
}

fn is_noise(s: &str) -> bool {
    let t = s.trim_start();
    NOISE.iter().any(|n| t.starts_with(n))
}

impl Source for ClaudeCode {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        self.name
    }
    fn kind(&self) -> Kind {
        Kind::Live
    }
    fn roots(&self) -> Vec<PathBuf> {
        self.roots.clone()
    }
    fn files(&self, root: &Path) -> Vec<PathBuf> {
        walk(root, &|p| {
            p.extension().is_some_and(|e| e == "jsonl") && !p.components().any(|c| c.as_os_str() == "subagents")
        })
    }
    fn append_only(&self, _: &Path) -> bool {
        true
    }

    fn scan(&self, path: &Path, from: u64, finalize: bool) -> Result<FileScan> {
        let session = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut f = std::fs::File::open(path)?;
        f.seek(SeekFrom::Start(from))?;
        let mut reader = BufReader::with_capacity(1 << 16, f);
        let mut events = Vec::new();
        let mut pos = from;
        let mut buf = Vec::new();
        loop {
            buf.clear();
            let n = reader.read_until(b'\n', &mut buf)?;
            if n == 0 || buf.last() != Some(&b'\n') {
                break; // EOF, or a line still being written: resume here next time
            }
            let line_pos = pos;
            pos += n as u64;
            // Cheap filters before any JSON work. Tool results are user-role records too.
            let is_user = memfind(&buf, br#""type":"user""#);
            let is_ai = memfind(&buf, br#""type":"assistant""#);
            let is_queued = memfind(&buf, b"queued_command");
            if !(is_user || is_ai || is_queued) || (is_user && memfind(&buf, br#""tool_result""#)) {
                continue;
            }
            let Ok(line) = serde_json::from_slice::<Line>(&buf) else { continue };
            if line.sidechain || line.meta {
                continue;
            }
            let ts = line.timestamp.as_deref().and_then(parse_ts);
            match line.kind.as_deref() {
                Some("user") => {
                    let Some(text) = line.message.as_ref().and_then(|m| m.content.as_ref()).and_then(text_of) else {
                        continue;
                    };
                    if let (false, Some(ts)) = (is_noise(&text), ts) {
                        events.push(Event::Human { text, ts, key: line.uuid, pos: line_pos });
                    }
                }
                Some("attachment") => {
                    let Some(a) = line.attachment else { continue };
                    let Some(serde_json::Value::String(text)) = a.prompt else { continue };
                    if a.kind.as_deref() == Some("queued_command") && !is_noise(&text) {
                        if let Some(ts) = ts {
                            events.push(Event::Human { text, ts, key: line.uuid, pos: line_pos });
                        }
                    }
                }
                Some("assistant") => {
                    let Some(m) = line.message else { continue };
                    // "<synthetic>" replies (errors, interruptions, "No response requested") were
                    // written by Claude Code, not a model: they're neither a reply nor an admission.
                    if m.model.as_deref().is_some_and(|id| !engine::is_real_model(id)) {
                        continue;
                    }
                    if let Some(text) = m.content.as_ref().and_then(text_of) {
                        events.push(Event::Ai { text, model: m.model, pos: line_pos });
                    }
                }
                _ => {}
            }
        }
        Ok(pair(self.id, &session, events, pos, finalize))
    }
}

pub(crate) fn memfind(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn line(v: serde_json::Value) -> String {
        format!("{}\n", v)
    }

    #[test]
    fn reads_turns_and_skips_noise() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("abc.jsonl");
        let ts = "2026-10-01T10:00:00Z";
        let mut f = std::fs::File::create(&p).unwrap();
        for l in [
            line(
                serde_json::json!({"type":"assistant","timestamp":ts,"message":{"model":"claude-opus-5-5","content":[{"type":"text","text":"Go to /admin."}]}}),
            ),
            line(
                serde_json::json!({"type":"user","uuid":"u1","timestamp":ts,"message":{"content":"that's not a real page"}}),
            ),
            line(
                serde_json::json!({"type":"user","timestamp":ts,"message":{"content":[{"type":"tool_result","content":"x"}]}}),
            ),
            line(
                serde_json::json!({"type":"user","timestamp":ts,"message":{"content":"<system-reminder>hi</system-reminder>"}}),
            ),
            line(
                serde_json::json!({"type":"user","timestamp":ts,"isSidechain":true,"message":{"content":"subagent prompt"}}),
            ),
            line(
                serde_json::json!({"type":"assistant","timestamp":ts,"message":{"model":"claude-opus-5-5","content":[{"type":"text","text":"You're right."}]}}),
            ),
        ] {
            f.write_all(l.as_bytes()).unwrap();
        }
        f.write_all(b"{\"type\":\"user\",\"partial").unwrap(); // half-written line
        let scan = ClaudeCode::code().scan(&p, 0, false).unwrap();
        assert_eq!(scan.turns.len(), 1);
        let t = &scan.turns[0].turn;
        assert_eq!(t.human, "that's not a real page");
        assert_eq!(t.next_ai.as_ref().unwrap().text, "You're right.");
        assert_eq!(engine::classify(t).label, Some(engine::Label::Confirmed));
        let len = std::fs::metadata(&p).unwrap().len();
        assert!(scan.checkpoint < len, "must not skip the partial line");
    }
}
