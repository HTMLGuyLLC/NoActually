//! `noactually delete-everything`: what Settings → Privacy → Delete everything does in the app, for when
//! the app won't open or is gone. It deletes the app's data and log folders and turns off Launch at
//! login. It won't run while the app is open (the app holds [`store::wipe::APP_LOCK`]), or while report
//! cards are on: turning them off removes the Claude Code plugin and the Codex hooks, which only the app
//! knows how to do.

use anyhow::{bail, Context, Result};
use std::io::{BufRead, IsTerminal};
use std::path::{Path, PathBuf};
use store::wipe;

/// The app's name, as Launch at login knows it (Tauri's product name).
const APP_NAME: &str = "No, Actually...";
/// The argument the app starts with at login (`HIDDEN_ARG` in the app).
const HIDDEN_ARG: &str = "--hidden";

/// Where everything lives, checked before anything changes.
pub(crate) struct Folders {
    data: PathBuf,
    logs: PathBuf,
    home: PathBuf,
}

impl Folders {
    /// The app's own folders. Refuses when the database is somewhere else ([`store::DB_ENV`]): that
    /// folder may hold other things.
    pub(crate) fn find() -> Result<Folders> {
        let db = store::default_db_path()?;
        let own = dirs::data_dir().map(|d| d.join(store::APP_ID));
        let home = dirs::home_dir().context("couldn't find your home folder")?;
        let logs = wipe::log_dir().context("couldn't find the log folder")?;
        let data = db.parent().map(Path::to_path_buf).unwrap_or_default();
        if own.as_deref() != Some(data.as_path()) || !wipe::own_folder(&data, &home) || !wipe::own_folder(&logs, &home)
        {
            bail!(
                "{} points outside the app's own folder, so nothing was deleted. Delete that database yourself.",
                store::DB_ENV
            );
        }
        Ok(Folders { data, logs, home })
    }
}

/// Whether report cards look on: the plugin is installed in Claude Code, or the skill is in place for
/// Codex. Read from the files those tools keep, the same places the app writes to.
fn report_cards_on(home: &Path) -> bool {
    let claude = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()).map(PathBuf::from);
    report_cards_on_in(home, &claude.unwrap_or_else(|| home.join(".claude")))
}

/// [`report_cards_on`] with Claude Code's folder given.
fn report_cards_on_in(home: &Path, claude: &Path) -> bool {
    let plugin = std::fs::read_to_string(claude.join("plugins").join("installed_plugins.json"))
        .is_ok_and(|s| s.contains("@noactually\""));
    let skill = home.join(".agents").join("skills").join("grade-me").exists();
    plugin || skill
}

/// Turns off Launch at login, the way the app's autostart plugin turned it on.
fn login_off() -> Result<()> {
    let exe = std::env::current_exe().context("couldn't find this program")?;
    let mut builder = auto_launch::AutoLaunchBuilder::new();
    builder.set_app_name(APP_NAME).set_app_path(&exe.display().to_string()).set_args(&[HIDDEN_ARG]);
    #[cfg(target_os = "macos")]
    builder.set_macos_launch_mode(auto_launch::MacOSLaunchMode::LaunchAgent);
    let login = builder.build().context("couldn't check Launch at login")?;
    if login.is_enabled().context("couldn't check Launch at login")? {
        login.disable().context("couldn't turn off Launch at login")?;
    }
    Ok(())
}

/// True on `--yes`, or a yes at the prompt.
fn confirmed(yes: bool, interactive: bool, input: &mut impl BufRead) -> Result<bool> {
    crate::confirm(
        "Delete everything No, Actually... keeps on this computer (scores, history, settings and logs) and \
         turn off Launch at login? This can't be undone.",
        "delete everything",
        yes,
        interactive,
        input,
    )
}

/// Runs it.
pub(crate) fn run(yes: bool) -> Result<()> {
    let f = Folders::find()?;
    if !f.data.exists() && !f.logs.exists() {
        println!("Nothing to delete: No, Actually... has no data on this computer.");
        return Ok(());
    }
    let Some(app_lock) = wipe::lock_app(&f.data).context("couldn't check whether the app is open")? else {
        bail!("No, Actually... is open. Quit it from its menu, then run this again. Nothing was deleted.");
    };
    if report_cards_on(&f.home) {
        bail!(
            "Report cards are on. Turn them off in the app first (Settings → Report cards) so the Claude Code \
             plugin and Codex hooks go too, or use Delete everything there. Nothing was deleted."
        );
    }
    let stdin = std::io::stdin();
    if !confirmed(yes, stdin.is_terminal(), &mut stdin.lock())? {
        println!("Cancelled. Nothing was deleted.");
        return Ok(());
    }
    login_off().context("nothing was deleted")?;
    drop(app_lock); // Windows won't delete a file that's open
    let left = wipe::remove_own_folder(&f.data, &f.home).unwrap_or_else(|| vec![f.data.clone()]);
    if !left.is_empty() {
        bail!("couldn't delete everything. Still there: {}. Delete them by hand.", list(&left));
    }
    let logs = wipe::remove_own_folder(&f.logs, &f.home).unwrap_or_else(|| vec![f.logs.clone()]);
    println!("Deleted everything No, Actually... kept on this computer, and Launch at login is off.");
    if !logs.is_empty() {
        println!("Some logs are left in {}: {}.", f.logs.display(), list(&logs));
    }
    println!("To finish, uninstall the app.");
    Ok(())
}

fn list(paths: &[PathBuf]) -> String {
    paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_yes_is_needed_and_no_terminal_means_no() {
        assert!(confirmed(true, false, &mut "".as_bytes()).unwrap());
        assert!(!confirmed(false, true, &mut "n\n".as_bytes()).unwrap());
        assert!(confirmed(false, true, &mut "y\n".as_bytes()).unwrap());
        assert!(confirmed(false, false, &mut "y\n".as_bytes()).is_err(), "never guesses without a terminal");
    }

    #[test]
    fn report_cards_count_as_on_in_either_tool() {
        let home = tempfile::tempdir().unwrap();
        let (h, claude) = (home.path(), home.path().join(".claude"));
        assert!(!report_cards_on_in(h, &claude));
        std::fs::create_dir_all(claude.join("plugins")).unwrap();
        std::fs::write(claude.join("plugins/installed_plugins.json"), r#"{"plugins":{"grade@noactually":[]}}"#)
            .unwrap();
        assert!(report_cards_on_in(h, &claude), "the Claude Code plugin");
        std::fs::remove_dir_all(&claude).unwrap();
        std::fs::create_dir_all(h.join(".agents/skills/grade-me")).unwrap();
        assert!(report_cards_on_in(h, &claude), "the Codex skill");
    }
}
