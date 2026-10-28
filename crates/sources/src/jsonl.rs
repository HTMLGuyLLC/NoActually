//! Reading append-only JSONL logs one complete line at a time.

use crate::{Error, Result};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

/// Opens `path` for line reading, positioned at byte `from`.
pub(crate) fn open_at(path: &Path, from: u64) -> Result<BufReader<File>> {
    let mut f = File::open(path).map_err(Error::io(path))?;
    f.seek(SeekFrom::Start(from)).map_err(Error::io(path))?;
    Ok(BufReader::with_capacity(1 << 16, f))
}

/// Calls `f(line_pos, line)` for each complete line (newline included) of `reader`, whose first
/// byte sits at offset `start`. Stops at EOF or at a last line still being written, and returns
/// the offset just past the last complete line: where the next pass should resume.
pub(crate) fn for_each_line<R: BufRead>(mut reader: R, start: u64, mut f: impl FnMut(u64, &[u8])) -> io::Result<u64> {
    let mut pos = start;
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let n = reader.read_until(b'\n', &mut buf)?;
        if n == 0 || buf.last() != Some(&b'\n') {
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
