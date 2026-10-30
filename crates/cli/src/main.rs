//! `noactually`: the command-line scorecard. All the work is in the `cli` library, which the
//! desktop app also runs for the report-card helper commands.

fn main() -> std::process::ExitCode {
    cli::run_os(std::env::args_os().skip(1))
}
