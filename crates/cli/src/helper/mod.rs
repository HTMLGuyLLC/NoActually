//! The report-card helper: what the `grade` plugin and the Codex hooks run to get a chat's numbers.
//!
//! - `digest --session <id>` or `digest --transcript <path> [--source <id>]`: the chat in about
//!   1.5k tokens, for the model that writes the report card.
//! - `hook session-start` and `hook stop` (Claude Code), `hook codex-prompt` and `hook codex-stop`
//!   (Codex): the hooks.
//!
//! None of them may ever get in the way. On any error or missing data they print nothing and
//! succeed. They write nothing, except `report-cards.key` the first time a digest needs a seal.

mod chat;
mod digest;
mod hook;
mod tail;
#[cfg(test)]
mod tests;

use chat::{find_claude, find_codex, for_path, session_id_ok, tail_start, Found};
use hook::Hook;
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

/// Claude Code's source id.
pub(crate) const CLAUDE_CODE: &str = "claude-code";
/// Codex's source id.
pub(crate) const CODEX: &str = "codex";

/// A file beside the database. While it exists, Claude Code chats get the tip rules (the Settings
/// switch makes and removes it). Its contents are ignored.
pub const TIPS_FILE: &str = "report-cards-tips";

/// A file beside the database, left by turning tips off: a chat that had the rules may still have
/// them, so one that comes back is told to stop. Turning tips on removes it.
pub const TIPS_OFF_FILE: &str = "report-cards-tips-off";

/// A file beside the database. While it exists, the hooks show no one-line reminders (the Settings
/// checkbox "Remind me after a task" makes and removes it).
pub const QUIET_FILE: &str = "report-cards-quiet";

/// Env var that points `digest --session` and the Stop hook at another Claude Code projects folder.
const CLAUDE_PROJECTS_ENV: &str = "NOACTUALLY_CLAUDE_PROJECTS";
/// Env var that points the Codex hooks at another sessions folder.
const CODEX_SESSIONS_ENV: &str = "NOACTUALLY_CODEX_SESSIONS";
/// Set by Claude Code: which app a chat runs in.
const ENTRYPOINT_ENV: &str = "CLAUDE_CODE_ENTRYPOINT";
/// The apps a person chats in. Anything else (`sdk-cli` for `claude -p`, an MCP server, a GitHub
/// Action) has nobody there to read a note or a tip.
const ATTENDED: [&str; 3] = ["cli", "claude-desktop", "claude-vscode"];
/// A hook that takes longer than this gives up silently.
const HOOK_TIMEOUT: Duration = Duration::from_secs(2);
/// A hook's input is a few hundred bytes; this only stops a runaway pipe.
const MAX_STDIN: u64 = 1 << 20;

/// What the helper reads from its surroundings. Tests build one instead of touching the process.
#[derive(Debug, Clone, Default)]
pub(crate) struct Env {
    /// The database path. The seal key, [`TIPS_FILE`] and [`QUIET_FILE`] sit beside it.
    pub db: Option<PathBuf>,
    /// The app Claude Code says it's running in, if it says.
    pub entrypoint: Option<String>,
    /// Where Claude Code transcripts are looked up, instead of the real folders.
    pub claude_projects: Option<PathBuf>,
    /// Where Codex rollouts are looked up, instead of the real folder.
    pub codex_sessions: Option<PathBuf>,
}

impl Env {
    fn from_process() -> Env {
        let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
        Env {
            db: store::default_db_path().ok(),
            entrypoint: var(ENTRYPOINT_ENV).map(|v| v.to_string_lossy().into_owned()),
            claude_projects: var(CLAUDE_PROJECTS_ENV).map(PathBuf::from),
            codex_sessions: var(CODEX_SESSIONS_ENV).map(PathBuf::from),
        }
    }

    /// True when Claude Code says this chat is in an app a person chats in.
    pub fn attended(&self) -> bool {
        self.entrypoint.as_deref().is_some_and(|e| ATTENDED.contains(&e))
    }

    /// True when Claude Code says this chat has nobody there. Saying nothing isn't saying that.
    pub fn headless(&self) -> bool {
        self.entrypoint.is_some() && !self.attended()
    }

    fn beside_db(&self, file: &str) -> bool {
        self.db.as_ref().is_some_and(|db| db.with_file_name(file).exists())
    }

    /// True when tips are switched on.
    pub fn tips(&self) -> bool {
        self.beside_db(TIPS_FILE)
    }

    /// True when the person stopped reading `source`, so hooks may not open its chats.
    pub fn stopped(&self, source: &str) -> bool {
        self.db.as_deref().is_some_and(|db| store::source_stopped(db, source))
    }

    /// True when tips were switched on once and are off now.
    pub fn tips_turned_off(&self) -> bool {
        self.beside_db(TIPS_OFF_FILE)
    }

    /// True when the person turned the one-line reminders off.
    pub fn quiet(&self) -> bool {
        self.beside_db(QUIET_FILE)
    }

    /// The secret that seals report cards. Only a digest makes it (`create`): a hook that finds
    /// none knows no card can be valid yet.
    pub fn secret(&self, create: bool) -> Option<Vec<u8>> {
        store::card_secret(self.db.as_ref()?, create).ok().flatten()
    }
}

/// Which chat a digest is about.
#[derive(Debug, PartialEq)]
pub(crate) enum Target {
    /// A Claude Code or Cowork chat, by id.
    Session(String),
    /// A transcript file. The source is guessed from the file name when not given.
    Transcript { path: PathBuf, source: Option<String> },
}

/// One helper command, parsed.
#[derive(Debug, PartialEq)]
pub(crate) enum Request {
    Digest(Target),
    Hook(Hook),
}

/// True when `word` names a helper command.
pub(crate) fn is_helper_command(word: &str) -> bool {
    matches!(word, "digest" | "hook")
}

/// True when `args` start a helper command, which the desktop app runs without opening a window.
pub(crate) fn is_helper(args: &[String]) -> bool {
    args.first().is_some_and(|word| is_helper_command(word))
}

/// Reads a helper command. `None` for anything malformed: the caller then does nothing.
pub(crate) fn parse(args: &[String]) -> Option<Request> {
    let (cmd, rest) = args.split_first()?;
    match (cmd.as_str(), rest) {
        ("hook", [which]) => Hook::parse(which).map(Request::Hook),
        ("digest", _) => {
            let (mut session, mut path, mut source) = (None, None, None);
            let mut it = rest.iter();
            while let Some(flag) = it.next() {
                let value = it.next()?;
                match flag.as_str() {
                    "--session" => session = Some(value.clone()),
                    "--transcript" => path = Some(PathBuf::from(value)),
                    "--source" => source = Some(value.clone()),
                    _ => return None,
                }
            }
            match (session, path) {
                (Some(id), None) => Some(Request::Digest(Target::Session(id))),
                (None, Some(path)) => Some(Request::Digest(Target::Transcript { path, source })),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Runs a helper command from the process's own arguments, environment and stdin.
pub(crate) fn main(args: &[String]) -> ExitCode {
    // A broken helper must never put anything in front of the person, a panic message included.
    std::panic::set_hook(Box::new(|_| {}));
    let Some(request) = parse(args) else { return ExitCode::SUCCESS };
    if matches!(request, Request::Hook(_)) {
        std::thread::spawn(|| {
            std::thread::sleep(HOOK_TIMEOUT);
            std::process::exit(0);
        });
    }
    let out = std::panic::catch_unwind(|| handle(&request, &Env::from_process(), read_stdin)).ok().flatten();
    if let Some(out) = out {
        // A closed pipe is not worth a message.
        let mut stdout = std::io::stdout().lock();
        let _ = stdout.write_all(out.as_bytes());
        if !out.ends_with('\n') {
            let _ = stdout.write_all(b"\n");
        }
        let _ = stdout.flush();
    }
    ExitCode::SUCCESS
}

/// What a helper command prints, if anything. `stdin` is read only by the hooks.
pub(crate) fn handle(request: &Request, env: &Env, stdin: impl FnOnce() -> String) -> Option<String> {
    match request {
        Request::Digest(target) => {
            let found = match target {
                Target::Session(_) if env.stopped(CLAUDE_CODE) => return None,
                Target::Session(id) if session_id_ok(id) => find_claude(env, id)?,
                Target::Session(_) => return None,
                Target::Transcript { path, source } => {
                    let found = for_path(path, source.as_deref())?;
                    if env.stopped(found.source.id()) {
                        return None;
                    }
                    found
                }
            };
            digest_of(&found, env, true)
        }
        Request::Hook(hook) => {
            let input: serde_json::Value = serde_json::from_str(&stdin()).ok()?;
            hook::run(*hook, &input, env)
        }
    }
}

/// The digest of the chat in `found`, sealed with this machine's secret. It reads the last
/// [`digest::READ_TAIL`] bytes. With `usage` it also counts tokens, which a Codex rollout can only do
/// by reading it from the top, whatever its length; without, the rows show no cost and the read is
/// the tail alone, which is how a hook (with a couple of seconds) gets a digest of a very long chat.
fn digest_of(found: &Found, env: &Env, usage: bool) -> Option<String> {
    let secret = env.secret(true)?;
    let chat = found.chat_from(Some(&secret), tail_start(&found.path, digest::READ_TAIL)?, usage)?;
    Some(digest::render(&chat, &engine::seal(&secret, &chat.view.session), usage))
}

/// The hook's input. Nothing at a terminal, where there is no hook to read.
fn read_stdin() -> String {
    let stdin = std::io::stdin();
    let mut text = String::new();
    if !stdin.is_terminal() {
        let _ = stdin.lock().take(MAX_STDIN).read_to_string(&mut text);
    }
    text
}

/// The transcript a hook is about: the path it was given, else the chat found by its id.
fn transcript(input: &serde_json::Value, env: &Env, codex: bool) -> Option<Found> {
    if let Some(path) = input["transcript_path"].as_str().filter(|p| !p.is_empty()) {
        return for_path(Path::new(path), codex.then_some("codex"));
    }
    let id = input["session_id"].as_str()?;
    if codex {
        find_codex(env, id)
    } else {
        find_claude(env, id)
    }
}
