//! What report cards can't do on Windows yet. Claude Code there runs commands, the plugin's hook
//! among them, with the `bash` that Git for Windows installs, and the plugin's shim is a shell
//! script, so without Git there is nothing to run it. Codex's hooks and skill are not set up for
//! Windows at all. Turning on is refused with a sentence, and the row says it where the button would be.

use super::{Env, Tool};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub(super) const NEEDS_GIT: &str = "Report cards need Git for Windows, which Claude Code uses to run commands. Install it, or set CLAUDE_CODE_GIT_BASH_PATH, then try again.";
pub(super) const CODEX_NOT_YET: &str = "Report cards for Codex aren't available on Windows yet.";

/// Claude Code's own setting for where `bash.exe` is.
const BASH_PATH_ENV: &str = "CLAUDE_CODE_GIT_BASH_PATH";

/// Git for Windows' `bash.exe`: the file `var` (the [`BASH_PATH_ENV`] setting) names, else the one
/// beside the `git.exe` found on `PATH` (`git`), else the places its installer puts it: for everyone,
/// for everyone on a 32-bit Git, and for one person (under `local_app_data`). `exists` says whether a
/// file is there.
pub(super) fn git_bash(
    var: Option<OsString>,
    git: Option<PathBuf>,
    local_app_data: Option<PathBuf>,
    exists: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let set = var.filter(|v| !v.is_empty()).map(PathBuf::from);
    let beside_git = git.as_deref().and_then(bash_beside);
    let usual = [r"C:\Program Files\Git\bin\bash.exe", r"C:\Program Files (x86)\Git\bin\bash.exe"].map(PathBuf::from);
    let per_person = local_app_data.map(|dir| dir.join("Programs").join("Git").join("bin").join("bash.exe"));
    set.into_iter().chain(beside_git).chain(usual).chain(per_person).find(|file| exists(file))
}

/// Where `bash.exe` is when `git` is Git for Windows' `git.exe`: `...\Git\cmd\git.exe` (the one on
/// `PATH`) and `...\Git\bin\git.exe` sit beside `...\Git\bin\bash.exe`. Only a guess; the caller checks
/// that a file is there.
fn bash_beside(git: &Path) -> Option<PathBuf> {
    Some(git.parent()?.parent()?.join("bin").join("bash.exe"))
}

/// The first `git.exe` on `PATH`.
fn git_on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join("git.exe")).find(|file| file.is_file())
}

/// [`git_bash`] on this computer.
pub(super) fn git_bash_here() -> Option<PathBuf> {
    git_bash(
        std::env::var_os(BASH_PATH_ENV),
        git_on_path(),
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        &|file| file.is_file(),
    )
}

/// Why report cards can't be turned on in `tool` here, if they can't: Windows without Git for Claude
/// Code, and Windows for Codex. Nothing is unavailable anywhere else.
pub(super) fn unavailable(env: &Env, tool: Tool) -> Option<&'static str> {
    if !env.windows {
        return None;
    }
    match tool {
        Tool::ClaudeCode => env.git_bash.is_none().then_some(NEEDS_GIT),
        Tool::Codex => Some(CODEX_NOT_YET),
    }
}
