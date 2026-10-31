//! The hooks. Claude Code runs the Stop hook after every reply and Codex runs its two on each
//! prompt and each reply, so most of the work is saying nothing, quickly: a reply gets a closer
//! look only when the message it answered was a sign-off ("thanks") or the start of a new task.
//!
//! Nothing here calls a model. The most a hook does is show a one-line note, or give a chat the
//! tip rules when it starts.

use super::chat::{in_tail, Chat, Look};
use super::{digest_of, tail, transcript, Env, CLAUDE_CODE, CODEX};
use engine::{is_signoff, read_prompt, AiMsg, Prompt, Turn, Verdict, TIP_RULES, TIP_STOP};
use serde_json::{json, Value};
use sources::{has_tip_rules, GRADE_COMMAND, GRADE_SKILL_CODEX};

/// The hooks the plugin and the app install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hook {
    /// Claude Code's `SessionStart`: the tip rules, when tips are on.
    SessionStart,
    /// Claude Code's Stop.
    Stop,
    /// Codex's `UserPromptSubmit`: hands `$grade-me` the digest.
    CodexPrompt,
    /// Codex's Stop.
    CodexStop,
}

impl Hook {
    pub fn parse(name: &str) -> Option<Hook> {
        match name {
            "session-start" => Some(Hook::SessionStart),
            "stop" => Some(Hook::Stop),
            "codex-prompt" => Some(Hook::CodexPrompt),
            "codex-stop" => Some(Hook::CodexStop),
            _ => None,
        }
    }
}

/// What the helper prints for `hook`, whose input is `input`.
pub(crate) fn run(hook: Hook, input: &Value, env: &Env) -> Option<String> {
    let source = if matches!(hook, Hook::CodexPrompt | Hook::CodexStop) { CODEX } else { CLAUDE_CODE };
    if env.stopped(source) {
        // Its chats are off limits, so this can't check whether the rules are in this one.
        let tips_were_on = env.tips() || env.tips_turned_off();
        return (matches!(hook, Hook::SessionStart) && tips_were_on && comes_back(input)).then(|| context(TIP_STOP));
    }
    match hook {
        Hook::SessionStart => session_start(input, env),
        Hook::CodexPrompt => codex_prompt(input, env),
        Hook::Stop => stop(input, env, false),
        Hook::CodexStop => stop(input, env, true),
    }
}

/// What the AI is told as a chat starts. A resumed or forked chat keeps what it was told, so it
/// isn't told twice. After tips are turned off, a chat that comes back is told to stop.
fn session_start(input: &Value, env: &Env) -> Option<String> {
    if !env.attended() {
        return None;
    }
    let carried_over = matches!(input["source"].as_str(), Some("resume" | "fork"));
    let told = || transcript(input, env, false).is_some_and(|found| has_tip_rules(&found.path));
    let context = if env.tips() {
        (!(carried_over && told())).then_some(TIP_RULES)?
    } else {
        (env.tips_turned_off() && comes_back(input) && told()).then_some(TIP_STOP)?
    };
    Some(self::context(context))
}

/// True for a chat that carries on from before, and so may still have the rules.
fn comes_back(input: &Value) -> bool {
    matches!(input["source"].as_str(), Some("resume" | "fork" | "compact"))
}

/// What the hook prints to give the chat `text`.
fn context(text: &str) -> String {
    json!({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": text}}).to_string()
}

/// What the end of a chat says to do.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// Show the person this line.
    Note(String),
    /// Nothing to say.
    Quiet,
    /// The tasks it needs begin before what was read.
    Wider,
}

fn stop(input: &Value, env: &Env, codex: bool) -> Option<String> {
    // No person is there to see a note; and a reply to another hook's stop isn't one they asked for.
    if (env.headless() && !codex) || input["stop_hook_active"].as_bool() == Some(true) || env.quiet() {
        return None;
    }
    let found = transcript(input, env, codex)?;
    if found.path.components().any(|c| c.as_os_str() == "subagents") {
        return None;
    }
    // The fast path: the end of the file alone says whether there is anything to do.
    let last = if codex {
        tail::last_codex_message(&found)?
    } else {
        tail::last_claude_message(&found, input["prompt_id"].as_str().filter(|id| !id.is_empty()))?
    };
    if !is_signoff(&last) && !may_start_task(&last) {
        return None;
    }
    // The slow path: the end of the chat, to find the task that just ended and whether it has a card.
    // It looks at the same windows as the fast path, and widens only while the task it needs begins
    // before what it read.
    let secret = env.secret(false);
    let cmd = if codex { GRADE_SKILL_CODEX } else { GRADE_COMMAND };
    let note = in_tail(&found.path, |from| {
        let Some(chat) = found.chat_from(secret.as_deref(), from, false) else { return Look::Never };
        match decide(&chat, cmd) {
            Outcome::Note(note) => Look::Found(note),
            Outcome::Quiet => Look::Never,
            Outcome::Wider => Look::Further,
        }
    })?;
    Some(json!({"systemMessage": note}).to_string())
}

/// True when `text` could start a new task in a chat that has one already. A cheap check made
/// with the app's own rules; the chat itself has the last word.
pub(super) fn may_start_task(text: &str) -> bool {
    let turn = Turn { human: text.to_string(), prev_ai: Some(AiMsg::default()), ..Turn::default() };
    let prompt = read_prompt(&turn, &Verdict::default(), true);
    matches!(prompt, Prompt::Task { .. }) && !prompt.is_first_request()
}

/// What to say after the last prompt, if anything. `cmd` is how this tool's person asks for a report
/// card. A chat read from part way in (`chat.cut`) asks for [`Outcome::Wider`] when the answer
/// depends on a task that began before it.
fn decide(chat: &Chat, cmd: &str) -> Outcome {
    let prompts = &chat.view.prompts;
    let Some(last) = prompts.len().checked_sub(1) else { return Outcome::Quiet };
    let unknown = if chat.cut { Outcome::Wider } else { Outcome::Quiet };
    // No task started before it in what was read: the one it belongs to began earlier.
    let Some(task_no) = chat.task_of(last) else { return unknown };
    let task = chat.tasks[task_no];
    if chat.is_signoff(last) {
        // Only the first sign-off after real work speaks: "thanks" twice in a row is one thanks.
        if last == 0 || chat.is_signoff(last - 1) {
            return Outcome::Quiet;
        }
        return match chat.rounds(task) {
            0 => Outcome::Quiet,
            1 => Outcome::Note("✓ Clear the first time.".into()),
            _ if chat.is_graded(task_no) => Outcome::Quiet,
            _ => Outcome::Note(format!("Want a report card on that? Type {cmd}")),
        };
    }
    // A new task right after one that took follow-ups. If that one ended on a sign-off, the
    // person has been asked already.
    if !matches!(prompts[last].prompt, Prompt::Task { .. }) || prompts[last].prompt.is_first_request() {
        return Outcome::Quiet;
    }
    let Some(before) = task_no.checked_sub(1) else { return unknown };
    let prev = chat.tasks[before];
    let done = chat.is_signoff(prev.end - 1);
    if !done && chat.rounds(prev) >= 2 && !chat.is_graded(before) {
        return Outcome::Note(format!("Last task done? Type {cmd} for a report card."));
    }
    Outcome::Quiet
}

/// Codex's `$grade-me` has no way to run a command, so the hook hands it the digest as context. The
/// hook has a couple of seconds, so the digest reads the end of the rollout only and shows no token
/// counts (Codex has no price to show them as dollars, and the numbers that matter are worked out
/// from the whole chat by the app later).
fn codex_prompt(input: &Value, env: &Env) -> Option<String> {
    let rest = input["prompt"].as_str()?.trim_start().strip_prefix(GRADE_SKILL_CODEX)?;
    if rest.chars().next().is_some_and(|c| !c.is_whitespace()) {
        return None; // "$grade-meal" is some other skill
    }
    let digest = digest_of(&transcript(input, env, true)?, env, false)?;
    Some(json!({"hookSpecificOutput": {"hookEventName": "UserPromptSubmit", "additionalContext": digest}}).to_string())
}
