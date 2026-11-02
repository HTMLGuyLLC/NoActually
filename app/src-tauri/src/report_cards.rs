//! Settings → Report cards. Turns the report card plugin on and off, in Claude Code (through its own
//! `claude plugin` commands) and in Codex (a skill and two hooks we write ourselves), and says where
//! it stands. This is blocking file and process work, so every command runs off the main thread.
//!
//! Nothing here hands the window raw output from `claude`: the person gets a plain sentence and the
//! details go to the log (cut short, with any password in a link taken out).

mod claude;
pub(crate) mod cli_path;
mod codex;
mod finding;
mod platform;
mod plugin_files;
#[cfg(test)]
mod tests;

use crate::{lock, App};
use chrono::{DateTime, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
use store::TipActivity;
use tauri::State;

type Res<T> = Result<T, String>;

/// Where the plugin comes from in a release build; the app's own tag is added after a `#`.
const REPO: &str = "HTMLGuyLLC/NoActually";
/// Debug builds only: use this marketplace source instead of the repo folder.
const SOURCE_ENV: &str = "NOACTUALLY_PLUGIN_SOURCE";

/// The tools report cards can be turned on in. Spelled like the window's `ReportCardTool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    ClaudeCode,
    Codex,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Tool::ClaudeCode => "Claude Code",
            Tool::Codex => "Codex",
        }
    }

    /// The source whose chats the app has to be allowed to read for a report card to count, and whose
    /// cards the row's "last report card" is about. Always a source that can carry one
    /// (`Source::gradable`); Cowork is another source and doesn't stand in for Claude Code.
    fn source(self) -> &'static str {
        match self {
            Tool::ClaudeCode => "claude-code",
            Tool::Codex => "codex",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolStatus {
    /// The tool itself is on this computer.
    pub installed_tool: bool,
    /// Everything we add to it is in place.
    pub on: bool,
    /// The day of the latest report card graded in this tool ("2026-10-29"), if there is one.
    pub last_card_day: Option<String>,
    /// What went wrong behind the person's back (the launch check), in plain words, for the row to show.
    pub problem: Option<String>,
    /// Report cards written in this tool that the app couldn't count.
    pub left_out: i64,
    /// Why report cards can't be turned on in it on this computer (see [`platform::unavailable`]), in
    /// plain words. The row says it where the Turn on button would be.
    pub unavailable: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    pub claude_code: ToolStatus,
    pub codex: ToolStatus,
    /// "Give me tips as I work" is on.
    pub tips: bool,
    /// Whether tips are reaching your chats.
    pub tip_activity: TipActivity,
    /// "Remind me after a task" is on (the hooks may show their free one-line notes).
    pub reminders: bool,
    /// The app is running from a temporary copy macOS made (it was opened straight from the download),
    /// so the plugin can't find the helper once the app is closed. Settings says to move it.
    pub translocated: bool,
}

/// Where everything lives. The real one reads the system; tests point it at temporary folders.
pub struct Env {
    pub home: PathBuf,
    /// The folder with the database in it. The tips and reminders files go there too.
    pub data: PathBuf,
    /// The `claude` command, if Claude Code is installed.
    pub claude: Option<PathBuf>,
    /// The `codex` command, if there is one. (Codex is also here if it has made its folder in `home`.)
    pub codex: Option<PathBuf>,
    /// This app's own executable.
    pub exe: Option<PathBuf>,
    /// This is Windows.
    pub windows: bool,
    /// Git for Windows' `bash.exe`, if it was found (see [`platform::git_bash`]). Only looked for on Windows.
    pub git_bash: Option<PathBuf>,
    /// The `git` command, if there is one. Only used to look that a release tag is there before the
    /// plugin is moved to it; without git that look is skipped.
    pub git: Option<PathBuf>,
}

impl Env {
    /// The real folders, and the tools found on this computer. A program is looked for once per run
    /// (see [`find_cached`]). `shell` lets that look include a login shell, which reads the person's
    /// shell setup files: only when they pressed Turn on, Turn off or Look again, never for a plain
    /// status check or a launch. `claude` is looked for where an earlier Turn on or Look again found it
    /// first (see [`find_claude`]), so a launch finds it even though it never asks a shell.
    fn system(shell: bool) -> Res<Env> {
        let env = Env::folders()?;
        let dirs = usual_dirs(&env.home);
        let via_shell: &dyn Fn(&str) -> Option<PathBuf> = if shell { &login_shell_finds } else { &|_| None };
        let claude = find_claude(&env.data, &FOUND, &dirs, via_shell, shell);
        // Checking for the folder first saves a look.
        let codex = if codex::codex_dir(&env).is_dir() { None } else { find_cached(&FOUND, "codex", &dirs, via_shell) };
        let git = find_cached(&FOUND, "git", &dirs, via_shell).filter(|git| !is_xcode_stub(git));
        Ok(Env { claude, codex, git, ..env })
    }

    /// Just the real folders, for what doesn't need to know which tools are installed.
    fn folders() -> Res<Env> {
        let home = std::env::home_dir().ok_or("Couldn't find your home folder.")?;
        let db = store::default_db_path().map_err(|e| plain("find the app's data folder", &e))?;
        let data = db.parent().map(Path::to_path_buf).ok_or("Couldn't find the app's data folder.")?;
        let windows = cfg!(windows);
        let git_bash = if windows { platform::git_bash_here() } else { None };
        let exe = std::env::current_exe().ok();
        Ok(Env { home, data, claude: None, codex: None, exe, windows, git_bash, git: None })
    }

    /// The file whose presence turns tips on (the session-start hook looks for it).
    fn tips_file(&self) -> PathBuf {
        self.data.join(cli::TIPS_FILE)
    }

    /// The file that says tips were on and were turned off, so a chat that comes back is told to stop.
    fn tips_off_file(&self) -> PathBuf {
        self.data.join(cli::TIPS_OFF_FILE)
    }

    /// The file whose presence turns the hooks' one-line reminders off.
    fn quiet_file(&self) -> PathBuf {
        self.data.join(cli::QUIET_FILE)
    }

    /// The file that says a move of the Claude Code plugin to a new release didn't finish (see
    /// `claude::Pending`). Its presence is what Settings shows as the failed update.
    fn repin_file(&self) -> PathBuf {
        self.data.join(REPIN_FILE)
    }

    /// The file that keeps what the plugin check found wrong with the downloaded plugin (see
    /// `claude::note_problem`), so Settings still says it after a restart.
    fn problem_file(&self) -> PathBuf {
        self.data.join(PROBLEM_FILE)
    }
}

/// A file beside the database. While it exists, the launch check still has a plugin move to finish.
const REPIN_FILE: &str = "report-cards-repin";
/// A file beside the database. It holds the sentence for what the plugin check found wrong.
const PROBLEM_FILE: &str = "report-cards-problem";

/// An error for the window, with the details kept in the log.
fn plain(what: &str, e: &dyn std::fmt::Display) -> String {
    log::warn!("report cards: couldn't {what}: {e}");
    format!("Couldn't {what}. Try again.")
}

// ---- finding and running programs -----------------------------------------------------------

/// Where Claude Code and Codex usually put their commands. A window opened from the Dock doesn't get
/// the shell's `PATH`, so these are checked first.
fn usual_dirs(home: &Path) -> Vec<PathBuf> {
    dirs_for(home, cfg!(windows), cfg!(target_os = "macos"))
}

/// [`usual_dirs`] for a system given, so tests don't depend on the one they run on. Besides the plain
/// places: where version managers and package managers keep commands, and, on macOS, the Claude
/// desktop app's own copy of Claude Code (last, so a copy the person installed themselves wins).
fn dirs_for(home: &Path, windows: bool, mac: bool) -> Vec<PathBuf> {
    if windows {
        return vec![home.join(".local").join("bin")];
    }
    let mut dirs =
        vec![home.join(".local/bin"), home.join(".claude/local"), "/opt/homebrew/bin".into(), "/usr/local/bin".into()];
    dirs.extend(finding::version_manager_dirs(home));
    if mac {
        dirs.extend(finding::desktop_app_dirs(home));
    }
    dirs
}

/// The program `name` in one of `dirs`, else on `PATH`, else wherever `shell` finds it, and only
/// then in the Claude desktop app's own copy: one the person installed themselves always wins.
fn find_exe(name: &str, dirs: &[PathBuf], shell: &dyn Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default();
    find_exe_in(name, dirs, &path, shell)
}

/// [`find_exe`] with `PATH` given, so tests don't depend on what's installed where they run.
fn find_exe_in(
    name: &str,
    dirs: &[PathBuf],
    path: &[PathBuf],
    shell: &dyn Fn(&str) -> Option<PathBuf>,
) -> Option<PathBuf> {
    let files: Vec<String> =
        if cfg!(windows) { vec![format!("{name}.exe"), format!("{name}.cmd")] } else { vec![name.into()] };
    let (desktop, own): (Vec<&PathBuf>, Vec<&PathBuf>) = dirs.iter().partition(|d| finding::is_desktop_app_dir(d));
    let look = |dirs: Vec<&PathBuf>| {
        dirs.into_iter().flat_map(|d| files.iter().map(move |f| d.join(f))).find(|p| finding::is_executable(p))
    };
    look(own.into_iter().chain(path).collect()).or_else(|| shell(name)).or_else(|| look(desktop))
}

/// Where each program was found so far this run.
static FOUND: Mutex<Vec<(String, PathBuf)>> = Mutex::new(Vec::new());

/// [`find_exe`], remembering what it finds in `cache`: the second ask is a lookup. A program that
/// has gone since is looked for again, and one that isn't found is not remembered.
fn find_cached(
    cache: &Mutex<Vec<(String, PathBuf)>>,
    name: &str,
    dirs: &[PathBuf],
    shell: &dyn Fn(&str) -> Option<PathBuf>,
) -> Option<PathBuf> {
    let known = cache.lock().unwrap_or_else(PoisonError::into_inner).iter().find(|(n, _)| n == name).cloned();
    if let Some((_, path)) = known.filter(|(_, p)| finding::is_executable(p)) {
        return Some(path);
    }
    let path = find_exe(name, dirs, shell)?;
    let mut found = cache.lock().unwrap_or_else(PoisonError::into_inner);
    found.retain(|(n, _)| n != name);
    found.push((name.to_string(), path.clone()));
    Some(path)
}

/// The `claude` command. With `remember` (the person pressed a button) it's looked for afresh, so a
/// newer install wins over an older one saved before, and what is found is saved for the launches
/// that follow. Otherwise the saved one (see [`finding::saved`]) comes first, if it's still there.
fn find_claude(
    data: &Path,
    cache: &Mutex<Vec<(String, PathBuf)>>,
    dirs: &[PathBuf],
    shell: &dyn Fn(&str) -> Option<PathBuf>,
    remember: bool,
) -> Option<PathBuf> {
    let path = if remember {
        cache.lock().unwrap_or_else(PoisonError::into_inner).retain(|(n, _)| n != "claude");
        find_cached(cache, "claude", dirs, shell).or_else(|| finding::saved(data))
    } else {
        finding::saved(data).or_else(|| find_cached(cache, "claude", dirs, shell))
    }?;
    if remember {
        if let Err(e) = finding::save(data, &path) {
            log::warn!("report cards: couldn't save where claude is: {e}");
        }
    }
    Some(path)
}

#[cfg(unix)]
fn login_shell_finds(name: &str) -> Option<PathBuf> {
    let shell = std::env::var_os("SHELL").filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/sh".into());
    let ran = run(Path::new(&shell), &["-lc", &format!("command -v {name}")], Duration::from_secs(5)).ok()?;
    // An alias or function prints its definition, not a path; only a real file counts.
    let path = PathBuf::from(ran.out.lines().last()?.trim());
    (path.is_absolute() && path.is_file()).then_some(path)
}

#[cfg(not(unix))]
fn login_shell_finds(_name: &str) -> Option<PathBuf> {
    None
}

/// How a finished program ended and what it printed.
pub(crate) struct Ran {
    pub ok: bool,
    pub timed_out: bool,
    pub out: String,
    pub err: String,
}

impl Ran {
    /// Everything it printed, lowercase, for matching on.
    pub(crate) fn said(&self) -> String {
        format!("{}\n{}", self.out, self.err).to_lowercase()
    }
}

/// Runs `exe` with no input and no window, and stops it after `limit`. `PATH` gets the folders a
/// Dock-launched app lacks, including the program's own (a `claude` script needs the `node` beside it).
pub(crate) fn run(exe: &Path, args: &[&str], limit: Duration) -> std::io::Result<Ran> {
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Never sit waiting for a password.
        .env("GIT_TERMINAL_PROMPT", "0");
    let mut dirs: Vec<PathBuf> =
        std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    dirs.extend(exe.parent().map(Path::to_path_buf));
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    if let Ok(path) = std::env::join_paths(dirs) {
        cmd.env("PATH", path);
    }
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000); // CREATE_NO_WINDOW

    let mut child = spawn(&mut cmd)?;
    // Read both pipes on their own threads so a chatty program can't fill one and stall.
    let (tx, rx) = mpsc::channel();
    if let Some(pipe) = child.stdout.take() {
        drain(pipe, false, tx.clone());
    }
    if let Some(pipe) = child.stderr.take() {
        drain(pipe, true, tx);
    }
    let deadline = Instant::now() + limit;
    let (ok, timed_out) = loop {
        if let Some(status) = child.try_wait()? {
            break (status.success(), false);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break (false, true);
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let (mut out, mut err) = (String::new(), String::new());
    // A program we killed may have left a child holding the pipe open, so don't wait long for the end.
    while let Ok((is_err, bytes)) = rx.recv_timeout(Duration::from_secs(1)) {
        let text = String::from_utf8_lossy(&bytes);
        if is_err {
            err = text.into_owned();
        } else {
            out = text.into_owned();
        }
    }
    Ok(Ran { ok, timed_out, out, err })
}

fn drain(mut pipe: impl Read + Send + 'static, is_err: bool, tx: mpsc::Sender<(bool, Vec<u8>)>) {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = (&mut pipe).take(1 << 20).read_to_end(&mut bytes);
        let _ = tx.send((is_err, bytes));
    });
}

/// A script someone just wrote can still be "busy" for a moment on Linux; try again shortly.
fn spawn(cmd: &mut Command) -> std::io::Result<std::process::Child> {
    for _ in 0..5 {
        match cmd.spawn() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(Duration::from_millis(25));
            }
            done => return done,
        }
    }
    cmd.spawn()
}

/// The first JSON value in `text`, skipping any warning printed before it.
pub(crate) fn json_in(text: &str) -> Option<serde_json::Value> {
    let from = text.find(['[', '{'])?;
    serde_json::Deserializer::from_str(&text[from..]).into_iter().next()?.ok()
}

// ---- what the plugin source is --------------------------------------------------------------

/// What `claude plugin marketplace add` is given. A release build pins the app's own tag, so the
/// plugin never runs code from an unreleased commit. A debug build uses this repo's folder, or
/// `NOACTUALLY_PLUGIN_SOURCE`; release builds ignore that variable.
fn source_for(debug: bool, version: &str, over: Option<String>) -> String {
    if !debug {
        return format!("{REPO}#v{version}");
    }
    over.filter(|s| !s.is_empty()).unwrap_or_else(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        root.canonicalize().unwrap_or(root).to_string_lossy().into_owned()
    })
}

fn plugin_source() -> String {
    source_for(cfg!(debug_assertions), env!("CARGO_PKG_VERSION"), std::env::var(SOURCE_ENV).ok())
}

// ---- what the window asks for ---------------------------------------------------------------

/// Where report cards stand. `last_cards` is the day each source last had a card written
/// (`Store::last_report_card_days`). `problem` is what the launch check couldn't do this run, if
/// anything. After that come a check that couldn't be made now, what an earlier check found wrong with the
/// downloaded plugin (`claude::saved_problem`), and a move it left unfinished (`claude::Pending`), until
/// a launch finishes it.
fn status(env: &Env, last_cards: &[(String, NaiveDate)], problem: Option<String>) -> Status {
    let last = |tool: Tool| last_cards.iter().find(|(s, _)| s == tool.source()).map(|(_, day)| day.to_string());
    let claude_installed = env.claude.is_some();
    let codex_installed = codex::installed(env);
    let (claude_on, unsure) = if claude_installed { claude::look(env) } else { (false, None) };
    let saved = || claude::saved_problem(env).map(str::to_string);
    Status {
        claude_code: ToolStatus {
            installed_tool: claude_installed,
            on: claude_on,
            last_card_day: last(Tool::ClaudeCode),
            left_out: 0,
            problem: problem
                .or_else(|| unsure.map(str::to_string))
                .or_else(saved)
                .or_else(|| env.repin_file().exists().then(|| claude::UPDATE_FAILED.to_string())),
            unavailable: platform::unavailable(env, Tool::ClaudeCode).map(str::to_string),
        },
        codex: ToolStatus {
            installed_tool: codex_installed,
            on: codex::is_on(env),
            last_card_day: last(Tool::Codex),
            left_out: 0,
            problem: None,
            unavailable: platform::unavailable(env, Tool::Codex).map(str::to_string),
        },
        tips: env.tips_file().exists(),
        tip_activity: TipActivity::default(),
        reminders: !env.quiet_file().exists(),
        translocated: env.exe.as_deref().is_some_and(cli_path::is_translocated),
    }
}

/// True for macOS's stand-in `git`, which pops up an Xcode install prompt when run without the tools.
fn is_xcode_stub(git: &Path) -> bool {
    let tools = ["/Library/Developer/CommandLineTools", "/Applications/Xcode.app"];
    cfg!(target_os = "macos") && git == Path::new("/usr/bin/git") && !tools.iter().any(|t| Path::new(t).exists())
}

/// Report cards are read from the tool's chats, so its source has to be allowed first.
fn require_source(tool: Tool, allowed: bool) -> Res<()> {
    if allowed {
        Ok(())
    } else {
        Err(format!("Allow {} in Sources first, so the app can read your report cards.", tool.name()))
    }
}

/// Only the scorecard window changes report cards; the small window under the menu bar icon never does.
fn main_window_only(window: &tauri::Window) -> Res<()> {
    only_main(window.label())?;
    still_open()
}

fn only_main(label: &str) -> Res<()> {
    if label == "main" {
        Ok(())
    } else {
        Err("That can only be done from the main window.".into())
    }
}

fn turn_on(env: &Env, tool: Tool, source: &str) -> Res<()> {
    // The row says this instead of showing Turn on; refusing here too keeps a stale window honest.
    if let Some(why) = platform::unavailable(env, tool) {
        return Err(why.into());
    }
    match tool {
        Tool::ClaudeCode => claude::turn_on(env, source),
        Tool::Codex => codex::turn_on(env),
    }
}

fn turn_off(env: &Env, tool: Tool) -> Res<()> {
    match tool {
        // Tips need the plugin, so they end with it, and turning report cards back on doesn't restart them.
        Tool::ClaudeCode => claude::turn_off(env).and_then(|()| set_tips(env, false)),
        Tool::Codex => codex::turn_off(env),
    }
}

/// Turns report cards off in every tool, for Delete everything, stopping at the first that fails and
/// naming it. Turning off is safe to repeat, so the tools don't have to be asked whether they're on,
/// which can itself fail. A tool that isn't on this computer has nothing to take out, unless Claude
/// Code's files still list the plugin but its command can't be found. That can't be removed from
/// here and mustn't block the delete for good, so it comes back as a note for the person.
fn turn_off_everywhere(env: &Env) -> Res<Option<String>> {
    let mut left = None;
    for tool in [Tool::ClaudeCode, Tool::Codex] {
        if matches!(tool, Tool::ClaudeCode) && env.claude.is_none() {
            left = claude::left_without_command(env);
        } else {
            turn_off(env, tool)
                .map_err(|why| format!("Report cards couldn't be turned off in {}. {why}", tool.name()))?;
        }
        forget_launch_problem(tool);
    }
    Ok(left)
}

/// [`turn_off_everywhere`] on the real folders and tools, found the way a click on Turn off finds them.
pub(crate) fn turn_off_on_this_computer() -> Res<Option<String>> {
    turn_off_everywhere(&Env::system(true)?)
}

/// Makes an empty file at `file` if there is nothing there. Whatever is there already (a file with
/// something in it, a link) is left exactly as it is.
fn touch_new(file: &Path) -> std::io::Result<()> {
    match store::write_new(file, b"", 0o644) {
        Err(e) if e.kind() != ErrorKind::AlreadyExists => Err(e),
        _ => Ok(()),
    }
}

/// Makes or removes a marker file: its presence is the setting. Making one never empties a file
/// that is already there.
fn set_marker(dir: &Path, file: &Path, on: bool, what: &str) -> Res<()> {
    if on {
        std::fs::create_dir_all(dir).and_then(|()| touch_new(file)).map_err(|e| plain(what, &e))
    } else {
        match std::fs::remove_file(file) {
            Err(e) if e.kind() != ErrorKind::NotFound => Err(plain(what, &e)),
            _ => Ok(()),
        }
    }
}

/// Turns tips on or off. Turning them off leaves a note for chats that already have the rules, but
/// only if tips were on: a box that was never ticked has nothing to take back.
fn set_tips(env: &Env, on: bool) -> Res<()> {
    let what = if on { "turn that on" } else { "turn that off" };
    let was_on = env.tips_file().exists();
    set_marker(&env.data, &env.tips_file(), on, what)?;
    if on || was_on {
        set_marker(&env.data, &env.tips_off_file(), !on, what)?;
    }
    Ok(())
}

/// Makes or removes the file that turns the hooks' reminders off (`on` is the reminders, so it removes the file).
fn set_reminders(env: &Env, on: bool) -> Res<()> {
    set_marker(&env.data, &env.quiet_file(), !on, if on { "turn that on" } else { "turn that off" })
}

/// One change at a time: two clicks, or a click during the launch check, never interleave.
pub(crate) fn exclusive() -> MutexGuard<'static, ()> {
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Set by Delete everything once the database is closed. Nothing here writes beside the database from
/// then on, so a click or the launch check can't put back a file about to be deleted.
static SHUT: AtomicBool = AtomicBool::new(false);

/// See [`SHUT`].
pub(crate) fn shut() {
    SHUT.store(true, Ordering::Relaxed);
}

/// Fails once [`SHUT`] is set.
fn still_open() -> Res<()> {
    refuse_if(SHUT.load(Ordering::Relaxed))
}

fn refuse_if(shut: bool) -> Res<()> {
    if shut {
        Err("That can't be done while your data is being deleted.".into())
    } else {
        Ok(())
    }
}

/// What the launch check couldn't do this run, until the next launch tries again.
static LAUNCH_PROBLEM: Mutex<Option<String>> = Mutex::new(None);

fn launch_problem() -> MutexGuard<'static, Option<String>> {
    LAUNCH_PROBLEM.lock().unwrap_or_else(PoisonError::into_inner)
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(|e| plain("finish that", &e))?
}

/// Which tools have report cards on, and the latest card from each. `look` also lets the search for
/// a tool include the person's login shell (Settings' "Look again" button).
#[tauri::command]
pub async fn report_cards_status(window: tauri::Window, app: State<'_, Arc<App>>, look: bool) -> Res<Status> {
    main_window_only(&window)?;
    let app = Arc::clone(&app);
    blocking(move || {
        let env = Env::system(look)?;
        let last = lock(&app.reader).last_report_card_days().unwrap_or_else(|e| {
            log::warn!("report cards: couldn't read the latest card: {e}");
            Vec::new()
        });
        let shown = status(&env, &last, launch_problem().clone());
        let reader = lock(&app.reader);
        let state = reader.source_state(Tool::ClaudeCode.source()).ok().flatten();
        let since = tips_since(&env, &shown).filter(|_| state.is_some_and(|s| s.allowed && !s.paused));
        let tip_activity = reader.tip_activity(Local::now().date_naive(), since).unwrap_or_else(|e| {
            log::warn!("report cards: couldn't read how tips are doing: {e}");
            TipActivity::default()
        });
        let left_out = |tool: Tool| reader.report_cards_left_out(tool.source()).unwrap_or(0);
        let claude_code = ToolStatus { left_out: left_out(Tool::ClaudeCode), ..shown.claude_code };
        let codex = ToolStatus { left_out: left_out(Tool::Codex), ..shown.codex };
        Ok(Status { claude_code, codex, tip_activity, ..shown })
    })
    .await
}

/// The day tips were turned on, when they're on and able to reach a chat.
fn tips_since(env: &Env, shown: &Status) -> Option<NaiveDate> {
    let on = fs::metadata(env.tips_file()).and_then(|m| m.modified()).ok();
    on.filter(|_| shown.claude_code.on).map(|t| DateTime::<Local>::from(t).date_naive())
}

#[tauri::command]
pub async fn report_cards_turn_on(window: tauri::Window, app: State<'_, Arc<App>>, tool: Tool) -> Res<()> {
    main_window_only(&window)?;
    let app = Arc::clone(&app);
    blocking(move || {
        let state = lock(&app.reader).source_state(tool.source());
        require_source(
            tool,
            state.map_err(|e| plain("check which sources are allowed", &e))?.is_some_and(|s| s.allowed),
        )?;
        let _one = exclusive();
        still_open()?;
        turn_on(&Env::system(true)?, tool, &plugin_source())?;
        forget_launch_problem(tool);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn report_cards_turn_off(window: tauri::Window, tool: Tool) -> Res<()> {
    main_window_only(&window)?;
    blocking(move || {
        let _one = exclusive();
        still_open()?;
        turn_off(&Env::system(true)?, tool)?;
        forget_launch_problem(tool);
        Ok(())
    })
    .await
}

/// A fresh install or a removal settles what the launch check was unhappy about.
fn forget_launch_problem(tool: Tool) {
    if tool == Tool::ClaudeCode {
        *launch_problem() = None;
    }
}

#[tauri::command]
pub async fn report_cards_set_tips(window: tauri::Window, app: State<'_, Arc<App>>, on: bool) -> Res<()> {
    main_window_only(&window)?;
    let app = Arc::clone(&app);
    blocking(move || {
        if on {
            let state = lock(&app.reader).source_state(Tool::ClaudeCode.source());
            let allowed = state.map_err(|e| plain("check which sources are allowed", &e))?;
            require_source(Tool::ClaudeCode, allowed.is_some_and(|s| s.allowed))?;
        }
        set_tips(&Env::folders()?, on)
    })
    .await
}

#[tauri::command]
pub async fn report_cards_set_reminders(window: tauri::Window, on: bool) -> Res<()> {
    main_window_only(&window)?;
    blocking(move || set_reminders(&Env::folders()?, on)).await
}

/// Deletes every saved report card. Returns how many there were.
#[tauri::command]
pub async fn report_cards_delete_history(window: tauri::Window, app: State<'_, Arc<App>>) -> Res<usize> {
    main_window_only(&window)?;
    let app = Arc::clone(&app);
    blocking(move || lock(&app.writer).delete_report_cards().map_err(|e| plain("delete the report card history", &e)))
        .await
}

/// How long a move that didn't finish and lost the plugin waits before it is tried again while the app stays open.
const RETRY_EVERY: Duration = Duration::from_secs(60 * 60);

/// One pass of the plugin check, on its own: no other pass and no click on Turn on or Turn off changes
/// the plugin meanwhile, so two moves never run at once. `env` is made once the pass has its turn. With
/// `retry` it only does anything if a move was left unfinished (see `claude::Pending`).
fn check_plugin(env: impl FnOnce() -> Res<Env>, version: &str, retry: bool) -> Res<Option<claude::Launch>> {
    let _one = exclusive();
    still_open()?;
    let env = env()?;
    Ok(if retry { claude::retry_pending(&env, version) } else { Some(claude::launch_check(&env, version)) })
}

/// Keeps what a pass found for Settings: a problem until a later pass finds none.
fn note_check(found: Res<Option<claude::Launch>>, version: &str) {
    match found {
        Ok(Some(claude::Launch::Moved)) => {
            log::info!("report cards: moved the Claude Code plugin to v{version}");
            *launch_problem() = None;
        }
        Ok(Some(claude::Launch::Fine)) => *launch_problem() = None,
        Ok(Some(claude::Launch::Problem(text))) => *launch_problem() = Some(text.into()),
        Ok(None) => {}
        Err(e) => log::warn!("report cards: launch check failed: {e}"),
    }
}

/// After an app update, brings the Codex files we wrote in line with this app (see [`codex::refresh`]), on
/// its own like a pass of the plugin check. `env` is made once it has its turn.
fn refresh_codex(env: impl FnOnce() -> Res<Env>) -> Res<bool> {
    let _one = exclusive();
    still_open()?;
    codex::refresh(&env()?)
}

/// After an app update, moves the plugin to the new release tag (the marketplace is pinned to the
/// version it was added at, and Claude Code doesn't update marketplaces we pinned), and checks that
/// what Claude Code downloaded is what this app ships. Runs in the background, only in release
/// builds. A problem is kept for Settings to show on the Claude Code row. A move that didn't finish
/// and took the plugin away is tried again every [`RETRY_EVERY`] while the app stays open; one that left
/// the old plugin in place waits for the next launch (see `claude::retry_pending`). Claude Code is not
/// asked anything unless report cards are on (see `claude::launch_check`).
pub fn spawn_launch_check() {
    if cfg!(debug_assertions) {
        return;
    }
    std::thread::spawn(|| {
        let version = env!("CARGO_PKG_VERSION");
        // Codex's files are written by this app, so an update has to rewrite them too. Only the folders
        // are needed for that, and the person has to have turned them on for there to be anything to do.
        match refresh_codex(Env::folders) {
            Ok(true) => log::info!("report cards: brought the Codex files up to date with v{version}"),
            Ok(false) => {}
            Err(e) => log::warn!("report cards: couldn't bring the Codex files up to date: {e}"),
        }
        // No login shell here: a launch never reads the person's shell setup.
        let env = || Env::system(false);
        note_check(check_plugin(env, version, false), version);
        loop {
            std::thread::sleep(RETRY_EVERY);
            note_check(check_plugin(env, version, true), version);
        }
    });
}

/// A program's output for the log: cut short, with any `user:password@` taken out of a link in it and
/// the home folder written as `~`.
pub(crate) fn for_log(text: &str) -> String {
    const LIMIT: usize = 1024;
    let clean = crate::logging::shorten_home_in(&without_credentials(text.trim()), std::env::home_dir().as_deref());
    if clean.len() <= LIMIT {
        return clean;
    }
    let cut = (0..=LIMIT).rev().find(|&i| clean.is_char_boundary(i)).unwrap_or(0);
    format!("{}… ({} more bytes)", &clean[..cut], clean.len() - cut)
}

/// `https://user:token@host/x` as `https://host/x`.
fn without_credentials(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("://") {
        let (head, tail) = rest.split_at(at + 3);
        out.push_str(head);
        let end =
            tail.find(|c: char| c.is_whitespace() || matches!(c, '/' | '?' | '#' | '"' | '\'')).unwrap_or(tail.len());
        let (authority, after) = tail.split_at(end);
        out.push_str(authority.rsplit_once('@').map_or(authority, |(_, host)| host));
        rest = after;
    }
    out.push_str(rest);
    out
}
