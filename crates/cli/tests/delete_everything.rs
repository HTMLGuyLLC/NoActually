//! `noactually delete-everything`, run for real against a made-up home folder. Not on Windows: there
//! Launch at login lives in the real user's registry, which a test mustn't touch.
#![cfg(not(windows))]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The app's folders under `home`, where `dirs` and Tauri put them.
fn folders(home: &Path) -> (PathBuf, PathBuf) {
    if cfg!(target_os = "macos") {
        let lib = home.join("Library");
        (lib.join("Application Support/app.noactually"), lib.join("Logs/app.noactually"))
    } else {
        let data = home.join(".local/share/app.noactually");
        (data.clone(), data.join("logs"))
    }
}

/// Where Launch at login keeps its entry under `home`.
fn login_entry(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/LaunchAgents/No, Actually....plist")
    } else {
        home.join(".config/autostart/No, Actually....desktop")
    }
}

/// A home with the app's data, logs and Launch at login in it.
fn set_up() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let home = tempfile::tempdir().unwrap();
    let (data, logs) = folders(home.path());
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("noactually.db"), "x").unwrap();
    std::fs::write(logs.join("No, Actually....log"), "x").unwrap();
    let login = login_entry(home.path());
    std::fs::create_dir_all(login.parent().unwrap()).unwrap();
    std::fs::write(&login, "x").unwrap();
    (home, data, logs)
}

/// Runs the command with nothing from the real environment but `PATH`.
fn run(home: &Path, extra: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_noactually"));
    cmd.env_clear().env("HOME", home).env("PATH", std::env::var_os("PATH").unwrap_or_default());
    for (k, v) in extra {
        cmd.env(k, v);
    }
    cmd.args(["delete-everything", "--yes"]).output().unwrap()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn deletes_the_data_and_logs_and_turns_off_launch_at_login() {
    let (home, data, logs) = set_up();
    let out = run(home.path(), &[]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("Deleted everything"), "{}", text(&out.stdout));
    assert!(!data.exists() && !logs.exists(), "both folders are gone");
    assert!(!login_entry(home.path()).exists(), "Launch at login is off");
    assert!(home.path().exists(), "the home folder itself stays");
}

#[test]
fn it_waits_for_the_app_to_quit() {
    let (home, data, _) = set_up();
    let held = store::wipe::lock_app(&data).unwrap().expect("the app's lock");
    let out = run(home.path(), &[]);
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("is open"), "{}", text(&out.stderr));
    assert!(data.join("noactually.db").exists() && login_entry(home.path()).exists(), "nothing changed");
    drop(held);
}

#[test]
fn it_leaves_report_cards_to_the_app() {
    let (home, data, _) = set_up();
    std::fs::create_dir_all(home.path().join(".agents/skills/grade-me")).unwrap();
    let out = run(home.path(), &[]);
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("Report cards are on"), "{}", text(&out.stderr));
    assert!(data.join("noactually.db").exists(), "nothing was deleted");
}

#[test]
fn a_database_somewhere_else_is_never_deleted() {
    let (home, data, _) = set_up();
    let elsewhere = home.path().join("mine/noactually.db");
    let out = run(home.path(), &[("NOACTUALLY_DB", elsewhere.to_str().unwrap())]);
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("outside the app's own folder"), "{}", text(&out.stderr));
    assert!(data.exists());
}

#[test]
fn with_nothing_there_it_says_so() {
    let home = tempfile::tempdir().unwrap();
    let out = run(home.path(), &[]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("Nothing to delete"));
    assert!(!folders(home.path()).0.exists(), "it made nothing either");
}
