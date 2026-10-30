//! One-off data exports: a `conversations.json` array, read whole every time it changes.

use crate::{pair, Error, Event, FileScan, Result};
use serde_json::Value;
use std::path::Path;

/// Reads an export: each conversation's events (from `events`) become turns in the session
/// `session(conv)` names. The checkpoint is the file size, since exports are never appended to.
pub(crate) fn scan_export(
    source: &'static str,
    path: &Path,
    session: fn(&Value) -> &str,
    events: fn(&Value) -> Vec<Event>,
) -> Result<FileScan> {
    let bytes = std::fs::read(path).map_err(Error::io(path))?;
    let convs: Vec<Value> =
        serde_json::from_slice(&bytes).map_err(|source| Error::Json { path: path.to_path_buf(), source })?;
    let mut scan = FileScan { turns: Vec::new(), checkpoint: bytes.len() as u64, cards: Vec::new() };
    for c in &convs {
        let one = pair(source, session(c), events(c), 0, true);
        scan.turns.extend(one.turns);
        scan.cards.extend(one.cards);
    }
    Ok(scan)
}
