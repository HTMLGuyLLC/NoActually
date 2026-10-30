//! Fixture builders for tests in this crate and the store's (feature `test-support`).
//! Records are shaped like the real tools write them, minus everything the adapters ignore.

use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, SystemTime};

/// One JSONL line: `v` plus a newline.
pub fn line(v: &Value) -> String {
    format!("{v}\n")
}

/// A Claude Code record the person typed.
pub fn cc_user(uuid: &str, ts: &str, text: &str) -> String {
    line(&json!({"type":"user","uuid":uuid,"timestamp":ts,"message":{"content":text}}))
}

/// A Claude Code reply from `model`.
pub fn cc_ai(model: &str, ts: &str, text: &str) -> String {
    line(&json!({"type":"assistant","timestamp":ts,"message":{"model":model,"content":[{"type":"text","text":text}]}}))
}

/// A Claude Code correction: "no, that's wrong" answered by "You're right." from Claude Opus 5.5.
pub fn cc_correction(uuid: &str, ts: &str) -> String {
    cc_user(uuid, ts, "no, that's wrong") + &cc_ai("claude-opus-5-5", ts, "You're right.")
}

/// Sets a file's modified time to `ago` before now, so tests see a change (or a quiet file)
/// without sleeping.
///
/// # Panics
/// If the file can't be opened or its time set.
pub fn set_mtime_ago(path: &Path, ago: Duration) {
    let f = std::fs::OpenOptions::new().append(true).open(path).expect("open fixture");
    f.set_modified(SystemTime::now() - ago).expect("set mtime");
}

/// `data` compressed as zstd, like Codex's archived `.jsonl.zst` rollouts.
pub fn zstd(data: &[u8]) -> Vec<u8> {
    ruzstd::encoding::compress_to_vec(data, ruzstd::encoding::CompressionLevel::Fastest)
}
