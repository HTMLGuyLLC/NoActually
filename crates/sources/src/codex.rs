//! Codex (CLI and desktop) rollouts: JSONL, older days compressed as `.jsonl.zst`.

use crate::jsonl::{for_each_line, open_at};
use crate::view::{build, Cut, Sample};
use crate::{
    contains, file_name, home, is_grade_request, pair, parse_ts, walk, ChatView, Error, Event, FileScan, Kind, Result,
    Source,
};
use engine::Usage;
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

/// How the Codex skill that writes a report card is invoked.
const GRADE_SKILL: &str = "$grade-me";

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

    fn view(&self, path: &Path) -> Option<Result<ChatView>> {
        Some(Self::read(path, 0, true).map(|(rollout, session, end)| {
            build(self.id(), &session, rollout.events, rollout.samples, rollout.cuts, end)
        }))
    }

    fn scan(&self, path: &Path, from: u64, finalize: bool) -> Result<FileScan> {
        let (rollout, session, end) = Self::read(path, from, false)?;
        // Compressed files are finished sessions: read whole, always final.
        Ok(pair(self.id(), &session, rollout.events, end, finalize || is_zst(path)))
    }
}

impl Codex {
    /// Reads a rollout from the top. `usage` also collects the token counts (for the chat view).
    fn read(path: &Path, from: u64, usage: bool) -> Result<(Rollout, String, u64)> {
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
        let from = if is_zst(path) { 0 } else { from };
        // The filename's uuid is the session id; it's stable no matter where a pass resumes.
        let session = file_name(path).trim_end_matches(".zst").trim_end_matches(".jsonl").to_string();
        let mut rollout = Rollout {
            session: session.clone(),
            from,
            model: None,
            human: true,
            usage,
            last_total: Usage::default(),
            events: Vec::new(),
            samples: Vec::new(),
            cuts: Vec::new(),
        };
        let end = for_each_line(reader, 0, |pos, buf| rollout.line(pos, buf)).map_err(Error::io(path))?;
        Ok((rollout, session, end))
    }
}

/// State carried down one rollout as it's read.
struct Rollout {
    session: String,
    /// Lines before this were already emitted; they're read only for metadata and the model.
    from: u64,
    model: Option<String>,
    /// False for guardian / subagent threads.
    human: bool,
    /// Also collect token counts.
    usage: bool,
    /// The running totals at the last `token_count`.
    last_total: Usage,
    events: Vec<Event>,
    samples: Vec<Sample>,
    /// Where grade requests were dropped.
    cuts: Vec<Cut>,
}

/// Codex's running totals as [`Usage`]. Its input counts cached and cache-written tokens too, and
/// its output counts reasoning, so input is split and output taken as is.
fn total_usage(t: &Value) -> Usage {
    let n = |k: &str| t[k].as_u64().unwrap_or(0);
    let (cached, written) = (n("cached_input_tokens"), n("cache_write_input_tokens"));
    Usage {
        input: n("input_tokens").saturating_sub(cached + written),
        cache_write: written,
        cache_read: cached,
        output: n("output_tokens"),
    }
}

/// Tokens used since `b`. Totals that went down (a reset, or a forked thread) start over.
fn minus(a: &Usage, b: &Usage) -> Usage {
    let all = |u: &Usage| u.input + u.cache_write + u.cache_read + u.output;
    if all(a) < all(b) {
        return *a;
    }
    Usage {
        input: a.input.saturating_sub(b.input),
        cache_write: a.cache_write.saturating_sub(b.cache_write),
        cache_read: a.cache_read.saturating_sub(b.cache_read),
        output: a.output.saturating_sub(b.output),
    }
}

impl Rollout {
    fn line(&mut self, pos: u64, buf: &[u8]) {
        let fresh = pos >= self.from;
        let meta = contains(buf, b"session_meta") || contains(buf, b"turn_context");
        let counts = self.usage && contains(buf, b"token_count");
        if !(meta || counts || (fresh && contains(buf, b"\"message\""))) {
            return;
        }
        let Ok(v) = serde_json::from_slice::<Value>(buf) else { return };
        let payload = &v["payload"];
        match v["type"].as_str() {
            Some("event_msg") if counts && payload["type"] == "token_count" => {
                // `info` is null before the first reply. Totals only grow, so a round's cost is
                // the step from the last count.
                let total = &payload["info"]["total_token_usage"];
                if total.is_object() {
                    let now = total_usage(total);
                    let ts = v["timestamp"].as_str().and_then(parse_ts);
                    let usage = minus(&now, &self.last_total);
                    self.last_total = now;
                    self.samples.push(Sample { pos, ts, model: None, usage });
                }
            }
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
                    if text.trim_start().starts_with(GRADE_SKILL) || is_grade_request(&text) {
                        if self.usage {
                            let ts = v["timestamp"].as_str().and_then(parse_ts);
                            self.cuts.push(Cut { pos, ts });
                        }
                        return;
                    }
                    if let Some(ts) = v["timestamp"].as_str().and_then(parse_ts) {
                        let key = payload["id"].as_str().map(|id| format!("{}:{id}", self.session));
                        self.events.push(Event::Human { text, ts, key, pos });
                    }
                }
                Some("assistant") => {
                    if let Some(text) = text_items(&payload["content"], "output_text") {
                        let ts = v["timestamp"].as_str().and_then(parse_ts);
                        let key = payload["id"].as_str().map(|id| format!("{}:{id}", self.session));
                        self.events.push(Event::Ai { text, model: self.model.clone(), ts, key, pos });
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
    use crate::AUTO_GRADE_PROMPT;
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

    fn counts(ts: &str, input: u64, cached: u64, output: u64, reasoning: u64) -> String {
        line(&json!({"timestamp":ts,"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{
            "input_tokens":input,"cached_input_tokens":cached,"cache_write_input_tokens":0,
            "output_tokens":output,"reasoning_output_tokens":reasoning,"total_tokens":input+output}}}}))
    }

    fn msg_at(ts: &str, role: &str, kind: &str, text: &str, id: &str) -> String {
        line(&json!({"timestamp":ts,"type":"response_item",
            "payload":{"type":"message","role":role,"id":id,"content":[{"type":kind,"text":text}]}}))
    }

    fn rollout(body: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("rollout-2026-10-20-abc.jsonl");
        std::fs::write(&p, body).unwrap();
        (dir, p)
    }

    #[test]
    fn grade_requests_are_not_the_persons_messages() {
        let body = opening()
            + &msg("assistant", "output_text", "You're right.", "a2")
            + &msg("user", "input_text", "$grade-me", "g1")
            + &msg("user", "input_text", &format!("{AUTO_GRADE_PROMPT}7"), "g2")
            + &msg("assistant", "output_text", "Card.", "a3");
        let (_d, p) = rollout(&body);
        let scan = Codex.scan(&p, 0, true).unwrap();
        assert_eq!(scan.turns.len(), 1);
        assert_eq!(scan.turns[0].turn.human, "that function doesn't exist");
    }

    #[test]
    fn cards_are_pulled_from_replies() {
        let body = opening() + &msg("assistant", "output_text", "Report.\n```noactually\n{}\n```", "a2");
        let (_d, p) = rollout(&body);
        let scan = Codex.scan(&p, 0, true).unwrap();
        assert_eq!(scan.cards.len(), 1);
        assert_eq!(scan.cards[0].key.as_deref(), Some("rollout-2026-10-20-abc:a2"));
        assert_eq!(scan.turns[0].turn.next_ai.as_ref().unwrap().text, "");
    }

    #[test]
    fn view_turns_running_totals_into_per_round_usage() {
        let t = |m: u32| format!("2026-10-20T10:{m:02}:00Z");
        let null = line(&json!({"timestamp":t(0),"type":"event_msg","payload":{"type":"token_count","info":null}}));
        let body = line(&json!({"type":"session_meta","payload":{"id":"abc","thread_source":"user"}}))
            + &line(&json!({"type":"turn_context","payload":{"model":"gpt-6-astra"}}))
            + &null
            + &msg_at(&t(1), "user", "input_text", "build a settings page with tabs", "u1")
            + &msg_at(&t(2), "assistant", "output_text", "On it.", "a1")
            + &counts(&t(2), 1000, 600, 50, 20) // first round: from zero
            + &counts(&t(3), 1500, 900, 80, 30)
            + &msg_at(&t(40), "user", "input_text", "thanks", "u2")
            + &counts(&t(41), 1500, 900, 80, 30) // repeat of the same totals: nothing new
            + &counts(&t(42), 1800, 900, 100, 40);
        let (_d, p) = rollout(&body);
        let v = Codex.view(&p).unwrap().unwrap();
        assert_eq!(v.prompts.len(), 2);
        // Input counts cached tokens and output counts reasoning, so input is split.
        assert_eq!(v.prompts[0].usage, Usage { input: 600, cache_write: 0, cache_read: 900, output: 80 });
        assert_eq!(v.prompts[0].model.as_deref(), Some("gpt-6-astra"));
        assert_eq!(v.prompts[0].minutes, 30);
        assert_eq!(v.prompts[1].usage, Usage { input: 300, cache_write: 0, cache_read: 0, output: 20 });
        assert_eq!(v.prompts[1].minutes, 2);
        let scan = Codex.scan(&p, 0, true).unwrap();
        for (pv, st) in v.prompts.iter().zip(&scan.turns) {
            let want =
                engine::message_key("codex", &st.session, st.key.as_deref(), &st.ts.to_rfc3339(), &st.turn.human);
            assert_eq!(pv.key, want);
        }
    }

    #[test]
    fn view_handles_resets_quiet_rounds_and_repeated_totals() {
        let t = |m: u32| format!("2026-10-20T10:{m:02}:00Z");
        let body = msg_at(&t(1), "user", "input_text", "build a settings page with tabs", "u1")
            + &counts(&t(2), 1000, 0, 100, 10)
            + &msg_at(&t(3), "user", "input_text", "now add the tests", "u2") // no counts in this round
            + &msg_at(&t(4), "user", "input_text", "and the docs", "u3")
            + &counts(&t(4), 1000, 0, 100, 10) // same totals again, right at the boundary
            + &counts(&t(5), 300, 0, 50, 5); // totals went down: a reset, so these are all new
        let (_d, p) = rollout(&body);
        let v = Codex.view(&p).unwrap().unwrap();
        let used: Vec<_> = v.prompts.iter().map(|p| (p.usage.input, p.usage.output)).collect();
        assert_eq!(used, [(1000, 100), (0, 0), (300, 50)]);
    }

    #[test]
    fn view_cuts_at_grade_requests() {
        let t = |m: u32| format!("2026-10-20T10:{m:02}:00Z");
        for cut in ["  $grade-me please", &format!("{AUTO_GRADE_PROMPT}7")] {
            let body = msg_at(&t(1), "user", "input_text", "build a settings page with tabs", "u1")
                + &counts(&t(3), 100, 0, 10, 0)
                + &msg_at(&t(5), "user", "input_text", cut, "g")
                + &counts(&t(9), 900, 0, 90, 0)
                + &msg_at(&t(20), "user", "input_text", "thanks", "u2")
                + &counts(&t(21), 1000, 0, 100, 0);
            let (_d, p) = rollout(&body);
            let v = Codex.view(&p).unwrap().unwrap();
            assert_eq!(v.prompts.len(), 2, "{cut}");
            assert_eq!(v.prompts[0].usage.input, 100, "{cut}");
            assert_eq!(v.prompts[0].minutes, 4, "{cut}");
            // The grading round's tokens are skipped, but the totals still move on.
            assert_eq!(v.prompts[1].usage.input, 100, "{cut}");
        }
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
