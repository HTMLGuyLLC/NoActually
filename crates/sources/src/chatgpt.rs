//! ChatGPT data export (Settings → Data controls → Export): `conversations.json`.
//! Each conversation is a tree; the visible thread runs from `current_node` up to the root.

use crate::export::scan_export;
use crate::{epoch_ts, file_name, walk, Event, FileScan, Kind, Result, Source};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub struct ChatGptExport;

fn text_parts(msg: &Value) -> Option<String> {
    let content = &msg["content"];
    if content["content_type"].as_str() != Some("text") {
        return None;
    }
    let parts: Vec<&str> = content["parts"].as_array()?.iter().filter_map(Value::as_str).collect();
    let text = parts.join("\n");
    (!text.trim().is_empty()).then_some(text)
}

/// Longest thread walked; a malformed export with a parent cycle stops here.
const MAX_CHAIN: usize = 100_000;

fn session(conv: &Value) -> &str {
    conv["conversation_id"].as_str().or(conv["id"].as_str()).unwrap_or_default()
}

fn events(conv: &Value) -> Vec<Event> {
    let mapping = &conv["mapping"];
    let mut chain = Vec::new();
    let mut node = conv["current_node"].as_str();
    while let Some(id) = node {
        let n = &mapping[id];
        chain.push(n);
        node = n["parent"].as_str();
        if chain.len() > MAX_CHAIN {
            break;
        }
    }
    chain.reverse();
    let mut out = Vec::new();
    for n in chain {
        let msg = &n["message"];
        if msg.is_null() || msg["metadata"]["is_visually_hidden_from_conversation"] == true {
            continue;
        }
        let Some(text) = text_parts(msg) else { continue };
        let pos = out.len() as u64;
        match msg["author"]["role"].as_str() {
            Some("user") => {
                if let Some(ts) = msg["create_time"].as_f64().or(conv["create_time"].as_f64()).and_then(epoch_ts) {
                    let key = msg["id"].as_str().map(|id| format!("chatgpt:{id}"));
                    out.push(Event::Human { text, ts, key, pos });
                }
            }
            Some("assistant") => {
                let model = msg["metadata"]["model_slug"].as_str().or(conv["default_model_slug"].as_str());
                let ts = msg["create_time"].as_f64().and_then(epoch_ts);
                let key = msg["id"].as_str().map(|id| format!("chatgpt:{id}"));
                out.push(Event::Ai { text, model: model.map(str::to_string), ts, key, pos });
            }
            _ => {}
        }
    }
    out
}

impl Source for ChatGptExport {
    fn id(&self) -> &'static str {
        "chatgpt-export"
    }
    fn name(&self) -> &'static str {
        "ChatGPT (export)"
    }
    fn kind(&self) -> Kind {
        Kind::Import
    }
    fn roots(&self) -> Vec<PathBuf> {
        Vec::new() // the user picks the file
    }
    fn files(&self, root: &Path) -> Vec<PathBuf> {
        walk(root, &|p| {
            let n = file_name(p);
            n.starts_with("conversations") && n.ends_with(".json")
        })
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
    use serde_json::json;

    fn msg(id: &str, role: &str, text: &str, hidden: bool) -> Value {
        json!({"id":id,"author":{"role":role},"create_time":1.0,"content":{"content_type":"text","parts":[text]},
            "metadata":{"model_slug":"gpt-5.1","is_visually_hidden_from_conversation":hidden}})
    }

    #[test]
    fn scans_an_export_file() {
        let dir = tempfile::tempdir().unwrap();
        let conv = json!({"conversation_id":"c1","current_node":"c","mapping":{
            "a": {"parent": null, "message": msg("a","assistant","Use pandas.read_xlsx",false)},
            "h": {"parent": "a", "message": msg("h","system","hidden context",true)},
            "b": {"parent": "h", "message": msg("b","user","that function doesn't exist",false)},
            "c": {"parent": "b", "message": msg("c","assistant","You're right.",false)}
        }});
        let p = dir.path().join("conversations.json");
        let body = serde_json::to_vec(&json!([conv])).unwrap();
        std::fs::write(&p, &body).unwrap();
        let scan = ChatGptExport.scan(&p, 0, false).unwrap();
        assert_eq!(scan.turns.len(), 1);
        assert_eq!((scan.turns[0].session.as_str(), scan.turns[0].key.as_deref()), ("c1", Some("chatgpt:b")));
        assert_eq!(scan.checkpoint as usize, body.len());

        std::fs::write(dir.path().join("conversations-002.json"), "[]").unwrap();
        std::fs::write(dir.path().join("user.json"), "{}").unwrap();
        assert_eq!(ChatGptExport.files(dir.path()).len(), 2);

        std::fs::write(&p, "{not json").unwrap();
        assert!(ChatGptExport.scan(&p, 0, false).err().unwrap().to_string().contains("not a valid export"));
    }

    #[test]
    fn hidden_messages_are_skipped_and_cycles_end() {
        let conv = json!({"current_node":"b","mapping":{
            "a": {"parent": "b", "message": msg("a","user","no, that's wrong",true)},
            "b": {"parent": "a", "message": msg("b","assistant","Sure.",false)}
        }});
        let ev = events(&conv);
        assert_eq!(ev.len(), MAX_CHAIN / 2 + 1, "walk stops at the cap, keeping only visible replies");
        assert!(ev.iter().all(|e| matches!(e, Event::Ai { .. })));
    }

    #[test]
    fn walks_the_visible_branch() {
        let conv = serde_json::json!({
            "id": "c1", "current_node": "d",
            "mapping": {
                "root": {"message": null, "parent": null},
                "a": {"parent": "root", "message": {"id":"a","author":{"role":"assistant"},"create_time":1.0,"content":{"content_type":"text","parts":["Use pandas.read_xlsx"]},"metadata":{"model_slug":"gpt-5.1"}}},
                "b": {"parent": "a", "message": {"id":"b","author":{"role":"user"},"create_time":2.0,"content":{"content_type":"text","parts":["that function doesn't exist"]},"metadata":{}}},
                "x": {"parent": "b", "message": {"id":"x","author":{"role":"assistant"},"create_time":3.0,"content":{"content_type":"text","parts":["abandoned branch"]},"metadata":{}}},
                "d": {"parent": "b", "message": {"id":"d","author":{"role":"assistant"},"create_time":3.5,"content":{"content_type":"text","parts":["You're right, it's read_excel."]},"metadata":{"model_slug":"gpt-5.1"}}}
            }
        });
        let ev = events(&conv);
        assert_eq!(ev.len(), 3);
        let scan = pair("chatgpt-export", "c1", ev, 0, true);
        let t = &scan.turns[0].turn;
        assert_eq!(t.next_ai.as_ref().unwrap().text, "You're right, it's read_excel.");
        assert_eq!(t.prev_ai.as_ref().unwrap().model.as_deref(), Some("gpt-5.1"));
        assert_eq!(engine::classify(t).label, Some(engine::Label::Confirmed));
    }
}
