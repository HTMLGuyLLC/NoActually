//! One-off data exports: a `conversations.json` array, read in full every time it changes.

use crate::{pair, Error, Event, FileScan, Result, MAX_READ_BYTES};
use serde::de::{Deserializer, SeqAccess, Visitor};
use serde_json::Value;
use std::io::{BufReader, Read};
use std::path::Path;

/// Calls `each` with every element of the top-level array in `reader`, one at a time, so only one
/// conversation is held in memory (not the whole export as a tree of values).
fn each_element<R: Read>(reader: R, mut each: impl FnMut(Value)) -> serde_json::Result<()> {
    struct Elements<'a, F>(&'a mut F);
    impl<'de, F: FnMut(Value)> Visitor<'de> for Elements<'_, F> {
        type Value = ();
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("an array")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
            while let Some(v) = seq.next_element::<Value>()? {
                (self.0)(v);
            }
            Ok(())
        }
    }
    let mut de = serde_json::Deserializer::from_reader(reader);
    de.deserialize_seq(Elements(&mut each))?;
    de.end()
}

/// Reads an export: each conversation's events (from `events`) become turns in the session
/// `session(conv)` names. The checkpoint is the file size, since exports are never appended to.
/// The file is read as a stream and never past [`MAX_READ_BYTES`].
pub(crate) fn scan_export(
    source: &'static str,
    path: &Path,
    session: fn(&Value) -> &str,
    events: fn(&Value) -> Vec<Event>,
) -> Result<FileScan> {
    scan_export_capped(source, path, MAX_READ_BYTES, session, events)
}

fn scan_export_capped(
    source: &'static str,
    path: &Path,
    limit: u64,
    session: fn(&Value) -> &str,
    events: fn(&Value) -> Vec<Event>,
) -> Result<FileScan> {
    let file = std::fs::File::open(path).map_err(Error::io(path))?;
    let mut counted = Counted { inner: BufReader::new(file.take(limit + 1)), read: 0 };
    let mut scan = FileScan::default();
    let parsed = each_element(&mut counted, |c| {
        let one = pair(source, session(&c), events(&c), 0, true);
        scan.turns.extend(one.turns);
        scan.sessions.extend(one.sessions);
        scan.cards.extend(one.cards);
    });
    // Past the cap the stream is cut off, which also looks like a broken file: say which it was.
    if counted.read > limit {
        return Err(Error::TooBig { path: path.to_path_buf(), limit });
    }
    parsed.map_err(|source| Error::Json { path: path.to_path_buf(), source })?;
    scan.checkpoint = counted.read;
    Ok(scan)
}

/// A reader that counts the bytes taken through it.
struct Counted<R> {
    inner: R,
    read: u64,
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn id(c: &Value) -> &str {
        c["id"].as_str().unwrap_or_default()
    }

    fn events(c: &Value) -> Vec<Event> {
        let ts = chrono::DateTime::from_timestamp(1, 0).unwrap();
        let text = |k: &str| c[k].as_str().unwrap_or_default().to_string();
        vec![
            Event::Ai { text: text("ai"), model: None, ts: Some(ts), key: None, pos: 0 },
            Event::Human { text: text("human"), ts, key: Some(text("id")), pos: 1 },
            Event::Ai { text: "You're right.".into(), model: None, ts: Some(ts), key: None, pos: 2 },
        ]
    }

    fn write(dir: &tempfile::TempDir, body: &[u8]) -> std::path::PathBuf {
        let p = dir.path().join("conversations.json");
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn an_export_over_the_cap_is_refused_with_a_plain_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(&dir, b"[1, 2, 3, 4, 5]");
        assert!(scan_export_capped("x", &p, 15, id, events).is_ok(), "exactly at the cap is fine");
        assert!(matches!(scan_export_capped("x", &p, 14, id, events), Err(Error::TooBig { .. })));
        let body = serde_json::to_vec(&json!([{"id":"a","ai":"Use x","human":"no, wrong"}])).unwrap();
        let p = write(&dir, &body);
        assert_eq!(scan_export_capped("x", &p, body.len() as u64, id, events).unwrap().checkpoint, body.len() as u64);
        let err = scan_export_capped("x", &p, body.len() as u64 - 1, id, events).unwrap_err();
        assert!(matches!(err, Error::TooBig { .. }));
        assert!(err.to_string().contains("conversations.json: too large to read"), "{err}");
    }

    #[test]
    fn many_conversations_stream_through_one_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let convs: Vec<Value> =
            (0..50).map(|i| json!({"id": format!("c{i}"), "ai": "Use x", "human": format!("no, wrong {i}")})).collect();
        let body = serde_json::to_vec_pretty(&convs).unwrap();
        let scan = scan_export("x", &write(&dir, &body), id, events).unwrap();
        assert_eq!(scan.checkpoint, body.len() as u64);
        let want: Vec<_> = convs.iter().flat_map(|c| pair("x", id(c), events(c), 0, true).turns).collect();
        assert_eq!(scan.turns.len(), 50);
        let key = |t: &crate::SourceTurn| (t.session.clone(), t.key.clone(), t.turn.human.clone());
        assert_eq!(scan.turns.iter().map(key).collect::<Vec<_>>(), want.iter().map(key).collect::<Vec<_>>());
        assert_eq!(scan.turns[49].session, "c49");
    }

    #[test]
    fn a_broken_or_wrong_shaped_export_is_a_plain_json_error() {
        let dir = tempfile::tempdir().unwrap();
        for body in [&b"{not json"[..], b"{}", b"[{\"id\":\"a\"}", b"[] trailing", b""] {
            let err = scan_export("x", &write(&dir, body), id, events).unwrap_err();
            assert!(matches!(err, Error::Json { .. }), "{err}");
        }
        assert!(scan_export("x", &write(&dir, b"[]"), id, events).unwrap().turns.is_empty());
    }
}
