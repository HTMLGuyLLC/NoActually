//! The `plugins/grade/bin/na` shim as the plugin runs it: with `sh`, a made-up home folder and a
//! fake helper that records its arguments. It must pass the arguments on, cope with spaces in the
//! path, and never fail or print when the helper is missing (except `digest`, which prints `no-app`),
//! because a hook that fails blocks work.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn shim() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/grade/bin/na")
}

/// A home folder with the app's data folder (where the shim looks for `cli-path`) inside.
struct Home {
    dir: tempfile::TempDir,
}

impl Home {
    fn new(name: &str) -> Home {
        let dir = tempfile::Builder::new().prefix(name).tempdir().unwrap();
        Home { dir }
    }

    fn data(&self) -> PathBuf {
        self.dir.path().join("data/app.noactually")
    }

    /// A fake helper that writes each argument on its own line to `args.txt` beside it.
    fn helper(&self, folder: &str) -> PathBuf {
        let exe = self.dir.path().join(folder).join("noactually");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        let record = exe.parent().unwrap().join("args.txt");
        let script = format!("#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\"; done > '{}'\n", record.display());
        std::fs::write(&exe, script).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        exe
    }

    fn cli_path(&self, contents: &str) {
        std::fs::create_dir_all(self.data()).unwrap();
        std::fs::write(self.data().join("cli-path"), contents).unwrap();
    }

    /// Runs the shim with nothing on `PATH` but the system tools, so a `noactually` installed on
    /// this machine can't answer for the fake.
    fn run(&self, args: &[&str]) -> Output {
        Command::new("sh")
            .arg(shim())
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.dir.path())
            .env("XDG_DATA_HOME", self.dir.path().join("data"))
            .output()
            .unwrap()
    }

    fn recorded(&self, folder: &str) -> Option<Vec<String>> {
        let text = std::fs::read_to_string(self.dir.path().join(folder).join("args.txt")).ok()?;
        Some(text.lines().map(str::to_string).collect())
    }
}

fn quiet_success(o: &Output) {
    assert!(o.status.success(), "{o:?}");
    assert!(o.stdout.is_empty() && o.stderr.is_empty(), "{o:?}");
}

#[test]
fn the_arguments_reach_the_helper() {
    let home = Home::new("shim");
    let exe = home.helper("bin");
    home.cli_path(&format!("{}\n", exe.display()));
    quiet_success(&home.run(&["hook", "stop"]));
    assert_eq!(home.recorded("bin").unwrap(), ["hook", "stop"]);
}

#[test]
fn a_path_with_spaces_works() {
    let home = Home::new("shim");
    let exe = home.helper("with space/in the way");
    home.cli_path(&format!("{}\n", exe.display()));
    quiet_success(&home.run(&["digest", "--session", "a b"]));
    assert_eq!(home.recorded("with space/in the way").unwrap(), ["digest", "--session", "a b"]);
}

#[test]
fn only_the_first_line_of_cli_path_counts() {
    let home = Home::new("shim");
    let exe = home.helper("bin");
    home.cli_path(&format!("{}\n/nowhere/else\n", exe.display()));
    quiet_success(&home.run(&["hook", "stop"]));
    assert_eq!(home.recorded("bin").unwrap(), ["hook", "stop"]);
}

/// A `cli-path` with a Windows line ending names a file that doesn't exist (`…/noactually\r`). On
/// this side of the shim it is skipped, quietly, and never run.
#[test]
fn a_cli_path_with_a_carriage_return_is_skipped_quietly() {
    let home = Home::new("shim");
    let exe = home.helper("bin");
    home.cli_path(&format!("{}\r\n", exe.display()));
    quiet_success(&home.run(&["hook", "stop"]));
    assert_eq!(home.recorded("bin"), None);
}

#[test]
fn a_missing_helper_exits_quietly() {
    let home = Home::new("shim");
    // Nothing recorded at all.
    quiet_success(&home.run(&["hook", "stop"]));
    // A path to a program that is gone.
    home.cli_path("/nowhere/noactually\n");
    quiet_success(&home.run(&["hook", "stop"]));
    // A path to a file that can't run.
    let exe = home.helper("bin");
    std::fs::write(&exe, "").unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o644)).unwrap();
    home.cli_path(&format!("{}\n", exe.display()));
    quiet_success(&home.run(&["hook", "stop"]));
}

#[test]
fn without_a_helper_only_digest_says_so() {
    let home = Home::new("shim");
    // `digest` tells the report card skill the app isn't installed, in exactly one line.
    let o = home.run(&["digest", "--session", "abc"]);
    assert!(o.status.success(), "{o:?}");
    assert_eq!(o.stdout, b"no-app\n", "{o:?}");
    assert!(o.stderr.is_empty(), "{o:?}");
    // A stale path to a program that is gone is the same as none.
    home.cli_path("/nowhere/noactually\n");
    assert_eq!(home.run(&["digest"]).stdout, b"no-app\n");
    // Hooks never gain output.
    quiet_success(&home.run(&["hook", "session-start"]));
    quiet_success(&home.run(&["hook", "stop"]));
    quiet_success(&home.run(&[]));
}

#[test]
fn a_stopped_source_stays_stopped_without_a_helper() {
    let home = Home::new("shim");
    // The app was moved, so the helper is gone, but its data says what the person chose.
    home.cli_path("/nowhere/noactually\n");
    let file = |name: &str| home.data().join(name);
    std::fs::write(file("noactually.db"), "").unwrap();
    quiet_success(&home.run(&["digest"])); // never allowed
    std::fs::write(file("source-allowed-claude-code"), "").unwrap();
    assert_eq!(home.run(&["digest"]).stdout, b"no-app\n", "allowed, so grading by eye is fine");
    std::fs::write(file("source-stopped-claude-code"), "").unwrap();
    quiet_success(&home.run(&["digest"])); // paused
}

#[test]
fn with_a_helper_digest_is_the_helpers_own() {
    let home = Home::new("shim");
    let exe = home.helper("bin");
    home.cli_path(&format!("{}\n", exe.display()));
    quiet_success(&home.run(&["digest", "--session", "abc"]));
    assert_eq!(home.recorded("bin").unwrap(), ["digest", "--session", "abc"]);
}
