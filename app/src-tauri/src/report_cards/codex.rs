//! Codex: a skill in `~/.agents/skills/grade-me/` and two hooks in `~/.codex/hooks.json`. The skill
//! is the Claude Code skill with the Codex name; the hooks run the same shim the plugin uses.

use super::plugin_files::lf;
use super::{plain, Env, Res};
use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sources::{GRADE_COMMAND, GRADE_SKILL_CODEX};
use std::path::{Path, PathBuf};
use store::write_atomic;

/// The source of truth for the skill's text.
const SKILL_SOURCE: &str = include_str!("../../../../plugins/grade/skills/me/SKILL.md");
/// The shim the hooks run (it finds the app's helper).
const SHIM_SOURCE: &str = include_str!("../../../../plugins/grade/bin/na");

/// The skill's name in Codex, which is also its folder: [`GRADE_SKILL_CODEX`] without its `$`.
fn skill_name() -> &'static str {
    GRADE_SKILL_CODEX.trim_start_matches('$')
}

/// Every command we add to `hooks.json` contains this, which is how our entries are found again.
/// (`grade-me` is [`skill_name`]; a test holds the two together.)
pub(super) const HOOK_MARK: &str = "/.agents/skills/grade-me/scripts/na";

/// (event, argument for the helper, seconds before Codex gives up on it)
const HOOKS: [(&str, &str, u32); 2] = [("UserPromptSubmit", "codex-prompt", 10), ("Stop", "codex-stop", 5)];

/// The first line of the skill's text after its front matter. It says the file is ours: a `grade-me`
/// skill without it is somebody else's, and is never overwritten or removed.
const SKILL_MARK: &str = "<!-- installed by No, Actually... -->";

const NOT_INSTALLED: &str = "Codex isn't installed.";
const NOT_OURS: &str = "There's already a Codex skill called grade-me (~/.agents/skills/grade-me) that isn't from this app. Rename or remove it, then try again.";
const BAD_HOOKS: &str = "Couldn't read your Codex hooks file (~/.codex/hooks.json). Fix it, then try again.";

// ---- where things go ------------------------------------------------------------------------

fn skill_dir(env: &Env) -> PathBuf {
    env.home.join(".agents").join("skills").join(skill_name())
}

fn skill_file(env: &Env) -> PathBuf {
    skill_dir(env).join("SKILL.md")
}

fn shim_file(env: &Env) -> PathBuf {
    skill_dir(env).join("scripts").join("na")
}

/// Codex's own folder: `$CODEX_HOME`, else `~/.codex`. A variable that isn't an absolute path counts as unset.
pub(super) fn codex_dir(env: &Env) -> PathBuf {
    codex_dir_in(std::env::var_os("CODEX_HOME"), &env.home)
}

fn codex_dir_in(var: Option<std::ffi::OsString>, home: &Path) -> PathBuf {
    var.map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home.join(".codex"))
}

fn hooks_file(env: &Env) -> PathBuf {
    codex_dir(env).join("hooks.json")
}

/// Codex is here if it has a folder in the home folder (the app reads its chats from there) or a
/// command was found.
pub(super) fn installed(env: &Env) -> bool {
    env.codex.is_some() || codex_dir(env).is_dir()
}

// ---- the skill ------------------------------------------------------------------------------

/// What replaces the line that runs the digest command: Codex has no such lines, so its prompt hook
/// puts the summary in the request instead.
const SUMMARY_LINE: &str = "Look for the chat summary No, Actually... added to this request; it starts with \"noactually digest v1\". If there isn't one, the app isn't running.";
/// What replaces the line with the slash command's argument, which Codex doesn't fill in.
fn request_line() -> String {
    format!("Request: whatever the person typed after {GRADE_SKILL_CODEX} (nothing, or \"all\").")
}

/// The Claude Code skill, reworded for Codex: its own name and `$grade-me` for `/grade:me`, no
/// Claude-only front matter, no digest command, and [`SKILL_MARK`] to say who installed it. A change
/// to the skill that moves any of these lines is an error here (and in the tests) rather than a
/// quietly broken Codex skill. Windows line endings in `source` are read as Unix ones, so the copy
/// is the same whichever way the file was checked out.
pub(super) fn codex_skill(source: &str) -> Result<String, String> {
    let source = lf(source);
    let rest = source.strip_prefix("---\n").ok_or("the skill has no front matter")?;
    let (front, body) = rest.split_once("\n---\n").ok_or("the skill's front matter doesn't end")?;
    let mut out = format!("---\nname: {}\n", skill_name());
    let (mut description, mut tools, mut hint) = (false, false, false);
    for line in front.lines() {
        if line.starts_with("allowed-tools:") {
            tools = true;
        } else if line.starts_with("argument-hint:") {
            hint = true;
        } else {
            description |= line.starts_with("description:") && line.contains(GRADE_COMMAND);
            out.push_str(&line.replace(GRADE_COMMAND, GRADE_SKILL_CODEX));
            out.push('\n');
        }
    }
    out.push_str("---\n");
    out.push_str(SKILL_MARK);
    out.push('\n');
    let (mut summary, mut request) = (false, false);
    for line in body.split_inclusive('\n') {
        let text = line.trim_end_matches('\n');
        if text.starts_with("!`") {
            summary = true;
            out.push_str(SUMMARY_LINE);
            out.push_str(&line[text.len()..]);
        } else if text == "Request: $ARGUMENTS" {
            request = true;
            out.push_str(&request_line());
            out.push_str(&line[text.len()..]);
        } else {
            out.push_str(&line.replace(GRADE_COMMAND, GRADE_SKILL_CODEX));
        }
    }
    let missing: Vec<String> = [
        (description, format!("a description that names {GRADE_COMMAND}")),
        (tools, "an allowed-tools line".into()),
        (hint, "an argument-hint line".into()),
        (summary, "the line that runs the digest".into()),
        (request, "the Request: $ARGUMENTS line".into()),
    ]
    .into_iter()
    .filter(|(found, _)| !found)
    .map(|(_, what)| what)
    .collect();
    if !missing.is_empty() {
        return Err(format!("the skill no longer has {}", missing.join(", ")));
    }
    // Nothing from Claude Code's plugin machinery may be left in the Codex copy.
    if out.contains("${CLAUDE_") || out.contains(GRADE_COMMAND) {
        return Err("the Codex skill still has Claude Code wording".into());
    }
    Ok(out)
}

/// Whether `text` is a skill this app wrote: its text after the front matter starts with [`SKILL_MARK`].
/// (An editor that saved it with Windows line endings doesn't make it someone else's.)
fn is_our_skill(text: &str) -> bool {
    let text = lf(text);
    text.strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .is_some_and(|(_, body)| body.lines().next() == Some(SKILL_MARK))
}

/// The `grade-me` skill that's there now, if any. One that isn't ours is an error for `turn_on` to
/// pass on, so it is never written over.
fn existing_skill(env: &Env) -> Res<Option<String>> {
    match std::fs::read_to_string(skill_file(env)) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        // Not readable as text: not one we wrote.
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => Ok(Some(String::new())),
        Err(e) => Err(plain("read the Codex skill", &e)),
    }
}

// ---- hooks.json -----------------------------------------------------------------------------

/// JSON that keeps object keys in file order, so adding our hooks doesn't reshuffle anyone's file.
/// (`serde_json::Value` sorts keys. Its `preserve_order` feature would do this, but it is on for the
/// whole workspace and makes every `Value` the chat readers parse about half again slower: measured
/// at 456 ms against 684 ms for 400,000 Codex-shaped lines. Not worth it for one small file.)
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Json {
    Null,
    Bool(bool),
    Num(serde_json::Number),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Serialize for Json {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Json::Null => s.serialize_unit(),
            Json::Bool(b) => s.serialize_bool(*b),
            Json::Num(n) => n.serialize(s),
            Json::Str(t) => s.serialize_str(t),
            Json::Arr(items) => items.serialize(s),
            Json::Obj(fields) => {
                let mut map = s.serialize_map(Some(fields.len()))?;
                for (k, v) in fields {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Json, D::Error> {
        struct Any;
        impl<'de> Visitor<'de> for Any {
            type Value = Json;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON")
            }
            fn visit_unit<E>(self) -> Result<Json, E> {
                Ok(Json::Null)
            }
            fn visit_bool<E>(self, b: bool) -> Result<Json, E> {
                Ok(Json::Bool(b))
            }
            fn visit_i64<E>(self, n: i64) -> Result<Json, E> {
                Ok(Json::Num(n.into()))
            }
            fn visit_u64<E>(self, n: u64) -> Result<Json, E> {
                Ok(Json::Num(n.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, n: f64) -> Result<Json, E> {
                serde_json::Number::from_f64(n).map(Json::Num).ok_or_else(|| E::custom("not a JSON number"))
            }
            fn visit_str<E>(self, t: &str) -> Result<Json, E> {
                Ok(Json::Str(t.to_string()))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element()? {
                    items.push(item);
                }
                Ok(Json::Arr(items))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
                let mut fields = Vec::new();
                while let Some(field) = map.next_entry()? {
                    fields.push(field);
                }
                Ok(Json::Obj(fields))
            }
        }
        d.deserialize_any(Any)
    }
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn get_mut(&mut self, key: &str) -> Option<&mut Json> {
        match self {
            Json::Obj(fields) => fields.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The field `key` of an object, added as `make()` if it isn't there. `None` if `self` isn't an object.
    fn field(&mut self, key: &str, make: fn() -> Json) -> Option<&mut Json> {
        let Json::Obj(fields) = self else { return None };
        let at = fields.iter().position(|(k, _)| k == key).unwrap_or_else(|| {
            fields.push((key.to_string(), make()));
            fields.len() - 1
        });
        Some(&mut fields[at].1)
    }
}

/// The command that runs the shim with `arg`. The path is quoted so a home folder with a space in it works.
pub(super) fn hook_command(env: &Env, arg: &str) -> String {
    let home = env.home.to_string_lossy();
    let path = format!("{}{HOOK_MARK}", home.trim_end_matches(['/', '\\']));
    // Inside double quotes a shell still reads these four.
    let quoted = if cfg!(unix) {
        path.replace('\\', "\\\\").replace('"', "\\\"").replace('$', "\\$").replace('`', "\\`")
    } else {
        path
    };
    format!("\"{quoted}\" hook {arg}")
}

fn hook_entry(command: String, timeout: u32) -> Json {
    Json::Obj(vec![
        ("type".into(), Json::Str("command".into())),
        ("command".into(), Json::Str(command)),
        ("timeout".into(), Json::Num(timeout.into())),
    ])
}

fn is_ours(hook: &Json) -> bool {
    matches!(hook.get("command"), Some(Json::Str(c)) if c.contains(HOOK_MARK))
}

/// The events (keys of `hooks`) that have one of our hooks.
fn events_with_ours(root: &Json) -> Vec<&str> {
    let Some(Json::Obj(events)) = root.get("hooks") else { return Vec::new() };
    let ours_in = |groups: &Json| match groups {
        Json::Arr(list) => {
            list.iter().any(|g| matches!(g.get("hooks"), Some(Json::Arr(hooks)) if hooks.iter().any(is_ours)))
        }
        _ => false,
    };
    events.iter().filter(|(_, groups)| ours_in(groups)).map(|(event, _)| event.as_str()).collect()
}

/// Puts our two hooks in `root`. One of ours that's already there is updated where it stands (and
/// repeats are dropped); a missing one is appended in a group of its own. Returns whether anything
/// changed, or `None` if `root` isn't shaped like a hooks file.
fn add_hooks(root: &mut Json, env: &Env) -> Option<bool> {
    let before = root.clone();
    let hooks = root.field("hooks", || Json::Obj(Vec::new()))?;
    if !matches!(hooks, Json::Obj(_)) {
        return None;
    }
    for (event, arg, timeout) in HOOKS {
        let Json::Arr(groups) = hooks.field(event, || Json::Arr(Vec::new()))? else { return None };
        let entry = hook_entry(hook_command(env, arg), timeout);
        let mut placed = false;
        groups.retain_mut(|group| {
            let Some(Json::Arr(inner)) = group.get_mut("hooks") else { return true };
            if !inner.iter().any(is_ours) {
                return true;
            }
            let mut kept = Vec::with_capacity(inner.len());
            for hook in std::mem::take(inner) {
                if !is_ours(&hook) {
                    kept.push(hook);
                } else if !placed {
                    kept.push(entry.clone());
                    placed = true;
                }
            }
            *inner = kept;
            !inner.is_empty() // a group that held only a repeat of ours goes
        });
        if !placed {
            groups.push(Json::Obj(vec![("hooks".into(), Json::Arr(vec![entry]))]));
        }
    }
    Some(*root != before)
}

/// Brings the command of each of our hooks that is in `root` in line with this app's (see
/// [`hook_command`]), where it differs. Nothing else is touched: not a hook's timeout or any other
/// key, not a hook that isn't there, not anyone else's. Returns whether anything changed, or `None`
/// if `root` isn't shaped like a hooks file.
fn update_commands(root: &mut Json, env: &Env) -> Option<bool> {
    let before = root.clone();
    let Json::Obj(top) = root else { return None };
    let Some(at) = top.iter().position(|(k, _)| k == "hooks") else { return Some(false) };
    let Json::Obj(events) = &mut top[at].1 else { return None };
    for (event, arg, _) in HOOKS {
        let Some(Json::Arr(groups)) = events.iter_mut().find(|(k, _)| k == event).map(|(_, v)| v) else { continue };
        let want = hook_command(env, arg);
        for group in groups {
            let Some(Json::Arr(inner)) = group.get_mut("hooks") else { continue };
            for hook in inner.iter_mut().filter(|h| is_ours(h)) {
                if let Some(Json::Str(command)) = hook.get_mut("command") {
                    if *command != want {
                        command.clone_from(&want);
                    }
                }
            }
        }
    }
    Some(*root != before)
}

/// Takes our hooks out of `root`, and any group, event or `hooks` object that leaves empty (one
/// that was empty already is the person's). Returns whether anything changed, or `None` if `root`
/// isn't shaped like a hooks file.
fn remove_hooks(root: &mut Json) -> Option<bool> {
    let before = root.clone();
    let Json::Obj(top) = root else { return None };
    let Some(at) = top.iter().position(|(k, _)| k == "hooks") else { return Some(false) };
    let Json::Obj(events) = &mut top[at].1 else { return None };
    let mut emptied = Vec::new();
    for (event, groups) in events.iter_mut() {
        let Json::Arr(list) = groups else { continue };
        let had = list.len();
        list.retain_mut(|group| {
            let Some(Json::Arr(inner)) = group.get_mut("hooks") else { return true };
            let n = inner.len();
            inner.retain(|h| !is_ours(h));
            n == inner.len() || !inner.is_empty()
        });
        if had > 0 && list.is_empty() {
            emptied.push(event.clone());
        }
    }
    events.retain(|(event, _)| !emptied.contains(event));
    if events.is_empty() && !emptied.is_empty() {
        top.remove(at);
    }
    Some(*root != before)
}

/// `hooks.json`, `None` if there is none yet. A file that isn't JSON is an error: it's theirs, so we
/// never overwrite it.
fn read_hooks(path: &Path) -> Res<Option<Json>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(plain("read your Codex hooks file", &e)),
    };
    if text.trim().is_empty() {
        return Ok(Some(Json::Obj(Vec::new())));
    }
    serde_json::from_str(&text).map(Some).map_err(|e| {
        log::warn!("report cards: {} isn't JSON: {e}", path.display());
        BAD_HOOKS.to_string()
    })
}

/// Copies the hooks file as it is to `hooks.json.bak-YYYYMMDD-HHMMSS` beside it, never over an
/// earlier copy (a second one in the same second gets `-1`, `-2`...).
fn back_up(real: &Path) -> std::io::Result<()> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let mut src = std::fs::File::open(real)?;
    for n in 0.. {
        let name = if n == 0 { format!("hooks.json.bak-{stamp}") } else { format!("hooks.json.bak-{stamp}-{n}") };
        let mut open = std::fs::OpenOptions::new();
        open.write(true).create_new(true);
        match open.open(real.with_file_name(name)) {
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
            Ok(mut dst) => {
                std::io::copy(&mut src, &mut dst)?;
                return dst.set_permissions(src.metadata()?.permissions());
            }
        }
    }
    unreachable!("the loop only ends by returning")
}

/// Saves `root` as `hooks.json` (through a symlink if it is one, which dotfile managers like). With
/// `backup`, the file as it was is first copied to a `hooks.json.bak-…` of its own, every time.
/// A file left with nothing in it is deleted: it was only ever ours.
fn save_hooks(path: &Path, root: &Json, backup: bool) -> Res<()> {
    let real = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let fail = |e: std::io::Error| plain("save your Codex hooks file", &e);
    if matches!(root, Json::Obj(fields) if fields.is_empty()) {
        return match std::fs::remove_file(&real) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(fail(e)),
            _ => Ok(()),
        };
    }
    if backup {
        back_up(&real).map_err(fail)?;
    }
    let mut text = serde_json::to_string_pretty(root).map_err(|e| plain("save your Codex hooks file", &e))?;
    text.push('\n');
    if let Some(dir) = real.parent() {
        std::fs::create_dir_all(dir).map_err(fail)?;
    }
    write_atomic(&real, text.as_bytes(), None).map_err(fail)
}

// ---- on and off -----------------------------------------------------------------------------

/// Whether everything we add is in place: the skill, the shim and both hooks.
pub(super) fn is_on(env: &Env) -> bool {
    if !skill_file(env).is_file() || !shim_file(env).is_file() {
        return false;
    }
    let Ok(Some(root)) = read_hooks(&hooks_file(env)) else { return false };
    let have = events_with_ours(&root);
    HOOKS.iter().all(|(event, ..)| have.contains(event))
}

/// Writes the skill and the shim, and adds our hooks to `hooks.json` (the file as it was is copied
/// first). A `grade-me` skill that isn't ours is never written over. Safe to run again.
pub(super) fn turn_on(env: &Env) -> Res<()> {
    if !installed(env) {
        return Err(NOT_INSTALLED.into());
    }
    let skill = codex_skill(SKILL_SOURCE).map_err(|e| plain("prepare the Codex skill", &e))?;
    if existing_skill(env)?.is_some_and(|text| !is_our_skill(&text)) {
        return Err(NOT_OURS.into());
    }
    // The hooks file is worked out first: one we can't read stops us before anything is written.
    let path = hooks_file(env);
    let existing = read_hooks(&path)?;
    let mut root = existing.clone().unwrap_or(Json::Obj(Vec::new()));
    let changed = add_hooks(&mut root, env).ok_or(BAD_HOOKS)?;

    let fail = |e: std::io::Error| plain("save the Codex skill", &e);
    std::fs::create_dir_all(skill_dir(env).join("scripts")).map_err(fail)?;
    write_atomic(&skill_file(env), skill.as_bytes(), Some(0o644)).map_err(fail)?;
    // The shim is a shell script: a carriage return after its first line would stop it running.
    write_atomic(&shim_file(env), lf(SHIM_SOURCE).as_bytes(), Some(0o755)).map_err(fail)?;
    if changed {
        save_hooks(&path, &root, existing.is_some())?;
    }
    Ok(())
}

/// After an app update: if what `turn_on` wrote is there (a `grade-me` skill with our mark), rewrites
/// the skill and the shim when they differ from what this app would write now, and the commands of
/// our hooks when theirs differ (see [`update_commands`]; a copy of `hooks.json` is saved first, as
/// for any change to it). Everything is written the way `turn_on` writes it. Nothing is added that
/// isn't there (a hook the person removed stays removed), and nothing that isn't ours is touched: no
/// skill without our mark, no other hook. Returns whether anything was rewritten.
pub(super) fn refresh(env: &Env) -> Res<bool> {
    let Some(have) = existing_skill(env)?.filter(|text| is_our_skill(text)) else { return Ok(false) };
    let skill = codex_skill(SKILL_SOURCE).map_err(|e| plain("prepare the Codex skill", &e))?;
    let fail = |e: std::io::Error| plain("update the Codex skill", &e);
    let mut changed = false;
    if have != skill {
        write_atomic(&skill_file(env), skill.as_bytes(), Some(0o644)).map_err(fail)?;
        changed = true;
    }
    let shim = lf(SHIM_SOURCE);
    if std::fs::read_to_string(shim_file(env)).ok().as_deref() != Some(&*shim) {
        std::fs::create_dir_all(skill_dir(env).join("scripts")).map_err(fail)?;
        write_atomic(&shim_file(env), shim.as_bytes(), Some(0o755)).map_err(fail)?;
        changed = true;
    }
    // Last, so a hooks file we can't read doesn't keep the skill and the shim from being brought up to date.
    let path = hooks_file(env);
    if let Some(mut root) = read_hooks(&path)? {
        if update_commands(&mut root, env).ok_or(BAD_HOOKS)? {
            save_hooks(&path, &root, true)?;
            changed = true;
        }
    }
    Ok(changed)
}

/// Takes out what `turn_on` put in, and nothing else. The report card history stays. Our hooks are
/// taken out of `hooks.json` (the file as it was is copied first); the skill folder is ours only if
/// its skill carries our mark, and only that folder is removed, never `~/.agents` or its `skills`.
pub(super) fn turn_off(env: &Env) -> Res<()> {
    let path = hooks_file(env);
    if let Some(mut root) = read_hooks(&path)? {
        if remove_hooks(&mut root).ok_or(BAD_HOOKS)? {
            save_hooks(&path, &root, true)?;
        }
    }
    if existing_skill(env)?.is_some_and(|text| !is_our_skill(&text)) {
        return Ok(()); // somebody else's grade-me: not ours to remove
    }
    for file in [shim_file(env), skill_file(env)] {
        match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(plain("remove the Codex skill", &e)),
            _ => {}
        }
    }
    // Our folder, if nothing else has moved in (`remove_dir` refuses a folder with files in it).
    let dir = skill_dir(env);
    for folder in [dir.join("scripts"), dir] {
        let _ = std::fs::remove_dir(folder);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_home_moves_the_hooks_but_only_to_an_absolute_path() {
        let home = Path::new("/home/me");
        let var = |v: &str| Some(std::ffi::OsString::from(v));
        assert_eq!(codex_dir_in(None, home), Path::new("/home/me/.codex"));
        let absolute = std::env::temp_dir().join("codex");
        assert_eq!(codex_dir_in(var(&absolute.to_string_lossy()), home), absolute);
        assert_eq!(codex_dir_in(var(""), home), Path::new("/home/me/.codex"), "empty is unset");
        assert_eq!(codex_dir_in(var("codex"), home), Path::new("/home/me/.codex"), "never relative");
    }
}
