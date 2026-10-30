//! The `cli-path` file beside the database: where the report card plugin's shim finds the helper. The
//! helper is this app's own executable, which runs `hook` and `digest` itself and exits before any
//! window opens. The shim runs it while the app is closed, so the path has to outlive this run.

use std::ffi::OsStr;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// The file beside the database that tells the report-card plugin where the helper is.
const FILE: &str = "cli-path";

/// macOS runs an app that was opened straight from a download out of a read-only copy under a folder
/// with this name, and the copy is gone when the app quits.
const TRANSLOCATION: &str = "/AppTranslocation/";

/// What a launch did about the file.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Published {
    Wrote,
    /// It already held this path.
    Unchanged,
    /// The app is running from a copy that will vanish, so the file was left as it is. Settings says
    /// to move the app to the Applications folder.
    Translocated,
}

/// Records where the report-card helper is. Written on every launch, but only touched when the path
/// changed (the app moved, or an update put it somewhere else).
pub(crate) fn publish(db: &Path) -> std::io::Result<Published> {
    let exe = std::fs::canonicalize(std::env::current_exe()?)?;
    // Only an AppImage sets this: it runs from a mount that is gone when the app quits.
    let image = if cfg!(target_os = "linux") { std::env::var_os("APPIMAGE") } else { None };
    publish_to(&db.with_file_name(FILE), &exe, image.as_deref())
}

/// [`publish`] with the executable and `$APPIMAGE` given, so tests don't depend on how they were started.
fn publish_to(file: &Path, exe: &Path, appimage: Option<&OsStr>) -> std::io::Result<Published> {
    match helper_path(exe, appimage) {
        Some(path) => write(file, &path),
        None => Ok(Published::Translocated),
    }
}

/// The path to record for the helper, or `None` when this run has none that will last. An `AppImage`
/// is recorded as the `.AppImage` file: its executable is a mount point that goes with the process.
fn helper_path(exe: &Path, appimage: Option<&OsStr>) -> Option<PathBuf> {
    if let Some(image) = appimage.map(Path::new).filter(|p| p.is_absolute()) {
        return Some(image.to_path_buf());
    }
    (!is_translocated(exe)).then(|| exe.to_path_buf())
}

/// Whether `exe` is the temporary copy macOS runs a freshly downloaded app from.
pub(crate) fn is_translocated(exe: &Path) -> bool {
    exe.to_string_lossy().contains(TRANSLOCATION)
}

/// Writes `exe` and a newline to `file`, owner-only on Unix. A file that already holds this path is
/// left alone. The write goes through a temporary file, so a hook reading it never sees half a path.
fn write(file: &Path, exe: &Path) -> std::io::Result<Published> {
    let path = exe.to_str().ok_or_else(|| std::io::Error::new(ErrorKind::InvalidData, "path is not UTF-8"))?;
    let text = format!("{}\n", without_verbatim_prefix(path));
    if std::fs::read_to_string(file).is_ok_and(|old| old == text) {
        return Ok(Published::Unchanged);
    }
    store::write_atomic(file, text.as_bytes(), Some(0o600))?;
    Ok(Published::Wrote)
}

/// Windows spells a canonical path `\\?\C:\App\x.exe`; a shell script wants `C:\App\x.exe`.
fn without_verbatim_prefix(path: &str) -> String {
    if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(path).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn the_path_is_written_once_and_only_rewritten_when_it_changes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);
        let exe = Path::new("/Applications/No, Actually....app/Contents/MacOS/noactually-app");
        assert_eq!(publish_to(&file, exe, None).unwrap(), Published::Wrote);
        assert_eq!(fs::read_to_string(&file).unwrap(), format!("{}\n", exe.display()));
        assert_eq!(publish_to(&file, exe, None).unwrap(), Published::Unchanged, "same path: left alone");
        let moved = Path::new("/opt/noactually/noactually-app");
        assert_eq!(publish_to(&file, moved, None).unwrap(), Published::Wrote);
        assert_eq!(fs::read_to_string(&file).unwrap(), "/opt/noactually/noactually-app\n");
        let names: Vec<_> = fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names, [FILE], "no temporary file is left behind");
    }

    #[cfg(unix)]
    #[test]
    fn only_the_owner_can_read_it() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);
        publish_to(&file, Path::new("/x/y"), None).unwrap();
        assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn a_missing_folder_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        assert!(publish_to(&dir.path().join("gone").join(FILE), Path::new("/x/y"), None).is_err());
    }

    #[test]
    fn windows_paths_lose_their_verbatim_prefix() {
        assert_eq!(
            without_verbatim_prefix(r"\\?\C:\Program Files\NoActually\app.exe"),
            r"C:\Program Files\NoActually\app.exe"
        );
        assert_eq!(without_verbatim_prefix(r"\\?\UNC\server\share\app.exe"), r"\\server\share\app.exe");
        assert_eq!(without_verbatim_prefix("/usr/bin/app"), "/usr/bin/app");
    }

    // AppImages are Linux only, and these paths aren't full paths on Windows.
    #[cfg(unix)]
    #[test]
    fn an_appimage_records_its_own_file_not_the_mount_it_runs_from() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);
        let mount = Path::new("/tmp/.mount_NoActuXYZ/usr/bin/noactually-app");
        let image = OsStr::new("/home/ann/Apps/No, Actually....AppImage");
        assert_eq!(publish_to(&file, mount, Some(image)).unwrap(), Published::Wrote);
        assert_eq!(fs::read_to_string(&file).unwrap(), "/home/ann/Apps/No, Actually....AppImage\n");
        assert_eq!(publish_to(&file, mount, Some(image)).unwrap(), Published::Unchanged);
        // A value that isn't a full path (or is empty) would break once the hook runs from another folder.
        for odd in ["", "NoActually.AppImage", "./NoActually.AppImage"] {
            assert_eq!(helper_path(mount, Some(OsStr::new(odd))), Some(mount.to_path_buf()), "{odd:?}");
        }
        assert_eq!(helper_path(mount, None), Some(mount.to_path_buf()));
    }

    #[test]
    fn a_translocated_app_writes_nothing_and_leaves_an_earlier_path_alone() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);
        let gone_soon = Path::new(
            "/private/var/folders/ab/cdef/T/AppTranslocation/3F2A-91/d/No, Actually....app/Contents/MacOS/noactually-app",
        );
        assert!(is_translocated(gone_soon));
        assert_eq!(publish_to(&file, gone_soon, None).unwrap(), Published::Translocated);
        assert!(!file.exists(), "a path that vanishes is never recorded");
        // A good path from an earlier launch still works, so it stays.
        fs::write(&file, "/Applications/No, Actually....app/Contents/MacOS/noactually-app\n").unwrap();
        assert_eq!(publish_to(&file, gone_soon, None).unwrap(), Published::Translocated);
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "/Applications/No, Actually....app/Contents/MacOS/noactually-app\n"
        );
        // Only a folder of exactly that name counts; ordinary installs and look-alikes are written.
        for fine in [
            "/Applications/No, Actually....app/Contents/MacOS/noactually-app",
            "/opt/NoAppTranslocation/x",
            "/opt/AppTranslocationX/x",
        ] {
            assert!(!is_translocated(Path::new(fine)), "{fine}");
        }
    }
}
