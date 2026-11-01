//! One-off data exports: a `conversations.json` array, read whole every time it changes.

use crate::{pair, Error, Event, FileScan, Result, MAX_READ_BYTES};
use serde_json::Value;
use std::io::Read;
use std::path::Path;

/// Reads `path` whole, or says it is over `limit` bytes without reading past that.
fn read_capped(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let file = std::fs::File::open(path).map_err(Error::io(path))?;
    file.take(limit + 1).read_to_end(&mut bytes).map_err(Error::io(path))?;
    if bytes.len() as u64 > limit {
        return Err(Error::TooBig { path: path.to_path_buf(), limit });
    }
    Ok(bytes)
}

/// Reads an export: each conversation's events (from `events`) become turns in the session
/// `session(conv)` names. The checkpoint is the file size, since exports are never appended to.
pub(crate) fn scan_export(
    source: &'static str,
    path: &Path,
    session: fn(&Value) -> &str,
    events: fn(&Value) -> Vec<Event>,
) -> Result<FileScan> {
    let bytes = read_capped(path, MAX_READ_BYTES)?;
    let convs: Vec<Value> =
        serde_json::from_slice(&bytes).map_err(|source| Error::Json { path: path.to_path_buf(), source })?;
    let mut scan = FileScan { checkpoint: bytes.len() as u64, ..FileScan::default() };
    for c in &convs {
        let one = pair(source, session(c), events(c), 0, true);
        scan.turns.extend(one.turns);
        scan.cards.extend(one.cards);
    }
    Ok(scan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_export_over_the_cap_is_refused_with_a_plain_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("conversations.json");
        std::fs::write(&p, "[1, 2, 3, 4, 5]").unwrap();
        assert_eq!(read_capped(&p, 15).unwrap().len(), 15);
        let err = read_capped(&p, 14).unwrap_err();
        assert!(matches!(err, Error::TooBig { .. }));
        assert!(err.to_string().contains("conversations.json: too large to read"), "{err}");
    }
}
