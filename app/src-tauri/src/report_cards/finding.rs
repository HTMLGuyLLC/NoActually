//! Finding Claude Code (and Codex) on this computer: the places a command is usually kept, the version
//! managers that keep one per Node version, the Claude desktop app's own copy, and the path an earlier
//! Turn on or Look again found. A window opened from the Dock doesn't get the shell's `PATH`, so a
//! launch has to find the command without asking a shell.

use std::path::{Path, PathBuf};

/// The file beside the database that holds where `claude` was last found (one absolute path and a
/// newline, owner-only). A launch never asks the login shell, so without this a `claude` that only a
/// shell knows about would be lost again after the app is closed.
const SAVED: &str = "report-cards-claude";

/// Whether `path` is a file the person can run. A link is followed. On Windows any file counts.
pub(super) fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else { return false };
    #[cfg(unix)]
    {
        std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o111 != 0 && meta.is_file()
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}

/// A folder name as numbers for comparing versions: `v20.11.0` is `[20, 11, 0]`. Anything else in the
/// name (a leading `v`, `-rc1`) only separates numbers. A name with no number is `[]`, the oldest.
fn version_key(name: &str) -> Vec<u64> {
    name.split(|c: char| !c.is_ascii_digit()).filter(|n| !n.is_empty()).filter_map(|n| n.parse().ok()).collect()
}

/// The folders inside `root`, newest version first (ties by name, so the order never depends on the
/// disk). Nothing if `root` isn't a folder.
fn newest_first(root: &Path) -> Vec<PathBuf> {
    let Ok(read) = std::fs::read_dir(root) else { return Vec::new() };
    let mut found: Vec<(Vec<u64>, String, PathBuf)> = read
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (version_key(&name), name, e.path())
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    found.into_iter().map(|(.., path)| path).collect()
}

/// The folders version managers and package managers put commands in: Volta, asdf, Bun, a global npm
/// prefix, and the folder each Node version has under nvm and fnm (newest first). Some of them may not
/// exist; a search skips those.
pub(super) fn version_manager_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        [".volta/bin", ".asdf/shims", ".bun/bin", ".npm-global/bin"].iter().map(|d| home.join(d)).collect();
    dirs.extend(newest_first(&home.join(".nvm/versions/node")).into_iter().map(|d| d.join("bin")));
    dirs.extend(
        newest_first(&home.join(".local/share/fnm/node-versions")).into_iter().map(|d| d.join("installation/bin")),
    );
    dirs
}

/// Where the Claude desktop app keeps the Claude Code it downloads (macOS):
/// `Application Support/Claude/claude-code/<version>/<hash>/claude.app/Contents/MacOS`, newest
/// version first. Opening the desktop app once is what puts it there.
/// True for a folder [`desktop_app_dirs`] returns: the Claude desktop app's own Claude Code.
pub(super) fn is_desktop_app_dir(dir: &Path) -> bool {
    dir.to_string_lossy().replace('\\', "/").contains("/Application Support/Claude/claude-code/")
}

pub(super) fn desktop_app_dirs(home: &Path) -> Vec<PathBuf> {
    let root = home.join("Library/Application Support/Claude/claude-code");
    newest_first(&root)
        .into_iter()
        .flat_map(|version| {
            // A version holds a folder named by a hash; the names are not versions, so just keep them in order.
            let mut hashes: Vec<PathBuf> =
                std::fs::read_dir(version).into_iter().flatten().flatten().map(|e| e.path()).collect();
            hashes.sort();
            hashes.into_iter().map(|hash| hash.join("claude.app/Contents/MacOS"))
        })
        .collect()
}

/// The `claude` an earlier Turn on or Look again saved, if it is still there and can be run.
pub(super) fn saved(data: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(data.join(SAVED)).ok()?;
    let path = PathBuf::from(text.trim());
    (path.is_absolute() && is_executable(&path)).then_some(path)
}

/// Saves where `claude` is, owner-only and through a temporary file, so a launch never reads half a
/// path. A file that already holds this path is left alone. A path that isn't absolute (a folder
/// that is only on `PATH` as `.`) would mean something else from another folder, so it is not saved.
pub(super) fn save(data: &Path, claude: &Path) -> std::io::Result<()> {
    let path = claude
        .to_str()
        .filter(|_| claude.is_absolute())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "not an absolute UTF-8 path"))?;
    let file = data.join(SAVED);
    let text = format!("{path}\n");
    if std::fs::read_to_string(&file).is_ok_and(|old| old == text) {
        return Ok(());
    }
    std::fs::create_dir_all(data)?;
    store::write_atomic(&file, text.as_bytes(), Some(0o600))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn exe_at(dir: &Path, name: &str) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let file = dir.join(name);
        fs::write(&file, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        fs::set_permissions(&file, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        file
    }

    #[test]
    fn versions_compare_as_numbers_not_as_text() {
        assert_eq!(version_key("v20.11.0"), [20, 11, 0]);
        assert_eq!(version_key("2.1.138"), [2, 1, 138]);
        assert_eq!(version_key("22.0.0-rc1"), [22, 0, 0, 1]);
        assert_eq!(version_key("system"), Vec::<u64>::new());
        let root = tempfile::tempdir().unwrap();
        for v in ["v9.0.0", "v20.11.0", "v20.9.1", "v18.20.4", "system"] {
            fs::create_dir_all(root.path().join(v)).unwrap();
        }
        fs::write(root.path().join("v99.0.0"), "a file, not a version folder").unwrap();
        let names: Vec<String> =
            newest_first(root.path()).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["v20.11.0", "v20.9.1", "v18.20.4", "v9.0.0", "system"]);
        assert_eq!(newest_first(&root.path().join("nope")), Vec::<PathBuf>::new());
    }

    #[test]
    fn only_a_file_that_can_be_run_counts() {
        let dir = tempfile::tempdir().unwrap();
        let file = exe_at(dir.path(), "claude");
        assert!(is_executable(&file));
        assert!(!is_executable(dir.path()), "a folder");
        assert!(!is_executable(&dir.path().join("gone")));
        #[cfg(unix)]
        {
            let plain = dir.path().join("plain");
            fs::write(&plain, "x").unwrap();
            assert!(!is_executable(&plain), "no execute bit");
        }
    }

    #[test]
    fn version_managers_are_listed_with_the_newest_node_first() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        for v in ["v18.20.4", "v22.1.0", "v20.11.0"] {
            fs::create_dir_all(h.join(".nvm/versions/node").join(v).join("bin")).unwrap();
        }
        for v in ["v20.0.0", "v21.5.0"] {
            fs::create_dir_all(h.join(".local/share/fnm/node-versions").join(v).join("installation/bin")).unwrap();
        }
        let dirs: Vec<String> = version_manager_dirs(h)
            .iter()
            .map(|d| d.strip_prefix(h).unwrap().to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(
            dirs,
            [
                ".volta/bin",
                ".asdf/shims",
                ".bun/bin",
                ".npm-global/bin",
                ".nvm/versions/node/v22.1.0/bin",
                ".nvm/versions/node/v20.11.0/bin",
                ".nvm/versions/node/v18.20.4/bin",
                ".local/share/fnm/node-versions/v21.5.0/installation/bin",
                ".local/share/fnm/node-versions/v20.0.0/installation/bin",
            ]
        );
        assert_eq!(version_manager_dirs(&h.join("empty")).len(), 4, "no nvm or fnm folders: just the fixed ones");
    }

    #[test]
    fn the_desktop_apps_copy_is_found_newest_version_first() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("Library/Application Support/Claude/claude-code");
        for (version, hash) in [("2.1.9", "aaa"), ("2.1.10", "bbb"), ("2.0.99", "ccc")] {
            fs::create_dir_all(root.join(version).join(hash).join("claude.app/Contents/MacOS")).unwrap();
        }
        let dirs = desktop_app_dirs(home.path());
        let hashes: Vec<String> = dirs
            .iter()
            .map(|d| {
                d.strip_prefix(&root).unwrap().components().nth(1).unwrap().as_os_str().to_string_lossy().into_owned()
            })
            .collect();
        assert_eq!(hashes, ["bbb", "aaa", "ccc"], "2.1.10 before 2.1.9 before 2.0.99");
        assert!(dirs.iter().all(|d| d.ends_with("claude.app/Contents/MacOS")));
        assert_eq!(desktop_app_dirs(&home.path().join("elsewhere")), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_saved_path_is_used_only_while_it_can_still_be_run() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let claude = exe_at(&dir.path().join("somewhere"), "claude");
        assert_eq!(saved(&data), None, "nothing saved yet");
        save(&data, &claude).unwrap();
        assert_eq!(saved(&data), Some(claude.clone()));
        assert_eq!(fs::read_to_string(data.join(SAVED)).unwrap(), format!("{}\n", claude.display()));
        fs::remove_file(&claude).unwrap();
        assert_eq!(saved(&data), None, "gone");
        for odd in ["", "claude", "./claude", "  \n"] {
            fs::write(data.join(SAVED), odd).unwrap();
            assert_eq!(saved(&data), None, "{odd:?} isn't a path to run");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_saved_path_is_owner_only_and_written_once() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let claude = exe_at(dir.path(), "claude");
        save(&data, &claude).unwrap();
        let file = data.join(SAVED);
        assert_eq!(fs::metadata(&file).unwrap().mode() & 0o777, 0o600);
        let inode = fs::metadata(&file).unwrap().ino();
        save(&data, &claude).unwrap();
        assert_eq!(fs::metadata(&file).unwrap().ino(), inode, "the same path is not written again");
        let names: Vec<_> = fs::read_dir(&data).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names, [SAVED], "no temporary file is left behind");
    }

    #[test]
    fn a_path_that_depends_on_the_folder_it_is_run_from_is_not_saved() {
        let dir = tempfile::tempdir().unwrap();
        assert!(save(dir.path(), Path::new("bin/claude")).is_err());
        assert!(save(dir.path(), Path::new("claude")).is_err());
        assert!(!dir.path().join(SAVED).exists());
    }
}
