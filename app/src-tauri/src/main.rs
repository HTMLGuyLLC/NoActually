// Hide the console window on Windows release builds.
//
// The report-card helper (`hook` and `digest`) is this same executable. With the attribute below
// it still has no console of its own on Windows, but what it prints reaches a parent that gave it
// pipe handles, which is how the plugin and the Codex hooks run it. Started by hand from a
// console, it would show nothing.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    // The helper commands run before anything else is set up: no window, no single-instance
    // check (hooks run while the app is open), no log file. They print nothing on any error.
    // Raw arguments: `std::env::args()` panics on one that isn't valid text, which must never stop
    // the window from opening. Only a helper command is turned into text.
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if let Some(helper) = cli::helper_args(&args) {
        return cli::run(helper);
    }
    noactually_app::run();
    ExitCode::SUCCESS
}
