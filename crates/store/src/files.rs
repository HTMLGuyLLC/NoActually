//! Whole-file writes that never leave half a file, for what the app and the helper keep beside the
//! database (the report card key, the path file, hook configs).

use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

/// Temporary names tried before giving up. A clash needs two 64-bit random tags to agree.
const NAME_TRIES: usize = 4;

/// Writes `bytes` to `path` through a temporary file in the same folder, so a reader never sees half
/// a file. The temporary file has a random name and is opened with `create_new`, so nothing someone
/// left at a guessable name is ever written through. On Unix the file gets `mode`, or keeps the one
/// `path` has now (else 0644) when `mode` is `None`. The bytes are flushed to disk before the rename.
pub fn write_atomic(path: &Path, bytes: &[u8], mode: Option<u32>) -> std::io::Result<()> {
    let want = mode.unwrap_or_else(|| mode_of(path));
    let mut tries = 0;
    loop {
        let tmp = temp_sibling(path)?;
        match write_new(&tmp, bytes, want) {
            Err(e) if e.kind() == ErrorKind::AlreadyExists && tries + 1 < NAME_TRIES => tries += 1,
            Err(e) => return Err(e),
            Ok(()) => {
                let placed = put_in_place(&tmp, path, want);
                if placed.is_err() {
                    let _ = std::fs::remove_file(&tmp);
                }
                return placed;
            }
        }
    }
}

/// Writes `bytes` to a file that doesn't exist yet (an error if it does), with `mode` on Unix, and
/// flushes it to disk. Nothing already at `path`, a link included, is written through.
pub fn write_new(path: &Path, bytes: &[u8], mode: u32) -> std::io::Result<()> {
    let mut open = std::fs::OpenOptions::new();
    open.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut open, mode);
    #[cfg(not(unix))]
    let _ = mode;
    let mut file = open.open(path)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    if written.is_err() {
        drop(file);
        let _ = std::fs::remove_file(path);
    }
    written
}

/// A random, hidden name beside `path` for a file that is about to replace or be linked to it.
pub(crate) fn temp_sibling(path: &Path) -> std::io::Result<PathBuf> {
    let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let mut tag = [0u8; 8];
    getrandom::fill(&mut tag).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(path.with_file_name(format!(".{name}.{}.tmp", tag.map(|b| format!("{b:02x}")).concat())))
}

#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).map_or(0o644, |m| m.mode() & 0o7777)
}

#[cfg(not(unix))]
fn mode_of(_path: &Path) -> u32 {
    0o644
}

/// Renames `tmp` to `path`, first giving it exactly `mode` on Unix (the mode it was created with was cut by the umask).
fn put_in_place(tmp: &Path, path: &Path, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    std::fs::set_permissions(tmp, std::os::unix::fs::PermissionsExt::from_mode(mode))?;
    #[cfg(not(unix))]
    let _ = mode;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<_> =
            fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into()).collect();
        v.sort();
        v
    }

    #[test]
    fn a_write_replaces_the_file_whole_and_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.json");
        fs::write(&file, "old").unwrap();
        write_atomic(&file, b"new", None).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "new");
        assert_eq!(names(dir.path()), ["a.json"]);
        let fresh = dir.path().join("b.json");
        write_atomic(&fresh, b"first", None).unwrap();
        assert_eq!(fs::read_to_string(&fresh).unwrap(), "first");
    }

    #[test]
    fn a_missing_folder_is_an_error_and_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(write_atomic(&dir.path().join("no").join("a.json"), b"x", None).is_err());
        assert_eq!(names(dir.path()), Vec::<String>::new());
    }

    #[test]
    fn a_new_file_is_never_written_over_something_there() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("k");
        write_new(&file, b"one", 0o600).unwrap();
        let again = write_new(&file, b"two", 0o600).unwrap_err();
        assert_eq!(again.kind(), ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&file).unwrap(), b"one");
    }

    #[cfg(unix)]
    #[test]
    fn a_file_planted_at_a_temporary_name_is_never_written_through() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let (file, victim) = (dir.path().join("cli-path"), dir.path().join("victim"));
        fs::write(&victim, "precious").unwrap();
        for planted in [".cli-path.tmp", "cli-path.tmp"] {
            symlink(&victim, dir.path().join(planted)).unwrap();
        }
        write_atomic(&file, b"/x/y\n", Some(0o600)).unwrap();
        assert_eq!(fs::read_to_string(&victim).unwrap(), "precious");
        assert_eq!(fs::read_to_string(&file).unwrap(), "/x/y\n");
        assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        // And a link sitting at the exact name write_new is given is refused, not followed.
        let trap = dir.path().join("trap");
        symlink(&victim, &trap).unwrap();
        assert!(write_new(&trap, b"x", 0o600).is_err());
        assert_eq!(fs::read_to_string(&victim).unwrap(), "precious");
    }

    #[cfg(unix)]
    #[test]
    fn the_mode_is_the_one_asked_for_or_the_one_the_file_has() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let file = dir.path().join("hooks.json");
        fs::write(&file, "{}").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        write_atomic(&file, b"[]", None).unwrap();
        assert_eq!(mode(&file), 0o600, "kept");
        let fresh = dir.path().join("fresh.json");
        write_atomic(&fresh, b"[]", None).unwrap();
        assert_eq!(mode(&fresh), 0o644, "the default for a new file");
        write_atomic(&file, b"[]", Some(0o640)).unwrap();
        assert_eq!(mode(&file), 0o640, "asked for");
        let key = dir.path().join("key");
        write_new(&key, b"k", 0o600).unwrap();
        assert_eq!(mode(&key), 0o600);
    }

    #[test]
    fn temporary_names_are_random_hidden_and_beside_the_file() {
        let path = Path::new("/data/cli-path");
        let (a, b) = (temp_sibling(path).unwrap(), temp_sibling(path).unwrap());
        assert_ne!(a, b);
        assert_eq!(a.parent(), path.parent());
        let name = a.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with(".cli-path.") && name.rsplit('.').next() == Some("tmp"), "{name}");
    }
}
