//! Deleting the app's own folders, for Delete everything in the app and `noactually delete-everything`.
//! Only folders named for the app are ever deleted, after checking them twice, and a lock file says
//! whether the app is open, so the command line never deletes a folder the app is still writing to.

use crate::APP_ID;
use std::fs::{self, File, TryLockError};
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

/// The folder inside the app's own folder that holds the log on Windows and Linux. (On macOS the log
/// folder is named after the app, in `~/Library/Logs`.)
pub const LOG_FOLDER: &str = "logs";

/// The file the open app holds a lock on, beside the database.
pub const APP_LOCK: &str = "app.lock";

/// The app's log folder, where Tauri puts it: `~/Library/Logs/<id>` on macOS, else
/// `<local app data>/<id>/logs`.
pub fn log_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        dirs::home_dir().map(|h| h.join("Library").join("Logs").join(APP_ID))
    } else {
        dirs::data_local_dir().map(|d| d.join(APP_ID).join(LOG_FOLDER))
    }
}

/// Locks [`APP_LOCK`] in `data`, the app's folder, creating it. `Ok(None)` means someone else holds
/// it: the app is open. The lock lasts until the file is dropped, and goes with the process.
pub fn lock_app(data: &Path) -> std::io::Result<Option<File>> {
    fs::create_dir_all(data)?;
    let file = File::options().create(true).truncate(false).write(true).open(data.join(APP_LOCK))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

/// Whether `folder` can be deleted whole: an absolute path to a real folder named for this app, or its
/// `logs` folder, that isn't the home folder or a folder around it. Everything else is refused, an
/// empty path and a root included.
pub fn own_folder(folder: &Path, home: &Path) -> bool {
    let named = |p: &Path, name: &str| p.file_name().is_some_and(|n| n == name);
    let the_app = named(folder, APP_ID);
    let its_logs = named(folder, LOG_FOLDER) && folder.parent().is_some_and(|p| named(p, APP_ID));
    let a_link = fs::symlink_metadata(folder).is_ok_and(|m| m.file_type().is_symlink());
    folder.is_absolute()
        && !folder.components().any(|c| c == Component::ParentDir)
        && !home.starts_with(folder)
        && (the_app || its_logs)
        && !a_link
}

/// Deletes `folder` as far as it can, after checking it once more. Returns what's still there (one
/// open file mustn't keep the rest: Windows won't delete a file that's in use), or `None` when the
/// folder isn't [`own_folder`] and nothing was touched.
pub fn remove_own_folder(folder: &Path, home: &Path) -> Option<Vec<PathBuf>> {
    if !own_folder(folder, home) {
        return None;
    }
    let failed = |r: std::io::Result<()>| r.is_err_and(|e| e.kind() != ErrorKind::NotFound);
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Some(Vec::new()),
        Err(_) => return Some(vec![folder.to_path_buf()]),
    };
    let mut left = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        // Links are removed, never followed.
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        if failed(if is_dir { fs::remove_dir_all(&path) } else { fs::remove_file(&path) }) {
            left.push(path);
        }
    }
    if left.is_empty() && failed(fs::remove_dir(folder)) {
        left.push(folder.to_path_buf());
    }
    Some(left)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_lock_on_the_app_folder_is_refused_until_the_first_goes() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join(APP_ID);
        let first = lock_app(&data).unwrap().expect("nobody holds it yet");
        assert!(lock_app(&data).unwrap().is_none(), "the app is open");
        drop(first);
        assert!(lock_app(&data).unwrap().is_some(), "closing the app frees it");
    }

    #[test]
    fn the_log_folder_is_one_the_app_may_delete() {
        let (logs, home) = (log_dir().unwrap(), dirs::home_dir().unwrap());
        assert!(own_folder(&logs, &home), "{}", logs.display());
    }
}
