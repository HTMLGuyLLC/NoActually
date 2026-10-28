//! One sync pass: for every allowed source, read whatever changed since last time.
//! Called on launch, on file-change events, and on a timer as a safety net.

use crate::Store;
use anyhow::Result;
use rayon::prelude::*;
use serde::Serialize;
use sources::{Kind, Source};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A file untouched this long is done: its last unanswered message is counted as-is.
const QUIET: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Default, Serialize)]
pub struct SyncReport {
    pub files_read: usize,
    pub messages: usize,
    pub catches: usize,
    pub errors: Vec<String>,
    /// Scores were rebuilt because the rules changed since they were counted.
    pub rescored: bool,
}

struct Job {
    path: PathBuf,
    from: u64,
    size: u64,
    mtime: i64,
    finalize: bool,
}

pub fn sync(store: &mut Store, all: &[Box<dyn Source>]) -> Result<SyncReport> {
    let mut report = SyncReport::default();
    if store.ensure_rules_current()? {
        report.rescored = true;
    }
    let now = SystemTime::now();
    for state in store.source_states()? {
        if !state.allowed || state.paused {
            continue;
        }
        let Some(src) = all.iter().find(|s| s.id() == state.id) else { continue };
        let mut roots: Vec<PathBuf> = if src.kind() == Kind::Live { src.roots() } else { Vec::new() };
        roots.extend(state.paths.iter().map(PathBuf::from));

        let mut jobs = Vec::new();
        for root in roots {
            let files = if root.is_file() { vec![root] } else { src.files(&root) };
            for path in files {
                let Ok(meta) = std::fs::metadata(&path) else { continue };
                let size = meta.len();
                let modified = meta.modified().unwrap_or(UNIX_EPOCH);
                let mtime = modified.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
                let prev = store.file_checkpoint(&path.to_string_lossy())?;
                let quiet = now.duration_since(modified).unwrap_or_default() > QUIET;
                let from = match prev {
                    Some((_, s, m)) if s == size && m == mtime && !quiet => continue, // unchanged
                    Some((cp, s, _)) if src.append_only(&path) && size >= s && cp <= size => {
                        if cp == size && s == size {
                            continue; // fully read, and still quiet
                        }
                        cp
                    }
                    Some((_, s, m)) if s == size && m == mtime => continue,
                    _ => 0, // new, rewritten, or shrank: re-read; message keys stop double counts
                };
                jobs.push(Job { path, from, size, mtime, finalize: quiet });
            }
        }

        // Parsing is pure and parallel; writes go through one connection, one file per transaction.
        // Chunks bound memory during a big first backfill.
        for chunk in jobs.chunks(64) {
            let scans: Vec<_> = chunk.par_iter().map(|j| (j, src.scan(&j.path, j.from, j.finalize))).collect();
            for (job, scan) in scans {
                match scan {
                    Ok(scan) => {
                        let (m, c) = store.ingest(
                            src.id(),
                            &job.path.to_string_lossy(),
                            &scan,
                            job.size,
                            job.mtime,
                            state.backfill_since,
                        )?;
                        report.files_read += 1;
                        report.messages += m;
                        report.catches += c;
                    }
                    Err(e) => report.errors.push(format!("{}: {e}", job.path.display())),
                }
            }
        }
    }
    store.sweep(chrono::Utc::now())?;
    Ok(report)
}
