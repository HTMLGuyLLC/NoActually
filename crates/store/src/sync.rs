//! One sync pass: for every allowed source, read whatever changed since last time.
//! Called on launch, on file-change events, and on a timer as a safety net.

use crate::{cards, source_files, FileMeta, Pass, Result, SourceState, Store};
use rayon::prelude::*;
use serde::Serialize;
use sources::{FileScan, Source};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// A file untouched this long is done: its last unanswered message is counted as-is.
const QUIET: Duration = Duration::from_secs(15 * 60);
/// Files parsed in parallel before their results are written. Bounds memory on a big backfill.
const CHUNK: usize = 64;

/// What one pass did.
#[derive(Debug, Default, Serialize)]
pub struct SyncReport {
    /// Files read and saved.
    pub files_read: usize,
    /// New messages counted.
    pub messages: usize,
    /// New corrections counted.
    pub corrections: usize,
    /// One line per file that was skipped, with the reason.
    pub errors: Vec<String>,
    /// Scores were rebuilt because the rules changed since they were counted.
    pub rescored: bool,
}

struct Job {
    path: PathBuf,
    from: u64,
    pass: Pass,
}

/// Reads every allowed, unpaused source among `all`. Per-file problems land in
/// [`SyncReport::errors`]; only database-wide failures return `Err`.
pub fn sync(store: &mut Store, all: &[Box<dyn Source>]) -> Result<SyncReport> {
    let mut report = SyncReport { rescored: store.ensure_rules_current()?, ..SyncReport::default() };
    let now = SystemTime::now();
    for state in store.source_states()? {
        if !state.allowed || state.paused {
            continue;
        }
        let Ok(src) = sources::find(all, &state.id) else { continue }; // from a newer or older build
        let jobs = plan(store, src, &state, now, &mut report);
        // Parsing is pure and parallel; writes go through one connection, one file per transaction.
        for chunk in jobs.chunks(CHUNK) {
            let scans: Vec<_> = chunk
                .par_iter()
                .map(|j| {
                    let scan = src.scan(&j.path, j.from, j.pass.finalized).map(|scan| tips_if_gradable(src, scan));
                    // Report cards are rare. Only a file with one not read yet is read again, whole.
                    let chat = scan
                        .as_ref()
                        .ok()
                        .filter(|s| cards::unread(&s.cards, j.pass.cards_to))
                        .and_then(|_| src.view(&j.path));
                    (j, scan, chat)
                })
                .collect();
            for (job, scan, chat) in scans {
                let saved = scan.map_err(|e| e.to_string()).and_then(|scan| {
                    // A file that can't be read again keeps its checkpoint, so its cards aren't lost.
                    let chat = chat.transpose().map_err(|e| e.to_string())?;
                    // One file failing to save (a locked database, say) must not stop the rest.
                    store
                        .ingest(&state, &job.path.to_string_lossy(), &scan, chat.as_ref(), job.pass)
                        .map_err(|e| format!("{}: couldn't save: {e}", job.path.display()))
                });
                match saved {
                    Ok((m, c)) => {
                        report.files_read += 1;
                        report.messages += m;
                        report.corrections += c;
                    }
                    Err(e) => report.errors.push(e),
                }
            }
        }
    }
    store.sweep(chrono::Utc::now())?;
    Ok(report)
}

/// `scan`, without its tips when `src` can't have any: the same adapter reads chats it can't give rules to.
fn tips_if_gradable(src: &dyn Source, mut scan: FileScan) -> FileScan {
    if !src.gradable() {
        scan.tips.clear();
        scan.rules.clear();
    }
    scan
}

/// The files of `src` that changed since they were last read, and where to resume each.
fn plan(store: &Store, src: &dyn Source, state: &SourceState, now: SystemTime, report: &mut SyncReport) -> Vec<Job> {
    let mut jobs = Vec::new();
    for path in source_files(src, &state.paths) {
        let Ok((meta, modified)) = FileMeta::read(&path) else { continue };
        let prev = match store.file_checkpoint(&path.to_string_lossy()) {
            Ok(prev) => prev,
            Err(e) => {
                report.errors.push(format!("{}: couldn't read its checkpoint: {e}", path.display()));
                continue;
            }
        };
        let quiet = now.duration_since(modified).unwrap_or_default() > QUIET;
        let was = prev.map(|c| (c.at, c.meta, c.finalized));
        if let Some(from) = resume_from(was, meta, quiet, src.append_only(&path)) {
            // A pass that starts over reads every card again; one that resumes skips what it has read.
            let cards_to = if from == 0 { None } else { prev.and_then(|c| c.cards_to) };
            jobs.push(Job { path, from, pass: Pass { meta, finalized: quiet, cards_to } });
        }
    }
    jobs
}

/// Where to start reading a file, or `None` to skip it. `prev` is (checkpoint, size and mtime
/// then, and whether that pass was final); `quiet` means it hasn't changed in a while, so a last
/// unanswered message is final.
fn resume_from(prev: Option<(u64, FileMeta, bool)>, now: FileMeta, quiet: bool, append_only: bool) -> Option<u64> {
    match prev {
        Some((_, then, finalized)) if then == now && (!quiet || finalized) => None, // unchanged
        Some((cp, then, _)) if append_only && now.size >= then.size && cp <= now.size => {
            // Grew (or just went quiet): resume at the checkpoint, unless it's all read already.
            (cp != now.size || then.size != now.size).then_some(cp)
        }
        Some((_, then, _)) if then == now => None,
        _ => Some(0), // new, rewritten, or shrank: re-read; message keys stop double counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sources::{RulesSeen, TipSeen};

    #[test]
    fn only_sources_that_can_have_tips_keep_them() {
        let seen = TipSeen {
            session: "s".into(),
            key: None,
            ts: chrono::Utc::now(),
            pos: 0,
            model: None,
            tip: engine::parse_tip("Tip from No, Actually...: say more. [target, redid 50%]").unwrap(),
        };
        let rules = RulesSeen { session: "s".into(), ts: seen.ts };
        for src in sources::all() {
            let scan = FileScan { tips: vec![seen.clone()], rules: vec![rules.clone()], ..FileScan::default() };
            let kept = tips_if_gradable(&*src, scan);
            assert_eq!(kept.tips.len() == 1, src.gradable(), "{}", src.id());
            assert_eq!(!kept.rules.is_empty(), src.gradable(), "{}", src.id());
        }
    }

    fn m(size: u64, mtime: i64) -> FileMeta {
        FileMeta { size, mtime }
    }

    #[test]
    fn resume_points() {
        assert_eq!(resume_from(None, m(10, 1), false, true), Some(0), "new file");
        assert_eq!(resume_from(Some((10, m(10, 1), false)), m(10, 1), false, true), None, "unchanged");
        assert_eq!(resume_from(Some((6, m(10, 1), false)), m(20, 2), false, true), Some(6), "appended");
        assert_eq!(
            resume_from(Some((6, m(10, 1), false)), m(10, 1), true, true),
            Some(6),
            "went quiet: finalize the tail"
        );
        assert_eq!(resume_from(Some((10, m(10, 1), false)), m(10, 1), true, true), None, "quiet and fully read");
        assert_eq!(resume_from(Some((10, m(10, 1), false)), m(4, 2), false, true), Some(0), "shrank");
        assert_eq!(resume_from(Some((10, m(10, 1), false)), m(12, 2), false, false), Some(0), "whole files re-read");
        assert_eq!(resume_from(Some((10, m(10, 1), false)), m(10, 1), true, false), None, "quiet whole file unchanged");
        assert_eq!(
            resume_from(Some((6, m(10, 1), true)), m(10, 1), true, true),
            None,
            "quiet, finalized, checkpoint < size"
        );
        assert_eq!(resume_from(Some((6, m(10, 1), true)), m(20, 2), false, true), Some(6), "finalized, then appended");
    }
}
