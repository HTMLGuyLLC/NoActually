//! Claude.ai data export (Settings → Privacy → Export data): `conversations.json`.
//! The export doesn't record which model replied, so these catches roll up under "Claude".

use crate::{pair, parse_ts, Event, FileScan, Kind, Source};
use anyhow::Result;
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

pub(crate) fn events(conv: &Value) -> Vec<Event> {
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
            Some("assistant") => out.push(Event::Ai { text, model: Some("claude".into()), pos }),
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
        crate::walk(root, &|p| p.file_name().is_some_and(|n| n == "conversations.json"))
    }
    fn append_only(&self, _: &Path) -> bool {
        false
    }
    fn scan(&self, path: &Path, _from: u64, _finalize: bool) -> Result<FileScan> {
        let convs: Vec<Value> = serde_json::from_slice(&std::fs::read(path)?)?;
        let mut scan = FileScan::default();
        for c in &convs {
            let id = c["uuid"].as_str().unwrap_or_default();
            scan.turns.extend(pair("claude-ai-export", id, events(c), 0, true).turns);
        }
        scan.checkpoint = std::fs::metadata(path)?.len();
        Ok(scan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
