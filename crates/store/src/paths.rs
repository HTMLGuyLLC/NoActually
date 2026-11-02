//! Where the app keeps its files: the database, its folder's permissions, and the report card key.

use crate::{files, write_new, Error, Result};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The desktop app's bundle identifier; its data folder is named after it.
pub const APP_ID: &str = "app.noactually";
/// The database file inside the data folder.
pub const DB_FILE: &str = "noactually.db";
/// Environment variable that points the CLI and the app at another database.
pub const DB_ENV: &str = "NOACTUALLY_DB";
/// The variable's old name. Still read, after [`DB_ENV`]; deprecated.
pub const LEGACY_DB_ENV: &str = "WA_DB";
/// The secret that seals report cards, kept next to the database.
pub const CARD_KEY_FILE: &str = "report-cards.key";

/// The database the CLI and the app share: `$NOACTUALLY_DB`, else `$WA_DB` (deprecated), else
/// `<app data>/app.noactually/noactually.db`.
pub fn default_db_path() -> Result<PathBuf> {
    resolve_db_path(|k| std::env::var_os(k), dirs::data_dir())
}

/// What [`CARD_KEY_FILE`] holds right now.
enum KeyFile {
    Missing,
    /// A file with nothing in it seals nothing, so it counts as unusable.
    Empty,
    Secret(Vec<u8>),
}

fn read_key(path: &Path) -> std::io::Result<KeyFile> {
    match std::fs::read(path) {
        Ok(secret) if secret.is_empty() => Ok(KeyFile::Empty),
        Ok(secret) => Ok(KeyFile::Secret(secret)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(KeyFile::Missing),
        Err(e) => Err(e),
    }
}

/// The secret that seals report cards for the database at `db`: [`CARD_KEY_FILE`] beside it. With
/// `create`, a missing file is made (32 random bytes, owner-only) and an existing one is never
/// overwritten; without it, a missing file is `None`. An empty file seals nothing: `None` without
/// `create`, and replaced with `create`. Only the helper creates it: the app just checks seals, and
/// a card can't be sealed before the helper has run.
///
/// Two first runs can't end with different secrets. The new secret is written whole to a temporary
/// file in the same folder and then hard-linked into place, which fails if the key is already there;
/// whoever loses reads the winner's. A file is never seen half written.
pub fn card_secret(db: &Path, create: bool) -> Result<Option<Vec<u8>>> {
    let path = db.with_file_name(CARD_KEY_FILE);
    let io = |source| Error::Io { path: path.clone(), source };
    let mut replace = match read_key(&path).map_err(io)? {
        KeyFile::Secret(secret) => return Ok(Some(secret)),
        _ if !create => return Ok(None),
        state => matches!(state, KeyFile::Empty),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let mut secret = vec![0u8; 32];
    getrandom::fill(&mut secret).map_err(|e| io(std::io::Error::other(e.to_string())))?;
    let tmp = files::temp_sibling(&path).map_err(io)?;
    write_new(&tmp, &secret, 0o600).map_err(io)?;
    let placed = (|| {
        // Twice at most: the second try is for an empty file that turned up between the read and the link.
        let mut tries = 0;
        loop {
            if let KeyFile::Secret(winner) = read_key(&path)? {
                return Ok(winner); // someone made it in the meantime: never replace a real key
            }
            let put = if replace { std::fs::rename(&tmp, &path) } else { std::fs::hard_link(&tmp, &path) };
            match put {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && !replace => {}
                Err(e) => return Err(e),
            }
            match read_key(&path)? {
                KeyFile::Secret(winner) => return Ok(winner),
                _ => replace = true,
            }
            tries += 1;
            if tries == 2 {
                return Err(std::io::Error::other("the report card key stayed empty"));
            }
        }
    })();
    let _ = std::fs::remove_file(&tmp); // gone already when it was renamed into place
    placed.map(Some).map_err(io)
}

/// True when only the deprecated [`LEGACY_DB_ENV`] is set, so callers can say so.
pub fn legacy_db_env_in_use() -> bool {
    let set = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
    !set(DB_ENV) && set(LEGACY_DB_ENV)
}

pub(crate) fn resolve_db_path(env: impl Fn(&str) -> Option<OsString>, data_dir: Option<PathBuf>) -> Result<PathBuf> {
    // An empty variable counts as unset, so `NOACTUALLY_DB= cmd` can't silently mean "here".
    let var = |k: &str| env(k).filter(|v| !v.is_empty());
    if let Some(p) = var(DB_ENV).or_else(|| var(LEGACY_DB_ENV)) {
        return Ok(p.into());
    }
    // Never a relative path: that would put the database wherever the command happened to run.
    let dir = data_dir.filter(|d| d.is_absolute()).ok_or(Error::NoDataDir)?;
    Ok(dir.join(APP_ID).join(DB_FILE))
}

/// Limits the database and its write-ahead log to their owner, and the app's own data folder too
/// (on Unix; elsewhere the user's profile already does it). A folder chosen with [`DB_ENV`] may
/// hold other things, so it isn't touched. Marker files stay world-readable: the hooks read them.
pub(crate) fn protect(db: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let mut name = db.as_os_str().to_owned();
        name.push(suffix);
        files::restrict(Path::new(&name), 0o600);
    }
    if let Some(dir) = db.parent().filter(|d| d.file_name().is_some_and(|n| n == APP_ID)) {
        files::restrict(dir, 0o700);
    }
}
