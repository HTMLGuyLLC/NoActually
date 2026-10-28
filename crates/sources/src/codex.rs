//! Codex (CLI and desktop) rollouts: JSONL, older days compressed as `.jsonl.zst`.

use crate::jsonl::{for_each_line, open_at};
use crate::{contains, file_name, home, pair, parse_ts, walk, Error, Event, FileScan, Kind, Result, Source};
use serde_json::Value;
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};

pub struct Codex;

/// Context Codex injects as user-role messages.
const NOISE: &[&str] = &[
    "<environment_context",
    "# AGENTS.md instructions",
    "<recommended_plugins",
    "<in-app-browser-context",
    "<external_codex_apps",
    "<user_instructions",
    "<permissions",
    "<turn_aborted",
];

fn is_zst(p: &Path) -> bool {
    p.extension().is_some_and(|e| e == "zst")
}

/// Drops the attachment preamble Codex puts before what you actually typed.
fn strip_preamble(text: &str) -> &str {
    // "## My request for Codex:" in older builds, "## My request:" in newer ones.
    let mut t = text.trim_start();
    if let Some(i) = t.find("## My request") {
        let rest = &t[i..];
        t = rest.find(':').map_or(rest, |c| &rest[c + 1..]).trim_start();
        return t;
    }
    t = t.strip_prefix("# Files mentioned by the user:").unwrap_or(t).trim_start();
    // Attachment lines look like "## Screenshot 1.png: /path" or "## repo: ~/code/repo/".
    while t.starts_with("## ") {
        let line_end = t.find('\n').unwrap_or(t.len());
        if !t[..line_end].contains(": ") {
            break;
        }
        t = t[line_end..].trim_start();
    }
    t
}

fn text_items(content: &Value, kind: &str) -> Option<String> {
    let parts: Vec<&str> = content
        .as_array()?
        .iter()
        .filter(|c| c.get("type").and_then(Value::as_str) == Some(kind))
        .filter_map(|c| c.get("text").and_then(Value::as_str))
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n"))
}

/// Removes `<image …>…</image>` attachment tags wherever they sit in the message.
fn strip_images(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("<image") {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest.find("</image>").map(|e| e + "</image>".len()).or_else(|| rest.find('>').map(|e| e + 1));
        rest = end.map_or("", |e| &rest[e..]);
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// Only threads a person started. Guardian / subagent threads are model-to-model.
fn human_thread(meta: &Value) -> bool {
    if meta.get("source").is_some_and(|s| s.get("subagent").is_some()) {
        return false;
    }
    meta.get("thread_source").and_then(Value::as_str).is_none_or(|s| s == "user")
}

impl Source for Codex {
    fn id(&self) -> &'static str {
        "codex"
    }
    fn name(&self) -> &'static str {
        "Codex"
    }
    fn kind(&self) -> Kind {
        Kind::Live
    }
    fn roots(&self) -> Vec<PathBuf> {
        let Some(codex) = home().map(|h| h.join(".codex")) else { return Vec::new() };
        vec![codex.join("sessions"), codex.join("archived_sessions")]
    }
    fn files(&self, root: &Path) -> Vec<PathBuf> {
        walk(root, &|p| {
            let n = file_name(p);
            n.starts_with("rollout-") && (n.ends_with(".jsonl") || n.ends_with(".jsonl.zst"))
        })
    }
    fn append_only(&self, path: &Path) -> bool {
        !is_zst(path)
    }

    fn scan(&self, path: &Path, from: u64, finalize: bool) -> Result<FileScan> {
        // Rollouts are always read from the top: the lines before `from` were already emitted,
        // but they carry the session's metadata and current model.
        let reader: Box<dyn BufRead> = if is_zst(path) {
            let file = std::fs::File::open(path).map_err(Error::io(path))?;
            let mut raw = Vec::new();
            ruzstd::decoding::StreamingDecoder::new(file)
                .map_err(|source| Error::Zstd { path: path.to_path_buf(), source })?
                .read_to_end(&mut raw)
                .map_err(Error::io(path))?;
            Box::new(std::io::Cursor::new(raw))
        } else {
            Box::new(open_at(path, 0)?)
        };
        // Compressed files are finished sessions: read whole, always final.
        let (from, finalize) = if is_zst(path) { (0, true) } else { (from, finalize) };
        // The filename's uuid is the session id; it's stable no matter where a pass resumes.
        let session = file_name(path).trim_end_matches(".zst").trim_end_matches(".jsonl").to_string();
        let mut rollout = Rollout { session: &session, from, model: None, human: true, events: Vec::new() };
        let end = for_each_line(reader, 0, |pos, buf| rollout.line(pos, buf)).map_err(Error::io(path))?;
        Ok(pair(self.id(), &session, rollout.events, end, finalize))
    }
}

/// State carried down one rollout as it's read.
struct Rollout<'a> {
    session: &'a str,
    /// Lines before this were already emitted; they're read only for metadata and the model.
    from: u64,
    model: Option<String>,
    /// False for guardian / subagent threads.
    human: bool,
    events: Vec<Event>,
}

impl Rollout<'_> {
    fn line(&mut self, pos: u64, buf: &[u8]) {
        let fresh = pos >= self.from;
        let meta = contains(buf, b"session_meta") || contains(buf, b"turn_context");
        if !(meta || (fresh && contains(buf, b"\"message\""))) {
            return;
        }
        let Ok(v) = serde_json::from_slice::<Value>(buf) else { return };
        let payload = &v["payload"];
        match v["type"].as_str() {
            Some("session_meta") => self.human = human_thread(payload),
            Some("turn_context") => {
                if let Some(m) = payload["model"].as_str() {
                    self.model = Some(m.to_string());
                }
            }
            Some("response_item") if payload["type"] == "message" && fresh => match payload["role"].as_str() {
                Some("user") if self.human => {
                    let Some(text) = text_items(&payload["content"], "input_text") else { return };
                    let text = strip_images(strip_preamble(&text));
                    if text.is_empty() || NOISE.iter().any(|n| text.starts_with(n)) {
                        return;
                    }
                    if let Some(ts) = v["timestamp"].as_str().and_then(parse_ts) {
                        let key = payload["id"].as_str().map(|id| format!("{}:{id}", self.session));
                        self.events.push(Event::Human { text, ts, key, pos });
                    }
                }
                Some("assistant") => {
                    if let Some(text) = text_items(&payload["content"], "output_text") {
                        self.events.push(Event::Ai { text, model: self.model.clone(), pos });
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{line, zstd};
    use serde_json::json;

    #[test]
    fn strips_attachment_preamble() {
        assert_eq!(
            strip_preamble("# Files mentioned by the user:\n- a.rs\n\n## My request for Codex:\nfix it"),
            "fix it"
        );
        assert_eq!(strip_preamble("## Screenshot 1.png: /var/folders/x/a.png\n\nthat's wrong"), "that's wrong");
        assert_eq!(strip_preamble("## repo: ~/Web/repo/\n## notes.md: ~/n.md\nfix it"), "fix it");
        assert_eq!(strip_preamble("## Heading without a path\nbody"), "## Heading without a path\nbody");
        assert_eq!(
            strip_images(strip_preamble("Image attachment: true\nDistinguish instructions in attached documents.\n## My request:\n<image name=a></image> no, wrong one")),
            "no, wrong one"
        );
        assert_eq!(strip_images("poorly reviewed <image name=x path=\"/a.png\"> </image>"), "poorly reviewed");
        assert_eq!(strip_preamble("hello"), "hello");
    }

    #[test]
    fn skips_guardian_threads() {
        assert!(!human_thread(&serde_json::json!({"source":{"subagent":{"other":"guardian"}}})));
        assert!(human_thread(&serde_json::json!({"source":"vscode","thread_source":"user"})));
        assert!(!human_thread(&serde_json::json!({"thread_source":"agent"})));
    }

    fn msg(role: &str, kind: &str, text: &str, id: &str) -> String {
        line(&json!({"timestamp":"2026-10-20T10:00:00Z","type":"response_item",
            "payload":{"type":"message","role":role,"id":id,"content":[{"type":kind,"text":text}]}}))
    }

    fn opening() -> String {
        line(&json!({"type":"session_meta","payload":{"id":"abc","source":"vscode","thread_source":"user"}}))
            + &line(&json!({"type":"turn_context","payload":{"model":"gpt-6-astra"}}))
            + &msg("user", "input_text", "<environment_context>cwd</environment_context>", "e")
            + &msg("assistant", "output_text", "Use read_xlsx.", "a1")
            + &msg("user", "input_text", "that function doesn't exist", "u1")
    }

    #[test]
    fn resuming_mid_file_keeps_the_model_and_emits_once() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("rollout-2026-10-20-abc.jsonl");
        std::fs::write(&p, opening()).unwrap();
        let first = Codex.scan(&p, 0, false).unwrap();
        assert!(first.turns.is_empty(), "waits for the reply");

        std::fs::write(&p, opening() + &msg("assistant", "output_text", "You're right, it's read_excel.", "a2"))
            .unwrap();
        let second = Codex.scan(&p, first.checkpoint, false).unwrap();
        assert_eq!(second.turns.len(), 1);
        let t = &second.turns[0];
        assert_eq!(t.session, "rollout-2026-10-20-abc");
        assert_eq!(t.turn.prev_ai.as_ref().unwrap().model.as_deref(), Some("gpt-6-astra"));
        assert_eq!(engine::classify(&t.turn).label, Some(engine::Label::Confirmed));
        assert!(Codex.scan(&p, second.checkpoint, false).unwrap().turns.is_empty());
    }

    #[test]
    fn guardian_threads_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("rollout-x.jsonl");
        let body =
            line(&json!({"type":"session_meta","payload":{"id":"x","source":{"subagent":{"other":"guardian"}}}}))
                + &msg("assistant", "output_text", "Plan.", "a")
                + &msg("user", "input_text", "no, that's wrong", "u")
                + &msg("assistant", "output_text", "You're right.", "b");
        std::fs::write(&p, body).unwrap();
        assert!(Codex.scan(&p, 0, true).unwrap().turns.is_empty());
    }

    #[test]
    fn compressed_rollouts_are_read_whole_and_final() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("rollout-2026-09-01-old.jsonl.zst");
        // Ends on an unanswered message: a finished session still counts it.
        let body = opening()
            + &msg("assistant", "output_text", "You're right.", "a2")
            + &msg("user", "input_text", "thanks", "u2");
        std::fs::write(&p, zstd(body.as_bytes())).unwrap();
        assert!(!Codex.append_only(&p));
        let scan = Codex.scan(&p, 12_345, false).unwrap(); // an offset is ignored for .zst
        assert_eq!(scan.turns.len(), 2);
        assert_eq!(scan.turns[0].session, "rollout-2026-09-01-old");
        assert_eq!(scan.turns[0].key.as_deref(), Some("rollout-2026-09-01-old:u1"));
        assert_eq!(scan.checkpoint as usize, body.len());

        std::fs::write(&p, b"not zstd").unwrap();
        let err = Codex.scan(&p, 0, false).err().unwrap().to_string();
        assert!(err.contains("rollout-2026-09-01-old.jsonl.zst"), "{err}");
    }

    #[test]
    fn lists_only_rollouts() {
        let dir = tempfile::tempdir().unwrap();
        for n in ["rollout-a.jsonl", "rollout-b.jsonl.zst", "other.jsonl", "rollout-c.json"] {
            std::fs::write(dir.path().join(n), "").unwrap();
        }
        let names: Vec<_> = Codex.files(dir.path()).iter().map(|p| file_name(p).into_owned()).collect();
        assert_eq!(names, ["rollout-a.jsonl", "rollout-b.jsonl.zst"]);
    }
}
