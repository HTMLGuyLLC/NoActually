//! Claude Code and Claude Cowork transcripts: append-only JSONL, one record per line.

use crate::jsonl::{for_each_line, open_at};
use crate::view::{build, Cut, Sample};
use crate::{
    contains, data_dir, home, is_grade_request, pair, parse_ts, walk, ChatView, Error, Event, FileScan, Kind, Result,
    Source,
};
use engine::{head_chars, Usage};
use serde::Deserialize;
use std::collections::HashMap;
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
    id: Option<String>,
    content: Option<Content>,
    model: Option<String>,
    usage: Option<RawUsage>,
}

/// The token counts Claude Code records on a reply.
#[derive(Deserialize)]
#[allow(clippy::struct_field_names)] // these are the tool's names
struct RawUsage {
    input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    output_tokens: Option<u64>,
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
    NOISE.iter().any(|n| t.starts_with(n)) || is_grade_request(t)
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

    fn view(&self, path: &Path) -> Option<Result<ChatView>> {
        Some(self.read_view(path))
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

impl ClaudeCode {
    /// Reads the whole file: every prompt, the tokens each one cost, and any report cards.
    fn read_view(&self, path: &Path) -> Result<ChatView> {
        let session = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut events = Vec::new();
        let mut samples: Vec<Sample> = Vec::new();
        let mut cuts = Vec::new();
        // One reply is written as several records that repeat the message id and usage.
        let mut by_id: HashMap<String, usize> = HashMap::new();
        let end = for_each_line(open_at(path, 0)?, 0, |pos, buf| {
            if let Some(e) = event(buf, pos) {
                events.push(e);
            }
            cuts.extend(cut(buf, pos));
            if let Some((id, s)) = sample(buf, pos) {
                if let Some(&i) = id.as_ref().and_then(|id| by_id.get(id)) {
                    let model = s.model.clone().or(samples[i].model.take());
                    samples[i] = Sample { model, ..s }; // the last record has the final counts
                } else {
                    if let Some(id) = id {
                        by_id.insert(id, samples.len());
                    }
                    samples.push(s);
                }
            }
        })
        .map_err(Error::io(path))?;
        Ok(build(self.id, &session, events, samples, cuts, end))
    }
}

/// A record that starts report-card work: the grade slash command, or a grade request.
fn cut(buf: &[u8], pos: u64) -> Option<Cut> {
    if !contains(buf, b"grade:me") {
        return None;
    }
    let line = serde_json::from_slice::<Line>(buf).ok()?;
    if line.sidechain {
        return None;
    }
    let text = match line.attachment.and_then(|a| a.prompt) {
        Some(serde_json::Value::String(t)) => t,
        _ => line.message?.content.as_ref().and_then(text_of)?,
    };
    // Skills arrive as "<command-message>grade:me</command-message>\n<command-name>/grade:me</command-name>".
    let slash = text.trim_start().starts_with("<command-") && head_chars(&text, 200).contains("grade:me</command-");
    (slash || is_grade_request(&text)).then(|| Cut { pos, ts: line.timestamp.as_deref().and_then(parse_ts) })
}

/// What a reply cost, from a non-sidechain `assistant` record (tool-only ones included).
fn sample(buf: &[u8], pos: u64) -> Option<(Option<String>, Sample)> {
    if !contains(buf, br#""type":"assistant""#) {
        return None;
    }
    let line = serde_json::from_slice::<Line>(buf).ok()?;
    if line.sidechain || line.kind.as_deref() != Some("assistant") {
        return None;
    }
    let m = line.message?;
    if m.model.as_deref().is_some_and(|id| !engine::is_real_model(id)) {
        return None;
    }
    let u = m.usage?;
    let usage = Usage {
        input: u.input_tokens.unwrap_or(0),
        cache_write: u.cache_creation_input_tokens.unwrap_or(0),
        cache_read: u.cache_read_input_tokens.unwrap_or(0),
        output: u.output_tokens.unwrap_or(0),
    };
    let ts = line.timestamp.as_deref().and_then(parse_ts);
    Some((m.id, Sample { pos, ts, model: m.model, usage }))
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
            Some(Event::Ai { text, model: m.model, ts, key: line.uuid, pos })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{cc_ai, cc_user, line};
    use crate::AUTO_GRADE_PROMPT;
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

    const FENCE: &str = "```noactually\n{\"v\":1}\n```";

    /// A reply record with its own uuid, API message id and token counts. `text` None is tool-only.
    fn reply(uuid: &str, msg: &str, ts: &str, text: Option<&str>, usage: (u64, u64, u64, u64)) -> String {
        let content = match text {
            Some(t) => json!([{"type":"text","text":t}]),
            None => json!([{"type":"tool_use","id":"t","name":"Bash","input":{}}]),
        };
        line(&json!({"type":"assistant","uuid":uuid,"timestamp":ts,"message":{"id":msg,"model":"claude-opus-5-5",
            "content":content,"usage":{"input_tokens":usage.0,"cache_creation_input_tokens":usage.1,
            "cache_read_input_tokens":usage.2,"output_tokens":usage.3}}}))
    }

    fn view_str(body: &str) -> ChatView {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("abc.jsonl");
        std::fs::write(&p, body).unwrap();
        ClaudeCode::code().view(&p).unwrap().unwrap()
    }

    #[test]
    fn a_card_is_pulled_out_and_hidden_from_scoring() {
        let card = format!("Report.\n{FENCE}");
        let body = cc_user("u1", TS, "build it")
            + &reply("a1", "m1", TS, Some(&card), (1, 0, 0, 1))
            + &cc_user("u2", TS, "no, that's wrong")
            + &cc_ai("claude-opus-5-5", TS, "You're right.");
        let scan = scan_str(&body, false);
        assert_eq!(scan.cards.len(), 1);
        let c = &scan.cards[0];
        assert_eq!((c.source, c.session.as_str(), c.key.as_deref()), ("claude-code", "abc", Some("a1")));
        assert!(c.ts.is_some() && c.body.contains("\"v\":1"));
        // The card reply is empty to the turn that follows it, and the first turn's next_ai.
        assert_eq!(scan.turns[0].turn.next_ai.as_ref().unwrap().text, "");
        assert_eq!(scan.turns[1].turn.prev_ai.as_ref().unwrap().text, "");
    }

    #[test]
    fn fences_from_people_and_tools_are_not_cards() {
        let body = cc_user("u1", TS, &format!("here is an example:\n{FENCE}"))
            + &line(&json!({"type":"user","timestamp":TS,"message":{"content":[
                {"type":"tool_result","content":FENCE}]}}))
            + &cc_ai("claude-opus-5-5", TS, "ok");
        let scan = scan_str(&body, true);
        assert_eq!(scan.cards.len(), 0);
        assert!(scan.turns[0].turn.human.contains("```noactually"), "the person's text is untouched");
    }

    #[test]
    fn a_resumed_copy_gives_the_same_card_key() {
        let card = reply("a1", "m1", TS, Some(FENCE), (1, 0, 0, 1));
        let first = scan_str(&(cc_user("u1", TS, "go") + &card), false);
        let copy = scan_str(&(cc_user("u0", TS, "other") + &card), false);
        assert_eq!(first.cards[0].key, copy.cards[0].key);
        assert_eq!(first.cards[0].body, copy.cards[0].body);
    }

    #[test]
    fn hook_and_grade_messages_are_not_the_persons() {
        let body = cc_user("u1", TS, "build the thing")
            + &cc_ai("claude-opus-5-5", TS, "Done.")
            + &cc_user("u2", TS, &format!("{AUTO_GRADE_PROMPT}7"))
            + &cc_user("u3", TS, "Stop hook feedback:\nUse the grade:me skill in short mode for task 7")
            + &cc_ai("claude-opus-5-5", TS, "Card.");
        let scan = scan_str(&body, true);
        assert_eq!(scan.turns.len(), 1);
        assert_eq!(scan.turns[0].turn.human, "build the thing");
    }

    #[test]
    fn view_counts_each_reply_once_and_files_it_under_its_prompt() {
        let t = |m: u32| format!("2026-10-01T10:{m:02}:00Z");
        let body = reply("a0", "m0", &t(0), Some("early"), (9, 9, 9, 9)) // before any prompt: dropped
            + &cc_user("u1", &t(1), "build a settings page with tabs")
            + &reply("a1", "m1", &t(2), Some("Looking."), (3, 5, 7, 1)) // one reply, split in two
            + &reply("a2", "m1", &t(3), None, (3, 5, 7, 40)) // same message id: last usage wins
            + &reply("a3", "m2", &t(4), None, (2, 0, 100, 10)) // tool-only, still counts
            + &cc_user("u2", &t(50), "thanks")
            + &reply("a4", "m3", &t(52), Some("Welcome."), (1, 1, 1, 1));
        let v = view_str(&body);
        assert_eq!((v.source, v.session.as_str()), ("claude-code", "abc"));
        assert_eq!(v.prompts.len(), 2);
        let p = &v.prompts[0];
        assert_eq!(p.usage, Usage { input: 5, cache_write: 5, cache_read: 107, output: 50 });
        assert_eq!(p.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(p.minutes, 30, "49 minutes to the next prompt, capped");
        assert_eq!(p.text, "build a settings page with tabs");
        assert_eq!(p.id, engine::short_id(&p.key));
        let last = &v.prompts[1];
        assert_eq!(last.usage, Usage { input: 1, cache_write: 1, cache_read: 1, output: 1 });
        assert_eq!(last.minutes, 2, "the last round ends at its last record");
    }

    #[test]
    fn view_keys_match_the_stores_including_the_no_id_fallback() {
        let no_id = line(&json!({"type":"user","timestamp":TS,"message":{"content":"second, no uuid"}}));
        let body = cc_user("u1", TS, "first") + &cc_ai("claude-opus-5-5", TS, "ok") + &no_id;
        let v = view_str(&body);
        let scan = scan_str(&body, true);
        for (p, st) in v.prompts.iter().zip(&scan.turns) {
            let want =
                engine::message_key("claude-code", "abc", st.key.as_deref(), &st.ts.to_rfc3339(), &st.turn.human);
            assert_eq!(p.key, want);
        }
        assert_eq!(v.prompts.len(), 2, "the trailing unanswered message is included");
        assert_eq!(
            v.prompts[1].key,
            engine::message_key("claude-code", "abc", None, &v.prompts[1].ts.to_rfc3339(), "second, no uuid")
        );
    }

    #[test]
    fn only_our_stop_feedback_is_dropped() {
        let ours = format!("Stop hook feedback:\n{AUTO_GRADE_PROMPT}7");
        let body = cc_user("u1", TS, "build the thing")
            + &cc_ai("claude-opus-5-5", TS, "Done.")
            + &cc_user("u2", TS, &ours)
            + &cc_user("u3", TS, "Stop hook feedback:\nlint failed, fix the tests")
            + &cc_user("u4", TS, "please mention Stop hook feedback: in the docs")
            + &cc_ai("claude-opus-5-5", TS, "ok");
        let humans: Vec<_> = scan_str(&body, true).turns.iter().map(|t| t.turn.human.clone()).collect();
        assert_eq!(
            humans,
            [
                "build the thing",
                "Stop hook feedback:\nlint failed, fix the tests",
                "please mention Stop hook feedback: in the docs"
            ]
        );
    }

    #[test]
    fn a_mere_mention_of_the_fence_keeps_the_reply_text() {
        let body = cc_user("u1", TS, "go")
            + &cc_ai("claude-opus-5-5", TS, "I will end with a ```noactually block.")
            + &cc_user("u2", TS, "ok");
        let scan = scan_str(&body, true);
        assert_eq!(scan.cards.len(), 0);
        assert_eq!(scan.turns[1].turn.prev_ai.as_ref().unwrap().text, "I will end with a ```noactually block.");
    }

    #[test]
    fn grading_work_is_not_billed_to_the_previous_prompt() {
        let t = |m: u32| format!("2026-10-01T10:{m:02}:00Z");
        let slash = line(&json!({"type":"user","uuid":"c","timestamp":t(5),"message":{"content":
            "<command-message>grade:me</command-message>\n<command-name>/grade:me</command-name>"}}));
        let cuts = [
            slash,
            cc_user("c", &t(5), &format!("Stop hook feedback:\n{AUTO_GRADE_PROMPT}7")),
            cc_user("c", &t(5), &format!("{AUTO_GRADE_PROMPT}7")),
            // A reply holding a card is the cut itself.
            reply("c", "mc", &t(5), Some(FENCE), (500, 0, 0, 500)),
        ];
        for cut in cuts {
            let body = cc_user("u1", &t(1), "build a settings page with tabs")
                + &reply("a1", "m1", &t(3), Some("Done."), (1, 2, 3, 4))
                + &cut
                + &reply("a2", "m2", &t(8), Some("Grading."), (500, 500, 500, 500))
                + &cc_user("u2", &t(20), "thanks")
                + &reply("a3", "m3", &t(21), Some("Welcome."), (1, 1, 1, 1));
            let v = view_str(&body);
            assert_eq!(v.prompts.len(), 2, "{cut}");
            assert_eq!(v.prompts[0].usage, Usage { input: 1, cache_write: 2, cache_read: 3, output: 4 }, "{cut}");
            assert_eq!(v.prompts[0].minutes, 4, "the round ends at the cut: {cut}");
            assert_eq!(v.prompts[1].usage.input, 1, "{cut}");
        }
    }

    #[test]
    fn other_noise_is_not_a_cut() {
        let t = |m: u32| format!("2026-10-01T10:{m:02}:00Z");
        let body = cc_user("u1", &t(1), "build a settings page with tabs")
            + &reply("a1", "m1", &t(2), Some("Done."), (1, 0, 0, 1))
            + &cc_user("n", &t(3), "<system-reminder>hi</system-reminder>")
            + &reply("a2", "m2", &t(4), Some("More."), (1, 0, 0, 1));
        let v = view_str(&body);
        assert_eq!(v.prompts[0].usage.input, 2);
        assert_eq!(v.prompts[0].minutes, 3);
    }

    #[test]
    fn view_skips_sidechain_and_synthetic_usage() {
        let side = line(
            &json!({"type":"assistant","isSidechain":true,"timestamp":TS,"message":{"id":"s","model":"claude-opus-5-5",
            "content":[{"type":"text","text":"sub"}],"usage":{"input_tokens":99,"output_tokens":99}}}),
        );
        let synth = line(&json!({"type":"assistant","timestamp":TS,"message":{"id":"y","model":"<synthetic>",
            "content":[{"type":"text","text":"No response requested."}],"usage":{"input_tokens":77,"output_tokens":77}}}));
        let body = cc_user("u1", TS, "build it") + &side + &synth + &reply("a1", "m1", TS, Some("ok"), (1, 0, 0, 2));
        assert_eq!(view_str(&body).prompts[0].usage, Usage { input: 1, cache_write: 0, cache_read: 0, output: 2 });
    }

    #[test]
    fn a_queued_command_is_a_round_boundary() {
        let queued = line(&json!({"type":"attachment","uuid":"q1","timestamp":TS,
            "attachment":{"type":"queued_command","prompt":"also add tests"}}));
        let body = cc_user("u1", TS, "build it")
            + &reply("a1", "m1", TS, Some("a"), (1, 0, 0, 1))
            + &queued
            + &reply("a2", "m2", TS, Some("b"), (10, 0, 0, 10));
        let v = view_str(&body);
        assert_eq!(v.prompts.len(), 2);
        assert_eq!((v.prompts[0].usage.input, v.prompts[1].usage.input), (1, 10));
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
