//! Claude Code and Claude Cowork transcripts: append-only JSONL, one record per line.

use crate::jsonl::{for_each_line, open_at};
use crate::{contains, data_dir, home, pair, parse_ts, walk, Error, Event, FileScan, Kind, Result, Source};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub struct ClaudeCode {
    id: &'static str,
    name: &'static str,
    roots: Vec<PathBuf>,
}

impl ClaudeCode {
    pub fn code() -> Self {
        let roots = home().map(|h| h.join(".claude").join("projects")).into_iter().collect();
        Self { id: "claude-code", name: "Claude Code", roots }
    }
    /// Claude Code reading from `root` instead of the real home folder (tests, custom installs).
    pub fn code_at(root: PathBuf) -> Self {
        Self { roots: vec![root], ..Self::code() }
    }
    pub fn cowork() -> Self {
        let roots = data_dir().map(|d| d.join("Claude").join("local-agent-mode-sessions")).into_iter().collect();
        Self { id: "claude-cowork", name: "Claude Cowork", roots }
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
            // IDEs add the open file or selection as its own block: context, not something typed.
            let parts: Vec<&str> = blocks
                .iter()
                .filter(|b| b.kind == "text")
                .filter_map(|b| b.text.as_deref())
                .filter(|t| !t.trim_start().starts_with("<ide_"))
                .collect();
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
        let mut events = Vec::new();
        let end = for_each_line(open_at(path, from)?, from, |pos, buf| {
            if let Some(e) = event(buf, pos) {
                events.push(e);
            }
        })
        .map_err(Error::io(path))?;
        Ok(pair(self.id, &session, events, end, finalize))
    }
}

/// The event one transcript line holds, if it's a message a person typed or a model wrote.
fn event(buf: &[u8], pos: u64) -> Option<Event> {
    // Cheap filters before any JSON work. Tool results are user-role records too.
    let is_user = contains(buf, br#""type":"user""#);
    let is_ai = contains(buf, br#""type":"assistant""#);
    let is_queued = contains(buf, b"queued_command");
    if !(is_user || is_ai || is_queued) || (is_user && contains(buf, br#""tool_result""#)) {
        return None;
    }
    let line = serde_json::from_slice::<Line>(buf).ok()?;
    if line.sidechain || line.meta {
        return None;
    }
    let ts = line.timestamp.as_deref().and_then(parse_ts);
    match line.kind.as_deref()? {
        "user" => {
            let text = line.message.as_ref().and_then(|m| m.content.as_ref()).and_then(text_of)?;
            (!is_noise(&text)).then_some(Event::Human { text, ts: ts?, key: line.uuid, pos })
        }
        // A message typed while Claude was busy, delivered later as an attachment.
        "attachment" => {
            let a = line.attachment?;
            let Some(serde_json::Value::String(text)) = a.prompt else { return None };
            (a.kind.as_deref() == Some("queued_command") && !is_noise(&text)).then_some(Event::Human {
                text,
                ts: ts?,
                key: line.uuid,
                pos,
            })
        }
        "assistant" => {
            let m = line.message?;
            // "<synthetic>" replies (errors, interruptions, "No response requested") were
            // written by Claude Code, not a model: they're neither a reply nor an admission.
            if m.model.as_deref().is_some_and(|id| !engine::is_real_model(id)) {
                return None;
            }
            let text = m.content.as_ref().and_then(text_of)?;
            Some(Event::Ai { text, model: m.model, pos })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{cc_ai, cc_user, line};
    use serde_json::json;

    const TS: &str = "2026-10-01T10:00:00Z";

    fn scan_str(body: &str, finalize: bool) -> FileScan {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("abc.jsonl");
        std::fs::write(&p, body).unwrap();
        ClaudeCode::code().scan(&p, 0, finalize).unwrap()
    }

    #[test]
    fn reads_turns_and_skips_noise() {
        let body = cc_ai("claude-opus-5-5", TS, "Go to /admin.")
            + &cc_user("u1", TS, "that's not a real page")
            + &line(
                &json!({"type":"user","timestamp":TS,"message":{"content":[{"type":"tool_result","content":"x"}]}}),
            )
            + &cc_user("u2", TS, "<system-reminder>hi</system-reminder>")
            + &line(&json!({"type":"user","timestamp":TS,"message":{"content":[
                {"type":"text","text":"<ide_opened_file>The user opened src/a.ts</ide_opened_file>"}]}}))
            + &line(&json!({"type":"user","timestamp":TS,"isSidechain":true,"message":{"content":"subagent prompt"}}))
            + &cc_ai("claude-opus-5-5", TS, "You're right.")
            + "{\"type\":\"user\",\"partial"; // half-written line
        let scan = scan_str(&body, false);
        assert_eq!(scan.turns.len(), 1);
        let t = &scan.turns[0].turn;
        assert_eq!(t.human, "that's not a real page");
        assert_eq!(t.next_ai.as_ref().unwrap().text, "You're right.");
        assert_eq!(engine::classify(t).label, Some(engine::Label::Confirmed));
        // It resumes at the last reply (re-read as context), never past the partial line.
        let last_reply = body.rfind(&cc_ai("claude-opus-5-5", TS, "You're right.")).unwrap();
        assert_eq!(scan.checkpoint as usize, last_reply, "must not skip the partial line");
    }

    #[test]
    fn queued_commands_count_as_typed() {
        let queued = line(&json!({"type":"attachment","uuid":"q1","timestamp":TS,
            "attachment":{"type":"queued_command","prompt":"you missed the tests"}}));
        let body = cc_ai("claude-opus-5-5", TS, "Done.") + &queued + &cc_ai("claude-opus-5-5", TS, "Adding them.");
        let scan = scan_str(&body, false);
        assert_eq!(scan.turns.len(), 1);
        assert_eq!(scan.turns[0].turn.human, "you missed the tests");
        assert_eq!(scan.turns[0].key.as_deref(), Some("q1"));
    }

    #[test]
    fn synthetic_replies_are_skipped() {
        let body = cc_ai("claude-opus-5-5", TS, "Use /admin.")
            + &cc_user("u1", TS, "that's not a real page")
            + &cc_ai("<synthetic>", TS, "No response requested.")
            + &cc_ai("claude-opus-5-5", TS, "You're right.");
        let scan = scan_str(&body, false);
        assert_eq!(scan.turns.len(), 1);
        assert_eq!(scan.turns[0].turn.next_ai.as_ref().unwrap().text, "You're right.", "placeholder is not the reply");
    }

    #[test]
    fn subagent_transcripts_are_not_listed() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("p").join("subagents");
        std::fs::create_dir_all(&sub).unwrap();
        for p in [dir.path().join("p/a.jsonl"), sub.join("b.jsonl"), dir.path().join("p/notes.txt")] {
            std::fs::write(p, "").unwrap();
        }
        let files = ClaudeCode::code().files(dir.path());
        assert_eq!(files, vec![dir.path().join("p/a.jsonl")]);
    }
}
