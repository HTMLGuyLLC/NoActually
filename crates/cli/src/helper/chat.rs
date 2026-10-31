//! Finding a chat's transcript, and reading it as tasks the way the digest and the hooks need.

use super::Env;
use engine::{tasks_overlap, Prompt};
use sources::{ChatView, Source};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// How much of the end of a chat a hook reads first, fast path and slow alike.
const FIRST_WINDOW: u64 = 256 * 1024;
/// The most a hook ever reads. A message further back than this is not "just sent", and a task that
/// began before the last [`MAX_WINDOW`] bytes is left alone rather than read for.
const MAX_WINDOW: u64 = 8 * 1024 * 1024;
/// How much wider each look is than the one before.
const WIDEN: u64 = 8;

/// The Codex source's id.
pub(crate) const CODEX: &str = "codex";

/// The sources whose chats can carry a report card, in display order.
fn gradable() -> Vec<Box<dyn Source>> {
    sources::all().into_iter().filter(|s| s.gradable()).collect()
}

/// A transcript on disk, and the adapter that reads it.
pub(crate) struct Found {
    pub source: Box<dyn Source>,
    pub path: PathBuf,
}

impl Found {
    /// The whole chat, read once. `secret` checks which report cards are real.
    pub fn chat(&self, secret: Option<&[u8]>) -> Option<Chat> {
        let view = self.source.view(&self.path)?.ok()?;
        Some(Chat::new(view, secret, false))
    }

    /// The chat from byte offset `from` (a line start, see [`tail_start`]) to its end. With `usage`
    /// the prompts carry what they cost; without, nothing before `from` is parsed.
    pub fn chat_from(&self, secret: Option<&[u8]>, from: u64, usage: bool) -> Option<Chat> {
        if from == 0 && usage {
            return self.chat(secret);
        }
        let view = self.source.view_from(&self.path, from, usage)?.ok()?;
        Some(Chat::new(view, secret, from > 0))
    }
}

/// What one look at the end of a chat said.
pub(crate) enum Look<T> {
    /// The answer.
    Found(T),
    /// There is none, and a wider look won't change that.
    Never,
    /// Not here, or not all of it: look further back.
    Further,
}

/// Runs `look` on the end of the file at `path`: a window of [`FIRST_WINDOW`] bytes, then wider ones
/// ([`WIDEN`] times each) up to [`MAX_WINDOW`], until it finds something, rules it out, or has seen the
/// whole file. `look` gets the offset of the window's first whole line (0 when the window covers the
/// file). One window is one pass over the end of the chat, so a chat's length never slows a hook down.
pub(crate) fn in_tail<T>(path: &Path, mut look: impl FnMut(u64) -> Look<T>) -> Option<T> {
    let len = std::fs::metadata(path).ok()?.len();
    let mut window = FIRST_WINDOW;
    loop {
        // No line begins in a window that falls inside one very long line: widen.
        let step = tail_start(path, window).map_or(Look::Further, &mut look);
        match step {
            Look::Found(found) => return Some(found),
            Look::Never => return None,
            Look::Further if len <= window || window >= MAX_WINDOW => return None,
            Look::Further => window = (window * WIDEN).min(MAX_WINDOW),
        }
    }
}

/// Where to start reading to get the last `window` bytes of the file at `path` as whole lines: the
/// start of the first line that begins inside them, or 0 when they cover the file. `None` when the
/// file can't be read or no line begins inside the window.
pub(crate) fn tail_start(path: &Path, window: u64) -> Option<u64> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let from = len.saturating_sub(window);
    if from == 0 {
        return Some(0);
    }
    // One byte back, to tell a window that starts on a line from one that starts inside it.
    file.seek(SeekFrom::Start(from - 1)).ok()?;
    let mut line = Vec::new();
    BufReader::new(file).read_until(b'\n', &mut line).ok()?;
    (line.last() == Some(&b'\n')).then(|| from - 1 + line.len() as u64).filter(|start| *start < len)
}

/// A session id as `digest --session` takes it: letters, digits and '-', 8 to 64 of them (Claude Code's
/// are UUIDs). Anything else is refused before it gets near the file system, so no path, dot or
/// separator can name a file outside the chats.
pub(crate) fn session_id_ok(id: &str) -> bool {
    (8..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// A session id is a file name, never a path.
fn safe_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 200 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The Claude Code or Cowork chat with this session id.
pub(crate) fn find_claude(env: &Env, id: &str) -> Option<Found> {
    if !safe_id(id) {
        return None;
    }
    let name = format!("{id}.jsonl");
    let all: Vec<Box<dyn Source>> = match &env.claude_projects {
        Some(root) => vec![sources::claude_code_at(root.clone())],
        None => gradable().into_iter().filter(|s| s.id() != CODEX).collect(),
    };
    all.into_iter().find_map(|source| {
        let path = source.roots().into_iter().find_map(|root| {
            // Claude Code keeps a chat one folder down; walk for any that is deeper.
            one_down(&root, &name)
                .or_else(|| source.files(&root).into_iter().find(|p| p.file_name() == Some(name.as_ref())))
        })?;
        Some(Found { source, path })
    })
}

/// `<root>/<any folder>/<name>`, without walking the whole tree.
fn one_down(root: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(root).ok()?.flatten().map(|e| e.path().join(name)).find(|p| p.is_file())
}

/// The Codex rollout of this session, newest day first: `sessions/<year>/<month>/<day>/rollout-*<id>.jsonl`.
pub(crate) fn find_codex(env: &Env, id: &str) -> Option<Found> {
    if !safe_id(id) {
        return None;
    }
    let source = gradable().into_iter().find(|s| s.id() == CODEX)?;
    let root = env.codex_sessions.clone().or_else(|| source.roots().into_iter().next())?;
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut subdirs = Vec::new();
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            match e.file_type() {
                Ok(t) if t.is_dir() => subdirs.push(e.path()),
                Ok(t)
                    if t.is_file()
                        && is_rollout_name(&name)
                        && name.contains(id)
                        && e.path().extension().is_some_and(|x| x == "jsonl") =>
                {
                    return Some(Found { source, path: e.path() });
                }
                _ => {}
            }
        }
        subdirs.sort();
        stack.extend(subdirs); // the newest day sorts last, so it's popped first
    }
    None
}

fn is_rollout_name(name: &str) -> bool {
    name.starts_with("rollout-")
}

/// The transcript at `path`, if its source can carry a report card. Without `source`, a `rollout-*` file
/// is Codex and anything else is Claude Code.
pub(crate) fn for_path(path: &Path, source: Option<&str>) -> Option<Found> {
    let id = match source {
        Some(id) => id,
        None if path.file_name().is_some_and(|n| is_rollout_name(&n.to_string_lossy())) => CODEX,
        None => "claude-code",
    };
    let source = gradable().into_iter().find(|s| s.id() == id)?;
    Some(Found { source, path: path.to_path_buf() })
}

/// One task in a chat: the prompts from `start` up to (not including) `end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Task {
    pub start: usize,
    pub end: usize,
}

/// A chat read into tasks, with what is already graded.
pub(crate) struct Chat {
    pub view: ChatView,
    pub tasks: Vec<Task>,
    /// The view began part way into the chat. Its first task is a real start (the view reads
    /// its first prompt as any later one), but prompts before that belong to a task begun
    /// earlier, and there may be tasks before it that the view doesn't hold.
    pub cut: bool,
    graded: Vec<bool>,
}

impl Chat {
    /// A task starts at each prompt the app's rules read as one, and runs to the next. Messages
    /// before the first task (a hello) belong to none. `cut`: see [`Chat::cut`].
    pub fn new(view: ChatView, secret: Option<&[u8]>, cut: bool) -> Chat {
        let n = view.prompts.len();
        let mut tasks: Vec<Task> = Vec::new();
        for (i, p) in view.prompts.iter().enumerate() {
            if matches!(p.prompt, Prompt::Task { .. }) {
                if let Some(last) = tasks.last_mut() {
                    last.end = i;
                }
                tasks.push(Task { start: i, end: n });
            }
        }
        let graded = graded(&view, &tasks, secret);
        Chat { view, tasks, cut, graded }
    }

    /// True when prompt `i` is only a sign-off.
    pub fn is_signoff(&self, i: usize) -> bool {
        self.view.prompts[i].signoff
    }

    /// Prompts in `task` that are rounds of work: everything but sign-offs.
    pub fn rounds(&self, task: Task) -> usize {
        self.view.rounds(task.start..task.end)
    }

    /// True when a report card with this chat's seal covers task number `i`.
    pub fn is_graded(&self, i: usize) -> bool {
        self.graded[i]
    }

    /// The number of the task prompt `i` falls in.
    pub fn task_of(&self, i: usize) -> Option<usize> {
        self.tasks.iter().rposition(|t| t.start <= i)
    }
}

/// Which tasks a valid report card covers: those that overlap one of its tasks by the store's rule
/// (`engine::tasks_overlap`): the same first prompt, or more than an endpoint in common. A card
/// whose task ends on the prompt a later task starts on doesn't cover the later one. A card whose
/// seal isn't this chat's (copied from another chat, a web page, these docs), or that names prompts
/// this chat doesn't have, covers nothing (`ChatView::sealed_spans`).
fn graded(view: &ChatView, tasks: &[Task], secret: Option<&[u8]>) -> Vec<bool> {
    let Some(secret) = secret else { return vec![false; tasks.len()] };
    let spans: Vec<(usize, usize)> = view.sealed_spans(secret).tasks.iter().map(|t| (t.at, t.to)).collect();
    tasks
        .iter()
        .map(|t| {
            let span = (t.start, t.end.saturating_sub(1).max(t.start));
            spans.iter().any(|&(from, to)| tasks_overlap(span, (from, to), from == t.start))
        })
        .collect()
}
