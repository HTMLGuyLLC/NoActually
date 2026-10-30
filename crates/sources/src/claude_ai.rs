//! Claude.ai data export (Settings → Privacy → Export data): `conversations.json`.
//! The export doesn't record which model replied, so these corrections roll up under "Claude".

use crate::export::scan_export;
use crate::{parse_ts, walk, Event, FileScan, Kind, Result, Source};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub struct ClaudeAiExport;

fn message_text(m: &Value) -> Option<String> {
    let blocks: Vec<&str> = m["content"]
        .as_array()
        .map(|a| a.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect())
        .unwrap_or_default();
    let text = if blocks.is_empty() { m["text"].as_str().unwrap_or_default().to_string() } else { blocks.join("\n") };
    (!text.trim().is_empty()).then_some(text)
}

fn session(conv: &Value) -> &str {
    conv["uuid"].as_str().unwrap_or_default()
}

fn events(conv: &Value) -> Vec<Event> {
    let mut out = Vec::new();
    for m in conv["chat_messages"].as_array().into_iter().flatten() {
        let Some(text) = message_text(m) else { continue };
        let pos = out.len() as u64;
        match m["sender"].as_str() {
            Some("human") => {
                if let Some(ts) = m["created_at"].as_str().and_then(parse_ts) {
                    let key = m["uuid"].as_str().map(|id| format!("claude-ai:{id}"));
                    out.push(Event::Human { text, ts, key, pos });
                }
            }
            Some("assistant") => {
                let ts = m["created_at"].as_str().and_then(parse_ts);
                let key = m["uuid"].as_str().map(|id| format!("claude-ai:{id}"));
                out.push(Event::Ai { text, model: Some("claude".into()), ts, key, pos });
            }
            _ => {}
        }
    }
    out
}

impl Source for ClaudeAiExport {
    fn id(&self) -> &'static str {
        "claude-ai-export"
    }
    fn name(&self) -> &'static str {
        "Claude.ai (export)"
    }
    fn kind(&self) -> Kind {
        Kind::Import
    }
    fn roots(&self) -> Vec<PathBuf> {
        Vec::new()
    }
    fn files(&self, root: &Path) -> Vec<PathBuf> {
        walk(root, &|p| p.file_name().is_some_and(|n| n == "conversations.json"))
    }
    fn append_only(&self, _: &Path) -> bool {
        false
    }
    fn scan(&self, path: &Path, _from: u64, _finalize: bool) -> Result<FileScan> {
        scan_export(self.id(), path, session, events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pair;

    #[test]
    fn scans_an_export_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("conversations.json");
        let convs = serde_json::json!([{"uuid":"c","chat_messages":[
            {"uuid":"1","sender":"assistant","text":"Use /admin","created_at":"2026-01-01T00:00:00Z"},
            {"uuid":"2","sender":"human","text":"that page doesn't exist","created_at":"2026-01-01T00:01:00Z"},
            {"uuid":"3","sender":"assistant","text":"You're right.","created_at":"2026-01-01T00:02:00Z"}
        ]}]);
        std::fs::write(&p, convs.to_string()).unwrap();
        std::fs::write(dir.path().join("projects.json"), "[]").unwrap();
        assert_eq!(ClaudeAiExport.files(dir.path()), vec![p.clone()]);
        let scan = ClaudeAiExport.scan(&p, 0, false).unwrap();
        assert_eq!(scan.turns.len(), 1);
        let t = &scan.turns[0];
        assert_eq!((t.source, t.session.as_str(), t.key.as_deref()), ("claude-ai-export", "c", Some("claude-ai:2")));
        assert_eq!(t.turn.prev_ai.as_ref().unwrap().model.as_deref(), Some("claude"));
    }

    #[test]
    fn reads_human_and_assistant() {
        let conv = serde_json::json!({"uuid":"c","chat_messages":[
            {"uuid":"1","sender":"assistant","text":"It's in /etc/app.conf","created_at":"2026-01-01T00:00:00Z"},
            {"uuid":"2","sender":"human","text":"no, that file isn't there","created_at":"2026-01-01T00:01:00Z"},
            {"uuid":"3","sender":"assistant","content":[{"type":"text","text":"My mistake, it's in ~/.app."}],"created_at":"2026-01-01T00:02:00Z"}
        ]});
        let scan = pair("claude-ai-export", "c", events(&conv), 0, true);
        assert_eq!(scan.turns.len(), 1);
        assert_eq!(engine::classify(&scan.turns[0].turn).label, Some(engine::Label::Confirmed));
    }
}
