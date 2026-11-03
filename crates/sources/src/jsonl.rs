//! Reading append-only JSONL logs one complete line at a time.

use crate::{Error, Result};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

/// Opens `path` for line reading, positioned at byte `from`.
pub(crate) fn open_at(path: &Path, from: u64) -> Result<BufReader<File>> {
    let mut f = File::open(path).map_err(Error::io(path))?;
    f.seek(SeekFrom::Start(from)).map_err(Error::io(path))?;
    Ok(BufReader::with_capacity(1 << 16, f))
}

/// The longest line read, newline included. A longer one is a corrupt or hostile file, not a message.
const MAX_LINE: usize = 64 << 20;

/// Calls `f(line_pos, line)` for each complete line (newline included) of `reader`, whose first
/// byte sits at offset `start`. Stops at EOF or at a last line still being written, and returns
/// the offset just past the last complete line: where the next pass should resume.
///
/// A line over [`MAX_LINE`] is skipped to its newline and reported as an empty `line`, which no real
/// line is. If the file ends inside one, the pass resumes at the end of the file, so the unfinished
/// line isn't read into memory again on every pass.
pub(crate) fn for_each_line<R: BufRead>(reader: R, start: u64, f: impl FnMut(u64, &[u8])) -> io::Result<u64> {
    for_each_line_capped(reader, start, MAX_LINE, f)
}

fn for_each_line_capped<R: BufRead>(
    mut reader: R,
    start: u64,
    cap: usize,
    mut f: impl FnMut(u64, &[u8]),
) -> io::Result<u64> {
    let mut pos = start;
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let n = Read::take(&mut reader, cap as u64 + 1).read_until(b'\n', &mut buf)?;
        if n == 0 {
            return Ok(pos);
        }
        if n > cap {
            let mut ended = buf.last() == Some(&b'\n');
            drop(std::mem::take(&mut buf)); // give the memory back before reading on
            let mut skipped = n as u64;
            while !ended {
                let chunk = reader.fill_buf()?;
                if chunk.is_empty() {
                    break;
                }
                let (used, found) =
                    chunk.iter().position(|&b| b == b'\n').map_or((chunk.len(), false), |i| (i + 1, true));
                reader.consume(used);
                skipped += used as u64;
                ended = found;
            }
            f(pos, b"");
            pos += skipped;
            if !ended {
                return Ok(pos); // EOF inside it
            }
            continue;
        }
        if buf.last() != Some(&b'\n') {
            return Ok(pos); // EOF, or a line still being written: resume here next time
        }
        f(pos, &buf);
        pos += n as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stops_before_a_partial_line() {
        let mut seen = Vec::new();
        let end = for_each_line(&b"ab\ncd\nef"[..], 10, |pos, line| seen.push((pos, line.to_vec()))).unwrap();
        assert_eq!(seen, vec![(10, b"ab\n".to_vec()), (13, b"cd\n".to_vec())]);
        assert_eq!(end, 16);
        assert_eq!(for_each_line(&b""[..], 4, |_, _| {}).unwrap(), 4);
    }

    #[test]
    fn a_line_over_the_cap_is_skipped_and_reported_empty() {
        let mut seen = Vec::new();
        let body = &b"ab\nTOO-LONG-LINE\nok\n"[..];
        let end = for_each_line_capped(body, 0, 5, |pos, line| seen.push((pos, line.to_vec()))).unwrap();
        assert_eq!(seen, vec![(0, b"ab\n".to_vec()), (3, vec![]), (17, b"ok\n".to_vec())]);
        assert_eq!(end, 20);
        // Exactly at the cap is still a line; one more byte is not.
        let mut seen = Vec::new();
        for_each_line_capped(&b"abcd\nabcde\n"[..], 0, 5, |pos, line| seen.push((pos, line.len()))).unwrap();
        assert_eq!(seen, vec![(0, 5), (5, 0)]);
    }

    #[test]
    fn an_unfinished_line_over_the_cap_is_skipped_once() {
        let mut seen = Vec::new();
        let end =
            for_each_line_capped(&b"ab\nXXXXXXXXXXXX"[..], 0, 5, |pos, line| seen.push((pos, line.len()))).unwrap();
        assert_eq!((seen, end), (vec![(0, 3), (3, 0)], 15), "resumes past it, not at its start");
        // A short unfinished line still waits for its newline.
        assert_eq!(for_each_line_capped(&b"ab\nXX"[..], 0, 5, |_, _| {}).unwrap(), 3);
    }

    #[test]
    fn open_at_resumes_mid_file_and_names_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.jsonl");
        std::fs::write(&p, "one\ntwo\n").unwrap();
        let mut lines = Vec::new();
        let end = for_each_line(open_at(&p, 4).unwrap(), 4, |_, l| lines.push(l.to_vec())).unwrap();
        assert_eq!((lines, end), (vec![b"two\n".to_vec()], 8));
        let missing = dir.path().join("missing.jsonl");
        assert!(open_at(&missing, 0).err().unwrap().to_string().contains("missing.jsonl"));
    }
}
