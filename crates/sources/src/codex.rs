//! Codex (CLI and desktop) rollouts: JSONL, older days compressed as `.jsonl.zst`.

use crate::claude_code::memfind;
use crate::{home, pair, parse_ts, walk, Event, FileScan, Kind, Source};
use anyhow::Result;
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
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
        vec![home().join(".codex").join("sessions"), home().join(".codex").join("archived_sessions")]
    }
    fn files(&self, root: &Path) -> Vec<PathBuf> {
        walk(root, &|p| {
            let n = p.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
            n.starts_with("rollout-") && (n.ends_with(".jsonl") || n.ends_with(".jsonl.zst"))
        })
    }
    fn append_only(&self, path: &Path) -> bool {
        !is_zst(path)
    }

    fn scan(&self, path: &Path, from: u64, finalize: bool) -> Result<FileScan> {
        let file = std::fs::File::open(path)?;
        let (mut reader, from): (Box<dyn BufRead>, u64) = if is_zst(path) {
            let mut raw = Vec::new();
            ruzstd::StreamingDecoder::new(file)?.read_to_end(&mut raw)?;
            (Box::new(std::io::Cursor::new(raw)), 0)
        } else {
            let mut f = file;
            f.seek(SeekFrom::Start(0))?;
            (Box::new(BufReader::with_capacity(1 << 16, f)), from)
        };
        // Compressed files are finished sessions: read whole, always final.
        let finalize = finalize || is_zst(path);
        // The filename's uuid is the session id; it's stable no matter where a pass resumes.
        let session = path
            .file_name()
            .map(|s| s.to_string_lossy().trim_end_matches(".zst").trim_end_matches(".jsonl").to_string())
            .unwrap_or_default();
        let mut model: Option<String> = None;
        let mut human = true;
        let mut events = Vec::new();
        let mut pos = 0u64;
        let mut buf = Vec::new();
        loop {
            buf.clear();
            let n = reader.read_until(b'\n', &mut buf)?;
            if n == 0 || buf.last() != Some(&b'\n') {
                break;
            }
            let line_pos = pos;
            pos += n as u64;
            // Lines before the checkpoint were already emitted; read them only for session
            // metadata and the current model.
            let meta = memfind(&buf, b"session_meta") || memfind(&buf, b"turn_context");
            if !(meta || (line_pos >= from && memfind(&buf, b"\"message\""))) {
                continue;
            }
            let Ok(v) = serde_json::from_slice::<Value>(&buf) else { continue };
            let payload = &v["payload"];
            match v["type"].as_str() {
                Some("session_meta") => human = human_thread(payload),
                Some("turn_context") => {
                    if let Some(m) = payload["model"].as_str() {
                        model = Some(m.to_string());
                    }
                }
                Some("response_item") if payload["type"] == "message" && line_pos >= from => {
                    let ts = v["timestamp"].as_str().and_then(parse_ts);
                    match payload["role"].as_str() {
                        Some("user") if human => {
                            let Some(text) = text_items(&payload["content"], "input_text") else { continue };
                            let text = strip_images(strip_preamble(&text));
                            if text.is_empty() || NOISE.iter().any(|n| text.starts_with(n)) {
                                continue;
                            }
                            if let Some(ts) = ts {
                                let key = payload["id"].as_str().map(|id| format!("{session}:{id}"));
                                events.push(Event::Human { text: text.to_string(), ts, key, pos: line_pos });
                            }
                        }
                        Some("assistant") => {
                            if let Some(text) = text_items(&payload["content"], "output_text") {
                                events.push(Event::Ai { text, model: model.clone(), pos: line_pos });
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        Ok(pair("codex", &session, events, pos, finalize))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

#[cfg(test)]
mod file_tests {
    use super::*;
    use std::io::Write;

    fn line(v: serde_json::Value) -> String {
        format!("{v}\n")
    }

    fn msg(role: &str, kind: &str, text: &str, id: &str) -> String {
        line(serde_json::json!({"timestamp":"2026-10-20T10:00:00Z","type":"response_item",
            "payload":{"type":"message","role":role,"id":id,"content":[{"type":kind,"text":text}]}}))
    }

    #[test]
    fn resuming_mid_file_keeps_the_model_and_emits_once() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("rollout-2026-10-20-abc.jsonl");
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(line(serde_json::json!({"type":"session_meta","payload":{"id":"abc","source":"vscode","thread_source":"user"}})).as_bytes()).unwrap();
        f.write_all(line(serde_json::json!({"type":"turn_context","payload":{"model":"gpt-6-astra"}})).as_bytes())
            .unwrap();
        f.write_all(msg("user", "input_text", "<environment_context>cwd</environment_context>", "e").as_bytes())
            .unwrap();
        f.write_all(msg("assistant", "output_text", "Use read_xlsx.", "a1").as_bytes()).unwrap();
        f.write_all(msg("user", "input_text", "that function doesn't exist", "u1").as_bytes()).unwrap();
        let first = Codex.scan(&p, 0, false).unwrap();
        assert!(first.turns.is_empty(), "waits for the reply");

        f.write_all(msg("assistant", "output_text", "You're right, it's read_excel.", "a2").as_bytes()).unwrap();
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
        let body = line(
            serde_json::json!({"type":"session_meta","payload":{"id":"x","source":{"subagent":{"other":"guardian"}}}}),
        ) + &msg("assistant", "output_text", "Plan.", "a")
            + &msg("user", "input_text", "no, that's wrong", "u")
            + &msg("assistant", "output_text", "You're right.", "b");
        std::fs::write(&p, body).unwrap();
        assert!(Codex.scan(&p, 0, true).unwrap().turns.is_empty());
    }
}
