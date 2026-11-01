//! Settings → Privacy → Delete everything. The order matters: first what the app added to other tools,
//! while a failure can still be reported and nothing is gone; then the database; then the folders;
//! then it quits.
//!
//! Nothing may write to the data folder afterwards, so each writer is dealt with before the delete:
//! - the tracker and updater threads are stopped, and an update can't start (a restart would make a
//!   new database);
//! - the connections are really closed and swapped for in-memory ones, so what still writes through
//!   `App` (the window placement on exit, a click in a window) reaches no file;
//! - report cards refuse any more work (see `report_cards::shut`), and `report_cards::exclusive` is
//!   held so a launch check can't be half way through;
//! - logging is off, since on Linux the log is inside the data folder.
//!
//! The single-instance plugin keeps nothing here: a socket in `/tmp` on macOS, D-Bus on Linux, a mutex
//! on Windows.

use crate::logging::{shorten_home_in, LOG_LEVEL};
use crate::updater::{Updates, INSTALLING};
use crate::{lock, report_cards, tracker, App};
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use store::Store;
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

type Res<T> = Result<T, String>;

/// How long to wait for a sync pass to finish. A pass can't be cut short, and a big one takes minutes.
const STOP_WAIT: Duration = Duration::from_secs(120);
/// The folder inside the app's own folder that holds the log on Windows and Linux. (On macOS the log
/// folder is named after the app, in `~/Library/Logs`.)
const LOG_FOLDER: &str = "logs";

/// Starts every message from a step that runs before the delete.
const NOTHING_DELETED: &str = "Nothing was deleted.";
const NOT_OURS: &str = "Nothing was changed. This app is using a folder that isn't its own, so it won't delete it.";

/// The two folders that get deleted, checked, and the home folder they were checked against.
#[derive(Debug)]
struct Folders {
    data: PathBuf,
    logs: PathBuf,
    home: PathBuf,
}

impl Folders {
    /// The real folders, from the same helpers that made them.
    fn find(handle: &AppHandle) -> Res<Folders> {
        let db = store::default_db_path().map_err(|_| NOT_OURS)?;
        let path = handle.path();
        let (own_data, logs) = (path.app_data_dir().map_err(|_| NOT_OURS)?, path.app_log_dir().map_err(|_| NOT_OURS)?);
        let home = std::env::home_dir().ok_or(NOT_OURS)?;
        Folders::check(&db, &own_data, logs, home)
    }

    /// Refuses unless the database is in the app's own folder (`NOACTUALLY_DB` can put it anywhere) and
    /// both folders pass [`own_folder`]. Run before anything changes.
    fn check(db: &Path, own_data: &Path, logs: PathBuf, home: PathBuf) -> Res<Folders> {
        let data = db.parent().map(Path::to_path_buf).ok_or(NOT_OURS)?;
        let named_for_the_app = data.file_name().is_some_and(|n| n == store::APP_ID);
        if data != own_data || !named_for_the_app || !own_folder(&data, &home) || !own_folder(&logs, &home) {
            return Err(NOT_OURS.into());
        }
        Ok(Folders { data, logs, home })
    }
}

/// Whether `folder` can be deleted whole: an absolute path to a real folder named for this app, or its
/// `logs` folder, that isn't the home folder or a folder around it. Everything else is refused, an
/// empty path and a root included.
fn own_folder(folder: &Path, home: &Path) -> bool {
    let named = |p: &Path, name: &str| p.file_name().is_some_and(|n| n == name);
    let the_app = named(folder, store::APP_ID);
    let its_logs = named(folder, LOG_FOLDER) && folder.parent().is_some_and(|p| named(p, store::APP_ID));
    let a_link = fs::symlink_metadata(folder).is_ok_and(|m| m.file_type().is_symlink());
    folder.is_absolute()
        && !folder.components().any(|c| c == Component::ParentDir)
        && !home.starts_with(folder)
        && (the_app || its_logs)
        && !a_link
}

/// Deletes `folder` as far as it can, after checking it once more. Returns what's still there: one
/// open file mustn't keep the rest (Windows won't delete a file that's in use).
fn remove_own_folder(folder: &Path, home: &Path) -> Res<Vec<PathBuf>> {
    if !own_folder(folder, home) {
        return Err(NOT_OURS.into());
    }
    let failed = |r: std::io::Result<()>| r.is_err_and(|e| e.kind() != ErrorKind::NotFound);
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Ok(vec![folder.to_path_buf()]),
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
    Ok(left)
}

/// Closes both connections, leaving in-memory ones behind so the rest of the app has something to
/// use. Waits for whoever holds a connection, and the next write reaches no file.
fn close_stores(app: &App) -> Res<()> {
    let failed = |e: &dyn std::fmt::Display| {
        log::warn!("delete everything: couldn't close the database: {e}");
        format!("{NOTHING_DELETED} Couldn't close the database. Quit the app, open it again, and try again.")
    };
    for connection in [&app.writer, &app.reader] {
        let stand_in = Store::open_in_memory().map_err(|e| failed(&e))?;
        let file = std::mem::replace(&mut *lock(connection), stand_in);
        file.close().map_err(|e| failed(&e))?;
    }
    Ok(())
}

/// What Delete everything does, one method a step, so the order can be tested without a real app.
trait Steps {
    fn report_cards_off(&mut self) -> Res<()>;
    fn login_off(&mut self) -> Res<()>;
    fn stop_syncing(&mut self) -> Res<()>;
    fn close_database(&mut self) -> Res<()>;
    fn logging(&mut self, on: bool);
    /// See [`remove_own_folder`].
    fn remove(&mut self, folder: &Path) -> Res<Vec<PathBuf>>;
}

/// `names` as a short list: file names, not whole paths.
fn names(left: &[PathBuf]) -> String {
    let name =
        |p: &PathBuf| p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned());
    left.iter().map(name).collect::<Vec<_>>().join(", ")
}

/// Every step, in order, stopping at the first failure. Once the data is gone there's nothing to go
/// back to, so the log folder can't stop it: returns what's left of that for the person to be told.
fn delete_all(steps: &mut impl Steps, folders: &Folders) -> Res<Vec<PathBuf>> {
    let before_the_delete = |why: String| format!("{NOTHING_DELETED} {why}");
    steps.report_cards_off().map_err(before_the_delete)?;
    steps.login_off().map_err(before_the_delete)?;
    steps.stop_syncing().map_err(before_the_delete)?;
    steps.close_database()?;
    steps.logging(false);
    match steps.remove(&folders.data) {
        Ok(left) if left.is_empty() => {}
        Ok(left) => {
            steps.logging(true);
            return Err(format!(
                "Couldn't delete everything. Still in {}: {}. Report cards and Launch at login are off. Quit the app, then delete that folder by hand.",
                folders.data.display(),
                names(&left)
            ));
        }
        Err(why) => {
            steps.logging(true);
            return Err(why);
        }
    }
    Ok(steps.remove(&folders.logs).unwrap_or_else(|_| vec![folders.logs.clone()]))
}

/// The steps themselves.
struct Real<'a> {
    handle: &'a AppHandle,
    app: &'a App,
    home: &'a Path,
}

impl Steps for Real<'_> {
    fn report_cards_off(&mut self) -> Res<()> {
        report_cards::turn_off_on_this_computer()
    }

    fn login_off(&mut self) -> Res<()> {
        self.handle.autolaunch().disable().map_err(|e| {
            log::warn!("delete everything: couldn't turn off Launch at login: {e}");
            "Report cards are off, but Launch at login couldn't be turned off. Try again.".to_string()
        })
    }

    fn stop_syncing(&mut self) -> Res<()> {
        self.handle.state::<Updates>().stop(self.handle)?;
        if tracker::stop(&self.app.tracker, STOP_WAIT) {
            Ok(())
        } else {
            Err("Report cards and Launch at login are off, but a check of your chats is still running. Try again in a minute.".into())
        }
    }

    fn close_database(&mut self) -> Res<()> {
        report_cards::shut();
        close_stores(self.app)
    }

    fn logging(&mut self, on: bool) {
        log::set_max_level(if on { LOG_LEVEL } else { log::LevelFilter::Off });
    }

    fn remove(&mut self, folder: &Path) -> Res<Vec<PathBuf>> {
        remove_own_folder(folder, self.home)
    }
}

/// What the person is told when the data is gone but part of the log isn't.
fn left_text(left: &[PathBuf], logs: &Path) -> String {
    format!(
        "Your data is deleted, but these log files were in use: {}.\n\nAfter the app quits, delete {}.",
        names(left),
        logs.display()
    )
}

/// Runs Delete everything and quits, or returns why it stopped. A failure leaves the app running; past
/// the database that's with nothing to show, and the message says what to do.
pub fn run(handle: &AppHandle, app: &App) -> Res<()> {
    let folders = Folders::find(handle)?;
    if handle.state::<Updates>().installing() {
        return Err(format!("{NOTHING_DELETED} {INSTALLING}"));
    }
    // Held until the app is gone: no report cards change, and no launch check, from here on.
    let _one = report_cards::exclusive();
    log::info!("delete everything: started");
    let mut steps = Real { handle, app, home: &folders.home };
    let left = delete_all(&mut steps, &folders).inspect_err(|why| {
        log::warn!("delete everything stopped: {}", shorten_home_in(why, Some(&folders.home)));
    })?;
    if left.is_empty() {
        handle.exit(0);
        return Ok(());
    }
    let exit = handle.clone();
    // Not a blocking dialog, like the one for a failed start.
    handle
        .dialog()
        .message(left_text(&left, &folders.logs))
        .title("No, Actually...")
        .kind(MessageDialogKind::Warning)
        .show(move |_| exit.exit(0));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SyncStatus;
    use std::sync::atomic::AtomicBool;
    use std::sync::{mpsc, Mutex};
    use tempfile::TempDir;

    /// Records the steps, and fails or leaves things behind on request.
    #[derive(Default)]
    struct Record {
        did: Vec<String>,
        fail: &'static str,
        data_left: bool,
        logs_left: bool,
    }

    impl Record {
        fn step(&mut self, name: &str) -> Res<()> {
            self.did.push(name.to_string());
            if self.fail == name {
                Err(format!("{name} failed."))
            } else {
                Ok(())
            }
        }
    }

    impl Steps for Record {
        fn report_cards_off(&mut self) -> Res<()> {
            self.step("report cards")
        }
        fn login_off(&mut self) -> Res<()> {
            self.step("login")
        }
        fn stop_syncing(&mut self) -> Res<()> {
            self.step("sync")
        }
        fn close_database(&mut self) -> Res<()> {
            self.step("close")
        }
        fn logging(&mut self, on: bool) {
            self.did.push(format!("logging {on}"));
        }
        fn remove(&mut self, folder: &Path) -> Res<Vec<PathBuf>> {
            let which = if folder.ends_with("data") { "data" } else { "logs" };
            self.step(which)?;
            let left = if which == "data" { self.data_left } else { self.logs_left };
            Ok(if left { vec![folder.join("noactually.log")] } else { Vec::new() })
        }
    }

    fn folders() -> Folders {
        Folders { data: "/x/data".into(), logs: "/x/logs".into(), home: "/x".into() }
    }

    #[test]
    fn the_steps_run_in_order() {
        let mut steps = Record::default();
        assert_eq!(delete_all(&mut steps, &folders()), Ok(Vec::new()));
        assert_eq!(steps.did, ["report cards", "login", "sync", "close", "logging false", "data", "logs"]);
    }

    #[test]
    fn a_failed_step_stops_before_any_data_goes() {
        for (fail, ran) in [("report cards", 1), ("login", 2), ("sync", 3), ("close", 4)] {
            let mut steps = Record { fail, ..Record::default() };
            let why = delete_all(&mut steps, &folders()).unwrap_err();
            assert!(why.contains(&format!("{fail} failed.")), "{why}");
            assert_eq!(steps.did.len(), ran, "{:?}", steps.did);
            assert!(
                !steps.did.iter().any(|s| s == "data" || s == "logs" || s.starts_with("logging")),
                "{:?}",
                steps.did
            );
        }
        // Each of the first three is told as "nothing was deleted"; the real close step says so itself.
        let why = delete_all(&mut Record { fail: "report cards", ..Record::default() }, &folders()).unwrap_err();
        assert!(why.starts_with(NOTHING_DELETED), "{why}");
    }

    #[test]
    fn data_that_will_not_go_stops_it_turns_the_log_back_on_and_says_what_is_left() {
        let mut steps = Record { data_left: true, ..Record::default() };
        let why = delete_all(&mut steps, &folders()).unwrap_err();
        assert!(why.contains("/x/data") && why.contains("noactually.log") && why.contains("by hand"), "{why}");
        assert_eq!(steps.did[steps.did.len() - 2..], ["data", "logging true"]);
        assert!(!steps.did.iter().any(|s| s == "logs"));

        let mut steps = Record { fail: "data", ..Record::default() };
        assert!(delete_all(&mut steps, &folders()).is_err());
        assert_eq!(steps.did.last().map(String::as_str), Some("logging true"));
    }

    #[test]
    fn a_log_file_in_use_does_not_stop_the_quit_but_is_reported() {
        let mut steps = Record { logs_left: true, ..Record::default() };
        let left = delete_all(&mut steps, &folders()).unwrap();
        assert_eq!(left, [PathBuf::from("/x/logs/noactually.log")]);
        let text = left_text(&left, Path::new("/x/logs"));
        assert!(text.contains("noactually.log") && text.contains("/x/logs"), "{text}");
        assert!(!text.to_lowercase().contains("everything"), "it must not claim all of it went: {text}");
    }

    #[test]
    fn only_the_apps_own_folders_can_be_deleted() {
        let id = store::APP_ID;
        let home = Path::new("/Users/me");
        let good = [format!("/Users/me/Library/Application Support/{id}"), format!("/Users/me/Library/Logs/{id}")];
        for folder in &good {
            assert!(own_folder(Path::new(folder), home), "{folder}");
        }
        assert!(own_folder(Path::new(&format!("/home/me/.local/share/{id}/logs")), Path::new("/home/me")));

        let bad = [
            String::new(),
            "/".into(),
            ".".into(),
            "..".into(),
            "/Users".into(),
            "/Users/me".into(),
            "/Users/me/Documents".into(),
            "/Users/me/logs".into(),
            id.to_string(),
            format!("relative/{id}"),
            format!("/Users/me/{id}/../{id}"),
            format!("/Users/me/{id}-other"),
            format!("/Users/me/{id}/other"),
        ];
        for folder in &bad {
            assert!(!own_folder(Path::new(folder), home), "{folder:?}");
        }
        // A home folder that happens to be named for the app, and one inside such a folder.
        assert!(!own_folder(Path::new(&format!("/home/{id}")), Path::new(&format!("/home/{id}"))));
        assert!(!own_folder(Path::new(&format!("/home/{id}")), Path::new(&format!("/home/{id}/me"))));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_the_folders_place_is_refused() {
        let lab = Lab::new();
        let elsewhere = lab.dir.path().join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        fs::write(elsewhere.join("keep"), "keep").unwrap();
        fs::remove_dir_all(&lab.data).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &lab.data).unwrap();
        assert_eq!(remove_own_folder(&lab.data, &lab.home), Err(NOT_OURS.to_string()));
        assert!(elsewhere.join("keep").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_inside_the_folder_is_removed_and_not_followed() {
        let lab = Lab::new();
        let elsewhere = lab.dir.path().join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        fs::write(elsewhere.join("keep"), "keep").unwrap();
        std::os::unix::fs::symlink(&elsewhere, lab.data.join("link")).unwrap();
        assert_eq!(remove_own_folder(&lab.data, &lab.home), Ok(Vec::new()));
        assert!(!lab.data.exists());
        assert!(elsewhere.join("keep").exists());
    }

    #[test]
    fn the_guard_runs_again_at_the_delete_and_touches_nothing() {
        let lab = Lab::new();
        let other = lab.dir.path().join("documents");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("keep"), "keep").unwrap();
        for folder in [other.as_path(), lab.home.as_path(), lab.dir.path(), Path::new(""), Path::new("/")] {
            assert_eq!(remove_own_folder(folder, &lab.home), Err(NOT_OURS.to_string()), "{}", folder.display());
        }
        assert!(other.join("keep").exists());
    }

    #[test]
    fn a_database_outside_the_apps_folder_is_refused_before_anything_changes() {
        let id = store::APP_ID;
        let own = PathBuf::from(format!("/Users/me/Library/Application Support/{id}"));
        let logs = || PathBuf::from(format!("/Users/me/Library/Logs/{id}"));
        let home = || PathBuf::from("/Users/me");
        let check = |db: &Path, logs: PathBuf| Folders::check(db, &own, logs, home()).map(|f| f.data);
        assert_eq!(check(&own.join(store::DB_FILE), logs()), Ok(own.clone()));
        for db in ["/Users/me/Documents/noactually.db", "/Users/me/noactually.db", "noactually.db", "/noactually.db"] {
            assert_eq!(check(Path::new(db), logs()), Err(NOT_OURS.to_string()), "{db}");
        }
        assert_eq!(
            check(Path::new("/tmp/app.noactually/noactually.db"), logs()),
            Err(NOT_OURS.to_string()),
            "same name, wrong place"
        );
        assert_eq!(
            check(&own.join(store::DB_FILE), home()),
            Err(NOT_OURS.to_string()),
            "the log folder is checked too"
        );
    }

    /// A temporary home, with an app data folder of the right name.
    struct Lab {
        dir: TempDir,
        home: PathBuf,
        data: PathBuf,
    }

    impl Lab {
        fn new() -> Lab {
            let dir = tempfile::tempdir().unwrap();
            let home = dir.path().join("home");
            let data = home.join("data").join(store::APP_ID);
            fs::create_dir_all(&data).unwrap();
            Lab { dir, home, data }
        }

        fn app(&self) -> App {
            let db = self.data.join(store::DB_FILE);
            App {
                writer: Mutex::new(Store::open(&db).unwrap()),
                reader: Mutex::new(Store::open(&db).unwrap()),
                sources: Vec::new(),
                tracker: mpsc::channel().0,
                status: Mutex::new(SyncStatus::default()),
                menu_bar: AtomicBool::new(true),
            }
        }
    }

    #[test]
    fn the_whole_folder_goes_and_its_neighbours_stay() {
        let lab = Lab::new();
        for file in [
            store::DB_FILE,
            "noactually.db-wal",
            "noactually.db-shm",
            store::BACKUP_FILE,
            store::CARD_KEY_FILE,
            "cli-path",
        ] {
            fs::write(lab.data.join(file), "x").unwrap();
        }
        fs::create_dir_all(lab.data.join(LOG_FOLDER)).unwrap();
        fs::write(lab.data.join(LOG_FOLDER).join("app.log"), "x").unwrap();
        let neighbour = lab.data.with_file_name("app.other");
        fs::create_dir_all(&neighbour).unwrap();
        assert_eq!(remove_own_folder(&lab.data, &lab.home), Ok(Vec::new()));
        assert!(!lab.data.exists());
        assert!(neighbour.exists());
        assert_eq!(remove_own_folder(&lab.data, &lab.home), Ok(Vec::new()), "already gone is done");
    }

    #[test]
    fn closing_really_closes_and_nothing_written_later_reaches_the_folder() {
        let lab = Lab::new();
        let app = lab.app();
        lock(&app.writer).set_setting("window.main", "1,2,3,4").unwrap();
        assert_eq!(lock(&app.reader).get_setting("window.main").unwrap().as_deref(), Some("1,2,3,4"));
        let wal = lab.data.join("noactually.db-wal");
        assert!(wal.exists(), "the test needs a write-ahead log to be open");

        close_stores(&app).unwrap();
        assert!(!wal.exists() && !lab.data.join("noactually.db-shm").exists(), "the last close removes them");

        assert_eq!(remove_own_folder(&lab.data, &lab.home), Ok(Vec::new()));
        // What the exit hook and a stray click do, after the delete.
        lock(&app.writer).set_setting("window.main", "5,6,7,8").unwrap();
        assert_eq!(lock(&app.reader).get_setting("window.main").unwrap(), None, "each stand-in is its own");
        assert!(!lab.data.exists(), "nothing came back");
        close_stores(&app).expect("closing again is safe, so the button can be pressed again");
    }
}
