//! One sync pass: for every allowed source, read whatever changed since last time.
//! Called on launch, on file-change events, and on a timer as a safety net.

use crate::{source_files, FileMeta, Result, SourceState, Store};
use rayon::prelude::*;
use serde::Serialize;
use sources::Source;
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
    /// New catches counted.
    pub catches: usize,
    /// One line per file that was skipped, with the reason.
    pub errors: Vec<String>,
    /// Scores were rebuilt because the rules changed since they were counted.
    pub rescored: bool,
}

struct Job {
    path: PathBuf,
    from: u64,
    meta: FileMeta,
    finalize: bool,
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
            let scans: Vec<_> = chunk.par_iter().map(|j| (j, src.scan(&j.path, j.from, j.finalize))).collect();
            for (job, scan) in scans {
                let saved = scan.map_err(|e| e.to_string()).and_then(|scan| {
                    // One file failing to save (a locked database, say) must not stop the rest.
                    store
                        .ingest(&state, &job.path.to_string_lossy(), &scan, job.meta)
                        .map_err(|e| format!("{}: couldn't save: {e}", job.path.display()))
                });
                match saved {
                    Ok((m, c)) => {
                        report.files_read += 1;
                        report.messages += m;
                        report.catches += c;
                    }
                    Err(e) => report.errors.push(e),
                }
            }
        }
    }
    store.sweep(chrono::Utc::now())?;
    Ok(report)
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
        if let Some(from) = resume_from(prev, meta, quiet, src.append_only(&path)) {
            jobs.push(Job { path, from, meta, finalize: quiet });
        }
    }
    jobs
}

/// Where to start reading a file, or `None` to skip it. `prev` is (checkpoint, size and mtime
/// then); `quiet` means it hasn't changed in a while, so a last unanswered message is final.
fn resume_from(prev: Option<(u64, FileMeta)>, now: FileMeta, quiet: bool, append_only: bool) -> Option<u64> {
    match prev {
        Some((_, then)) if then == now && !quiet => None, // unchanged
        Some((cp, then)) if append_only && now.size >= then.size && cp <= now.size => {
            // Grew (or just went quiet): resume at the checkpoint, unless it's all read already.
            (cp != now.size || then.size != now.size).then_some(cp)
        }
        Some((_, then)) if then == now => None,
        _ => Some(0), // new, rewritten, or shrank: re-read; message keys stop double counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(size: u64, mtime: i64) -> FileMeta {
        FileMeta { size, mtime }
    }

    #[test]
    fn resume_points() {
        assert_eq!(resume_from(None, m(10, 1), false, true), Some(0), "new file");
        assert_eq!(resume_from(Some((10, m(10, 1))), m(10, 1), false, true), None, "unchanged");
        assert_eq!(resume_from(Some((6, m(10, 1))), m(20, 2), false, true), Some(6), "appended");
        assert_eq!(resume_from(Some((6, m(10, 1))), m(10, 1), true, true), Some(6), "went quiet: finalize the tail");
        assert_eq!(resume_from(Some((10, m(10, 1))), m(10, 1), true, true), None, "quiet and fully read");
        assert_eq!(resume_from(Some((10, m(10, 1))), m(4, 2), false, true), Some(0), "shrank");
        assert_eq!(resume_from(Some((10, m(10, 1))), m(12, 2), false, false), Some(0), "whole files re-read");
        assert_eq!(resume_from(Some((10, m(10, 1))), m(10, 1), true, false), None, "quiet whole file unchanged");
    }
}
