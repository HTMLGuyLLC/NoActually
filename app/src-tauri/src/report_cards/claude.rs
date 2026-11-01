//! Claude Code: the plugin is added and removed with Claude Code's own `claude plugin` commands.

use super::plugin_files::{self, Difference};
use super::{for_log, json_in, run, source_for, Env, Ran, Res, REPO};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The plugin, as `claude plugin` names it: `<plugin>@<marketplace>`.
pub(super) const PLUGIN: &str = "grade@noactually";
/// The marketplace file at the repo's root names itself this.
pub(super) const MARKETPLACE: &str = "noactually";

/// Adding a marketplace downloads it; give a slow connection time.
const DOWNLOAD: Duration = Duration::from_secs(120);
const QUICK: Duration = Duration::from_secs(30);

const NOT_INSTALLED: &str = "Claude Code isn't installed.";
const OFFLINE: &str = "Couldn't reach GitHub to download the plugin. Check your connection and try again.";
/// What the person is told when the downloaded plugin isn't the one this app ships.
pub(super) const MISMATCH: &str = "The downloaded plugin didn't match this app version, so it was removed.";
/// What the Claude Code row says when the launch check couldn't move the plugin to this release.
pub(super) const UPDATE_FAILED: &str = "Couldn't update the report card plugin. It will try again next launch.";
/// What the row says when the plugin has files this app didn't put there, and was switched off for it.
pub(super) const EXTRA_FILES: &str =
    "The report card plugin has files this app didn't put there, so it was switched off.";
/// What the row says when the downloaded plugin didn't match and Claude Code wouldn't remove it.
pub(super) const MISMATCH_KEPT: &str =
    "It didn't match, and Claude Code couldn't remove it. Turn report cards off and on again.";
/// What the row says when the plugin's files couldn't be found to check. The plugin is kept.
pub(super) const UNCHECKED: &str = "Couldn't check the plugin. It'll try again when the app next opens.";
/// What the row says when Claude Code wouldn't say what is installed.
pub(super) const CHECK_FAILED: &str = "Couldn't check Claude Code. Try Look again.";

/// Why turning the plugin on didn't work. The launch check tells the downloaded-plugin case from the rest.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Failure {
    /// What Claude Code downloaded isn't the plugin this app ships; it was removed (see [`MISMATCH`]).
    Mismatch,
    /// The same, and Claude Code wouldn't remove it (see [`MISMATCH_KEPT`]).
    Kept,
    /// Anything else, as a sentence for the person.
    Said(String),
}

impl From<String> for Failure {
    fn from(said: String) -> Self {
        Failure::Said(said)
    }
}

impl From<&str> for Failure {
    fn from(said: &str) -> Self {
        Failure::Said(said.into())
    }
}

impl From<Failure> for String {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::Mismatch => MISMATCH.into(),
            Failure::Kept => MISMATCH_KEPT.into(),
            Failure::Said(said) => said,
        }
    }
}

/// Runs `claude <args>`. A program that won't start is a plain sentence; the cause goes to the log.
fn call(exe: &Path, args: &[&str], limit: Duration) -> Res<Ran> {
    run(exe, args, limit).map_err(|e| {
        log::warn!("report cards: couldn't start claude {}: {e}", args.join(" "));
        "Claude Code wouldn't start. Try again.".to_string()
    })
}

/// What git and Node print when there's no connection.
const OFFLINE_WORDS: [&str; 14] = [
    "could not resolve",
    "couldn't resolve",
    "unable to access",
    "failed to connect",
    "connection refused",
    "connection reset",
    "connection timed out",
    "network",
    "enotfound",
    "econnrefused",
    "econnreset",
    "etimedout",
    "getaddrinfo",
    "fetch failed",
];

/// What Claude Code prints when it doesn't know a command or an option we use: it is too old to have
/// `claude plugin` (or one of the flags we pass to it).
const TOO_OLD_WORDS: [&str; 10] = [
    "unknown command",
    "unrecognized command",
    "unrecognised command",
    "invalid command",
    "no such command",
    "not a known command",
    "not a valid command",
    "unknown subcommand",
    "unknown option",
    "unrecognized option",
];

/// What the person is told when `claude` is too old to have plugins.
const TOO_OLD: &str = "Update Claude Code, then try again.";
/// What the person is told when their organization's Claude Code settings block the plugin.
const BLOCKED: &str = "Your organization's Claude Code settings don't allow adding plugins.";

/// Said only when a marketplace is refused for not being on an allow list, so these are enough alone.
const POLICY_ALONE: [&str; 5] = [
    "strictknownmarketplaces",
    "blockedmarketplaces",
    "not in the allowed",
    "not in allowed",
    "not in the list of allowed",
];

/// What a blocked marketplace or plugin is said to be blocked by (managed settings, a policy, an allow list)...
const POLICY_SUBJECTS: [&str; 12] = [
    "policy",
    "policies",
    "managed settings",
    "managed-settings",
    "managed by",
    "enterprise",
    "your organization",
    "your organisation",
    "your admin",
    "administrator",
    "allowlist",
    "allow list",
];
/// ...and how it says so.
const POLICY_DENIALS: [&str; 11] = [
    "blocked",
    "not allowed",
    "isn't allowed",
    "disallowed",
    "disabled by",
    "denied",
    "prohibited",
    "forbidden",
    "restricted",
    "not permitted",
    "isn't permitted",
];

/// Whether `said` (lowercase) is Claude Code refusing a marketplace or plugin because of managed
/// settings or an organization's policy. Phrased many ways, so a policy word and a refusing word
/// anywhere in it are enough (or one of [`POLICY_ALONE`]).
fn blocked_by_policy(said: &str) -> bool {
    POLICY_ALONE.iter().any(|w| said.contains(w))
        || (POLICY_SUBJECTS.iter().any(|w| said.contains(w)) && POLICY_DENIALS.iter().any(|w| said.contains(w)))
}

/// Words for a failed `claude` call, in plain English. The full output goes to the log. A Claude Code
/// that is too old and an organization that blocks plugins each have their own sentence, since the
/// person can do something about them.
pub(super) fn explain(ran: &Ran, what: &str) -> String {
    log::warn!(
        "report cards: claude couldn't {what} (timed out: {}): {}{}",
        ran.timed_out,
        for_log(&ran.out),
        for_log(&ran.err)
    );
    let said = ran.said();
    let offline = OFFLINE_WORDS.iter().any(|w| said.contains(w));
    if ran.timed_out {
        "Claude Code took too long to answer. Try again in a moment.".into()
    } else if TOO_OLD_WORDS.iter().any(|w| said.contains(w)) {
        TOO_OLD.into()
    } else if blocked_by_policy(&said) {
        BLOCKED.into()
    } else if offline {
        OFFLINE.into()
    } else {
        format!("Claude Code couldn't {what}. Try again in a moment.")
    }
}

/// Removing something that's already gone is what was asked for.
fn already_gone(ran: &Ran) -> bool {
    let said = ran.said();
    ["not found", "not installed", "isn't installed", "does not exist", "no marketplace", "not configured"]
        .iter()
        .any(|w| said.contains(w))
}

/// One installed plugin from `claude plugin list --json`.
#[derive(Debug)]
pub(super) struct Installed {
    pub id: String,
    pub enabled: bool,
    /// Where Claude Code keeps the plugin's files, if the list says. This is the only place the
    /// check of the downloaded files looks.
    pub path: Option<PathBuf>,
}

/// The list as Claude Code prints it: an array of objects with an `id` like `grade@noactually`
/// (older builds may split it into `name` and `marketplace`), `enabled` and where its
/// files are (`installPath`). Anything
/// that isn't an object with a name is skipped, so a changed format reads as "nothing installed"
/// instead of an error.
pub(super) fn parse_installed(text: &str) -> Vec<Installed> {
    let Some(json) = json_in(text) else { return Vec::new() };
    let list = match &json {
        Value::Array(items) => items.as_slice(),
        Value::Object(o) => {
            o.get("plugins").or_else(|| o.get("installed")).and_then(Value::as_array).map_or(&[][..], Vec::as_slice)
        }
        _ => &[],
    };
    list.iter()
        .filter_map(|p| {
            let text = |k: &str| p.get(k).and_then(Value::as_str).map(str::to_string);
            let id = text("id").or_else(|| Some(format!("{}@{}", text("name")?, text("marketplace")?)))?;
            Some(Installed {
                id,
                enabled: p.get("enabled").and_then(Value::as_bool).unwrap_or(true),
                path: ["installPath", "install_path", "path"].iter().find_map(|k| text(k)).map(PathBuf::from),
            })
        })
        .collect()
}

/// Our plugin, if Claude Code has it installed.
fn installed_plugin(exe: &Path) -> Res<Option<Installed>> {
    let ran = call(exe, &["plugin", "list", "--json"], QUICK)?;
    if !ran.ok {
        return Err(explain(&ran, "say what's installed"));
    }
    Ok(parse_installed(&ran.out).into_iter().find(|p| p.id == PLUGIN))
}

/// Report cards are on when the plugin is installed and not switched off inside Claude Code, and a
/// sentence if Claude Code wouldn't say. Then the row keeps what Claude Code last recorded (see
/// [`recorded_on`]) instead of showing Off, which would offer a Turn on that installs the plugin again.
pub(super) fn look(env: &Env) -> (bool, Option<&'static str>) {
    let Some(exe) = env.claude.as_deref() else { return (false, None) };
    match installed_plugin(exe) {
        Ok(found) => (found.is_some_and(|p| p.enabled), None),
        Err(e) => {
            log::warn!("report cards: couldn't tell if the plugin is installed: {e}");
            (recorded_on(env), Some(CHECK_FAILED))
        }
    }
}

/// Whether Claude Code's own files say the plugin is installed: `installed_plugins.json` lists it, else (if
/// that file can't be read) a `noactually` marketplace is known. Read from the files, not asked of `claude`.
/// Whether it is switched off isn't there, so a plugin that is off shows as on until the list can be read.
fn recorded_on(env: &Env) -> bool {
    let file = env.home.join(".claude/plugins/installed_plugins.json");
    let json = std::fs::read_to_string(file).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok());
    match json {
        Some(json) => {
            json.get("plugins").and_then(Value::as_object).is_some_and(|plugins| plugins.contains_key(PLUGIN))
        }
        None => known_marketplace(env).is_some(),
    }
}

/// Every marketplace in `claude plugin marketplace list --json`, or `None` if Claude Code wouldn't say.
fn marketplaces(exe: &Path) -> Option<Vec<Value>> {
    let ran = run(exe, &["plugin", "marketplace", "list", "--json"], QUICK).ok().filter(|r| r.ok)?;
    json_in(&ran.out)?.as_array().cloned()
}

/// Our marketplace among them.
fn ours_in(list: &[Value]) -> Option<&Value> {
    list.iter().find(|m| m.get("name").and_then(Value::as_str) == Some(MARKETPLACE))
}

/// The repo and tag a marketplace entry was added from. Wherever Claude Code keeps them (a `repo` and
/// `ref` on the entry or on its `source`, or `repo#ref` in the repo or URL) is read.
fn source_of(m: &Value) -> Option<(String, Option<String>)> {
    let text = |v: Option<&Value>| v.and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
    let nested = m.get("source").filter(|s| s.is_object());
    let get = |k: &str| text(nested.and_then(|n| n.get(k))).or_else(|| text(m.get(k)));
    let from = get("repo").or_else(|| get("url")).or_else(|| text(m.get("source")).filter(|s| s.contains('/')))?;
    let (repo, hashed) = from.split_once('#').map_or((from.as_str(), None), |(r, t)| (r, Some(t.to_string())));
    Some((repo.to_string(), get("ref").or(hashed)))
}

/// The release tag this app's own marketplace was added at: only a marketplace added from exactly
/// `HTMLGuyLLC/NoActually#v<something>`. Another source of `noactually` (the README's unpinned one, or
/// a local folder in development) has none, and is not ours to move.
pub(super) fn pinned_ref(env: &Env, exe: &Path) -> Option<String> {
    let ours = |m: &Value| -> Option<String> {
        let (repo, tag) = source_of(m)?;
        tag.filter(|t| repo == REPO && t.len() > 1 && t.starts_with('v'))
    };
    marketplaces(exe).and_then(|list| ours_in(&list).and_then(ours)).or_else(|| ours(&known_marketplace(env)?))
}

/// The `noactually` entry in Claude Code's own `known_marketplaces.json`, if the file lists one. This
/// is read from the file, not asked of `claude`.
fn known_marketplace(env: &Env) -> Option<Value> {
    let file = env.home.join(".claude/plugins/known_marketplaces.json");
    serde_json::from_str::<Value>(&std::fs::read_to_string(file).ok()?).ok()?.get(MARKETPLACE).cloned()
}

/// Makes `source` the marketplace. One that's there already is removed first: it stays at the version
/// it was added at, so adding again is how the pin moves. Removing a marketplace uninstalls the plugins
/// that came from it, so the plugin has to be installed again afterwards.
fn add_marketplace(exe: &Path, source: &str) -> Res<()> {
    // If the list can't be read, try the removal anyway; "not found" is fine.
    if marketplaces(exe).is_none_or(|list| ours_in(&list).is_some()) {
        let gone = call(exe, &["plugin", "marketplace", "remove", MARKETPLACE], QUICK)?;
        if !gone.ok && !already_gone(&gone) {
            return Err(explain(&gone, "update the plugin"));
        }
    }
    let added = call(exe, &["plugin", "marketplace", "add", source], DOWNLOAD)?;
    if added.ok {
        Ok(())
    } else {
        Err(explain(&added, "add the plugin"))
    }
}

/// Installs the plugin from the marketplace that's there. `-y` is safe: the person asked for it.
fn install_plugin(exe: &Path) -> Res<()> {
    let installed = call(exe, &["plugin", "install", PLUGIN, "--scope", "user", "-y"], DOWNLOAD)?;
    if installed.ok || installed.said().contains("already installed") {
        Ok(())
    } else {
        Err(explain(&installed, "install the plugin"))
    }
}

/// Switches the plugin off in Claude Code, as the person had it before it was installed again (a new
/// install is always on).
fn disable(exe: &Path) -> Res<()> {
    let ran = call(exe, &["plugin", "disable", PLUGIN, "--scope", "user"], QUICK)?;
    if ran.ok || ran.said().contains("already disabled") {
        Ok(())
    } else {
        Err(explain(&ran, "switch the plugin off again"))
    }
}

/// Adds the marketplace at `source` and installs the plugin from it. From a release (a source like
/// `HTMLGuyLLC/NoActually#v0.4.0`) the downloaded files are then compared with the ones in this app.
pub(super) fn turn_on(env: &Env, source: &str) -> Res<()> {
    install(env, source).map_err(String::from)?;
    Pending::clear(env);
    Ok(())
}

/// [`turn_on`], saying whether a failure was the downloaded plugin not matching.
fn install(env: &Env, source: &str) -> Result<(), Failure> {
    let exe = env.claude.as_deref().ok_or(NOT_INSTALLED)?;
    add_marketplace(exe, source)?;
    install_plugin(exe)?;
    // A fresh install settles what an earlier check found; the check of this one says its own.
    forget_problem(env);
    if source.starts_with(&format!("{REPO}#v")) {
        let plugin = installed_plugin(exe).unwrap_or_else(|e| {
            log::warn!("report cards: couldn't list the plugin to check it: {e}");
            None
        });
        match verdict(env, exe, plugin.as_ref()) {
            Verdict::Same => {}
            Verdict::Unchecked => note_problem(env, UNCHECKED),
            // The person asked for the plugin to be on, so a download with anything extra doesn't stay.
            Verdict::Extra(_) | Verdict::Different => return Err(removed(exe)),
        }
    }
    Ok(())
}

/// What a download that doesn't match comes to once it is removed: gone, or still there.
fn removed(exe: &Path) -> Failure {
    if remove(exe) {
        Failure::Mismatch
    } else {
        Failure::Kept
    }
}

// ---- is it the plugin this app ships? --------------------------------------------------------

/// Where the installed plugin's files are: the folder the list names (`installPath`) and no other.
/// Claude Code's cache layout (a folder per version, which one is newest) is its own to change, and a
/// guess could check a different copy than the one that runs, passing a bad plugin or removing a good one.
fn plugin_dir(plugin: Option<&Installed>) -> Option<PathBuf> {
    plugin.and_then(|p| p.path.clone()).filter(|d| d.is_dir())
}

/// How the downloaded plugin compares with the one built into this app. The variants run from best to
/// worst, so the worse of two looks is the larger.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Verdict {
    /// It is the one this app ships.
    Same,
    /// Its files, or its marketplace file, couldn't be found to check. It is kept, and the person is told.
    Unchecked,
    /// Every shipped file is the same and there are files this app doesn't ship (the sentence says which).
    Extra(String),
    /// A shipped file or the marketplace entry is not this app's, or something is a link or can't be read.
    Different,
}

/// The plugin's files and its marketplace entry both have to be this app's. Only a plugin that is there
/// and different counts as not matching, so a list that leaves out where the plugin is can't take a
/// working plugin away; that is [`Verdict::Unchecked`], which is logged.
fn verdict(env: &Env, exe: &Path, plugin: Option<&Installed>) -> Verdict {
    let files = files_verdict(plugin);
    if files == Verdict::Different {
        return files;
    }
    files.max(marketplace_verdict(env, exe))
}

/// The plugin's files against the ones this app ships (see [`plugin_files::compare`]).
fn files_verdict(plugin: Option<&Installed>) -> Verdict {
    let Some(dir) = plugin_dir(plugin) else {
        log::warn!("report cards: the plugin list doesn't say where the plugin's files are, so they weren't checked");
        return Verdict::Unchecked;
    };
    match plugin_files::compare(&dir) {
        Some(Difference::Changed(why)) => {
            log::warn!("report cards: the downloaded plugin isn't this app's: {why}");
            Verdict::Different
        }
        Some(Difference::Extra(why)) => {
            log::warn!("report cards: the downloaded plugin has more than this app's: {why}");
            Verdict::Extra(why)
        }
        None => Verdict::Same,
    }
}

/// The marketplace file inside a marketplace's folder.
const MARKETPLACE_FILE: &str = ".claude-plugin/marketplace.json";
/// What the marketplace file calls the plugin, and where it says the plugin is (the folder in this repo).
const PLUGIN_NAME: &str = "grade";
const PLUGIN_SOURCE: &str = "./plugins/grade";
/// Ours is a few hundred bytes; a file much bigger than that is not it.
const MARKETPLACE_FILE_LIMIT: u64 = 64 * 1024;

/// Whether the entry for the plugin in `manifest` (a marketplace file) is the one in this app's own: a
/// single `grade` entry with a name, a source that is exactly [`PLUGIN_SOURCE`], a description and
/// maybe a version, and no other key. A marketplace entry can carry a plugin's hooks, servers and
/// commands itself, so anything more is a way to run something the plugin's files don't show.
pub(super) fn entry_is_ours(manifest: &Value) -> bool {
    let Some(plugins) = manifest.get("plugins").and_then(Value::as_array) else { return false };
    let mut ours = plugins.iter().filter(|p| p.get("name").and_then(Value::as_str) == Some(PLUGIN_NAME));
    let (Some(entry), None) = (ours.next(), ours.next()) else { return false };
    let Some(entry) = entry.as_object() else { return false };
    let text = |key: &str| entry.get(key).is_some_and(Value::is_string);
    entry.keys().all(|k| ["name", "source", "description", "version"].contains(&k.as_str()))
        && entry.get("source").and_then(Value::as_str) == Some(PLUGIN_SOURCE)
        && text("description")
        && (!entry.contains_key("version") || text("version"))
}

/// The folder Claude Code keeps our marketplace's files in: where `list` (the output of `claude plugin
/// marketplace list --json`) says (`installLocation`), else where Claude Code puts a marketplace by name.
fn marketplace_dir(env: &Env, list: Option<&[Value]>) -> PathBuf {
    let said = list.and_then(ours_in).and_then(|m| {
        ["installLocation", "install_location"]
            .iter()
            .find_map(|k| m.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()))
    });
    said.map_or_else(|| env.home.join(".claude/plugins/marketplaces").join(MARKETPLACE), PathBuf::from)
}

/// Whether the marketplace file Claude Code downloaded lists the plugin the way this app's does (see
/// [`entry_is_ours`]). A file that is there and isn't ours, or can't be read, or isn't a plain file,
/// is [`Verdict::Different`]. There being no file to find is [`Verdict::Unchecked`], which is logged: only
/// a marketplace that is there and different counts, so a different layout of Claude Code's folders can't
/// take a working plugin away.
fn marketplace_verdict(env: &Env, exe: &Path) -> Verdict {
    let list = marketplaces(exe);
    let file = marketplace_dir(env, list.as_deref()).join(MARKETPLACE_FILE);
    let meta = match std::fs::symlink_metadata(&file) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            log::warn!(
                "report cards: couldn't find the marketplace file at {}, so it wasn't checked",
                crate::home_short(&file)
            );
            return Verdict::Unchecked;
        }
        Err(e) => {
            log::warn!("report cards: couldn't look at the marketplace file {}: {e}", crate::home_short(&file));
            return Verdict::Different;
        }
    };
    if !meta.is_file() || meta.len() > MARKETPLACE_FILE_LIMIT {
        log::warn!(
            "report cards: the downloaded marketplace file {} isn't a small plain file",
            crate::home_short(&file)
        );
        return Verdict::Different;
    }
    let manifest = std::fs::read_to_string(&file).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok());
    if manifest.as_ref().is_some_and(entry_is_ours) {
        Verdict::Same
    } else {
        log::warn!(
            "report cards: the downloaded marketplace file {} doesn't list this app's plugin",
            crate::home_short(&file)
        );
        Verdict::Different
    }
}

/// Uninstalls the plugin and drops the marketplace: a clean-up, so a failure is logged and not passed
/// on. Says whether it worked, since "it was removed" is not for the person to be told if it wasn't.
fn remove(exe: &Path) -> bool {
    match remove_all(exe) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("report cards: couldn't remove the plugin that didn't match: {e}");
            false
        }
    }
}

fn remove_all(exe: &Path) -> Res<()> {
    let steps: [&[&str]; 2] =
        [&["plugin", "uninstall", PLUGIN, "-y"], &["plugin", "marketplace", "remove", MARKETPLACE]];
    for args in steps {
        let ran = call(exe, args, QUICK)?;
        if !ran.ok && !already_gone(&ran) {
            return Err(explain(&ran, "remove the plugin"));
        }
    }
    Ok(())
}

/// Uninstalls the plugin and drops the marketplace. Either may already be gone. The person has
/// decided: a move that was waiting to be finished is forgotten.
pub(super) fn turn_off(env: &Env) -> Res<()> {
    remove_all(env.claude.as_deref().ok_or(NOT_INSTALLED)?)?;
    Pending::clear(env);
    forget_problem(env);
    Ok(())
}

// ---- moving to a new release -------------------------------------------------------------------

/// A move to this release that didn't finish, kept in a file beside the database. Removing a marketplace
/// uninstalls its plugin, so a move that fails half way can leave the person with no plugin and no
/// marketplace, and nothing for the next launch to find. This file is how the next launch knows the
/// plugin is wanted and tries again. It is written before a move starts, so even a quit or a crash in
/// the middle of one is tried again. It stays until the new release is installed and checked, or the
/// person turns report cards on or off; while it does, Settings says the update failed.
#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Pending {
    /// The release tag the marketplace was pinned to before the move (`v0.4.0`): what to put back.
    pin: String,
    /// The plugin was switched off in Claude Code, so it has to be switched off again.
    disabled: bool,
    /// The move failed and the old tag and plugin were put back (see [`restore`]). A plugin that is
    /// missing after that was removed by the person, not by the move, and is not theirs to get back.
    restored: bool,
}

impl Pending {
    /// What is waiting. A file there that doesn't read is still a move waiting, with nothing to put back.
    fn read(env: &Env) -> Option<Pending> {
        let text = std::fs::read_to_string(env.repin_file()).ok()?;
        Some(serde_json::from_str(&text).unwrap_or_default())
    }

    /// Whether it was written.
    fn save(&self, env: &Env) -> bool {
        let written = serde_json::to_vec(self).map_err(std::io::Error::other).and_then(|bytes| {
            std::fs::create_dir_all(&env.data)?;
            store::write_atomic(&env.repin_file(), &bytes, Some(0o644))
        });
        if let Err(e) = &written {
            log::warn!("report cards: couldn't note the unfinished move: {e}");
        }
        written.is_ok()
    }

    pub(super) fn clear(env: &Env) {
        if let Err(e) = std::fs::remove_file(env.repin_file()) {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!("report cards: couldn't clear the note about the unfinished move: {e}");
            }
        }
    }
}

/// Keeps what the plugin check found wrong, in a file beside the database, so Settings still says it
/// after a restart. `text` is one of the sentences above; it is written the way [`Pending`] is.
fn note_problem(env: &Env, text: &str) {
    let written = std::fs::create_dir_all(&env.data)
        .and_then(|()| store::write_atomic(&env.problem_file(), text.as_bytes(), Some(0o644)));
    if let Err(e) = written {
        log::warn!("report cards: couldn't note what the plugin check found: {e}");
    }
}

/// What an earlier plugin check found wrong, if the file beside the database says. Only a sentence
/// this module writes is passed on, whatever else is in the file.
pub(super) fn saved_problem(env: &Env) -> Option<&'static str> {
    let text = std::fs::read_to_string(env.problem_file()).ok()?;
    [MISMATCH, MISMATCH_KEPT, EXTRA_FILES, UNCHECKED].into_iter().find(|known| *known == text.trim())
}

/// A clean check, or a fresh install or removal, ends what was noted.
fn forget_problem(env: &Env) {
    if let Err(e) = std::fs::remove_file(env.problem_file()) {
        if e.kind() != std::io::ErrorKind::NotFound {
            log::warn!("report cards: couldn't clear the note about the plugin check: {e}");
        }
    }
}

/// What the launch check found.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Launch {
    /// Nothing to do, or nothing of ours to do it to.
    Fine,
    /// The plugin was moved to this app's release.
    Moved,
    /// Something to tell the person (shown on the Claude Code row).
    Problem(&'static str),
}

/// Whether a `noactually` marketplace is there that isn't pinned to one of our release tags, so isn't ours to move.
fn other_marketplace(env: &Env, exe: &Path) -> bool {
    pinned_ref(env, exe).is_none()
        && (marketplaces(exe).is_some_and(|list| ours_in(&list).is_some()) || known_marketplace(env).is_some())
}

/// Puts back what a failed move took away: the old tag, and the plugin (removing the marketplace
/// uninstalled it), switched off again if it was.
fn restore(exe: &Path, tag: &str, disabled: bool) -> Res<()> {
    if tag.is_empty() {
        return Err("there is no old tag to put back".into());
    }
    add_marketplace(exe, &format!("{REPO}#{tag}"))?;
    install_plugin(exe)?;
    if disabled {
        disable(exe)?;
    }
    Ok(())
}

/// Whether the plugin has to be switched off after a move: the state it is in now, which is the
/// person's, unless an earlier move left it on while it should be off. A plugin that isn't there
/// (a move took it) is as the note says.
fn switched_off(plugin: Option<&Installed>, waiting: Option<&Pending>) -> bool {
    match plugin {
        Some(p) => !p.enabled || waiting.is_some_and(|w| w.disabled && !w.restored),
        None => waiting.is_some_and(|w| w.disabled),
    }
}

/// The plugin is at the app's version: checks it, and finishes what a move left undone.
fn settle(env: &Env, exe: &Path, plugin: Option<&Installed>, pending: Option<&Pending>, disabled: bool) -> Launch {
    let enabled = plugin.is_some_and(|p| p.enabled);
    let unchecked = match verdict(env, exe, plugin) {
        Verdict::Different => return gone(env, exe),
        Verdict::Extra(_) => {
            // Every shipped file is ours, so the plugin isn't removed for what Claude Code may have
            // added to its folder. It must not run with them, though: it is switched off, and stays so.
            if enabled {
                if let Err(e) = disable(exe) {
                    log::warn!("report cards: couldn't switch off the plugin that has extra files: {e}");
                    return gone(env, exe);
                }
            }
            Pending::clear(env);
            return found(env, EXTRA_FILES);
        }
        Verdict::Unchecked => true,
        Verdict::Same => {
            forget_problem(env);
            false
        }
    };
    // A move that got this far and stopped on switching the plugin off again.
    if pending.is_some() {
        if disabled && enabled {
            if let Err(e) = disable(exe) {
                log::warn!("report cards: couldn't switch the plugin off again: {e}");
                return Launch::Problem(UPDATE_FAILED);
            }
        }
        Pending::clear(env);
    }
    if unchecked {
        found(env, UNCHECKED)
    } else {
        Launch::Fine
    }
}

/// At launch: if the plugin is installed from this app's own pinned marketplace but not at this app's
/// release, adds it again at the new tag; and checks that the files Claude Code downloaded are the
/// ones this app ships. A `noactually` marketplace from any other source (the README's, or a local
/// folder) is never touched. Does nothing when Claude Code or the plugin isn't there, unless an
/// earlier move was left unfinished ([`Pending`]): then the plugin may be missing because of it, and
/// the move is tried again. (Unless that move put the plugin back and it has gone since: the person
/// removed it.) And does nothing, asking `claude` nothing at all, unless the person has turned report
/// cards on: that is a `noactually` marketplace in Claude Code's `known_marketplaces.json` (a file we
/// only read), or a move waiting.
pub(super) fn launch_check(env: &Env, version: &str) -> Launch {
    let Some(exe) = env.claude.as_deref() else { return Launch::Fine };
    let pending = Pending::read(env);
    if pending.is_none() && known_marketplace(env).is_none() {
        return Launch::Fine;
    }
    let plugin = match installed_plugin(exe) {
        Ok(plugin) => plugin,
        Err(e) => {
            log::warn!("report cards: couldn't tell if the plugin is installed: {e}");
            return Launch::Fine;
        }
    };
    if plugin.is_none() {
        match &pending {
            None => return Launch::Fine,
            Some(waiting) if waiting.restored => {
                Pending::clear(env);
                return Launch::Fine;
            }
            Some(_) => {}
        }
    }
    // The tag the marketplace is at, else the one a move left behind. A marketplace that isn't ours
    // ends it: the person has set up something else under our name, and there is nothing of ours to finish.
    let from = match (pinned_ref(env, exe), &pending) {
        (Some(pin), _) => pin,
        (None, Some(waiting)) if !other_marketplace(env, exe) => waiting.pin.clone(),
        (None, waiting) => {
            if waiting.is_some() {
                Pending::clear(env);
            }
            return Launch::Fine;
        }
    };
    let disabled = switched_off(plugin.as_ref(), pending.as_ref());
    if from == format!("v{version}") && plugin.is_some() {
        return settle(env, exe, plugin.as_ref(), pending.as_ref(), disabled);
    }
    let waiting = Pending { pin: from, disabled, restored: false };
    // Nothing is removed until the new tag is known to be there: an offline launch, or a tag that can't
    // be had, would otherwise take the working plugin away for good. Nothing has been taken, so a note
    // written now says the old plugin is in place (and no hourly retry is needed: see [`retry_pending`]).
    // A note that is there already stays as it is.
    if !tag_is_reachable(env, version) {
        if pending.is_none() {
            Pending { restored: true, ..waiting }.save(env);
        }
        return Launch::Problem(UPDATE_FAILED);
    }
    // Written before the marketplace is removed, so a quit or a crash from here on still gets another
    // try next time. If it can't be written there would be no way back, so the move is not started.
    if !waiting.save(env) {
        return Launch::Problem(UPDATE_FAILED);
    }
    match install(env, &source_for(false, version, None)) {
        Ok(()) => {
            if disabled {
                if let Err(e) = disable(exe) {
                    log::warn!("report cards: moved the plugin, but couldn't switch it off again: {e}");
                    return Launch::Problem(UPDATE_FAILED);
                }
            }
            // Only now, with the new release installed and checked, is the move over.
            Pending::clear(env);
            Launch::Moved
        }
        Err(Failure::Mismatch) => {
            Pending::clear(env);
            found(env, MISMATCH)
        }
        Err(Failure::Kept) => {
            Pending::clear(env);
            found(env, MISMATCH_KEPT)
        }
        Err(Failure::Said(e)) => {
            log::warn!("report cards: couldn't move the plugin to v{version}: {e}");
            match restore(exe, &waiting.pin, disabled) {
                Ok(()) => {
                    Pending { restored: true, ..waiting }.save(env);
                }
                Err(back) => log::warn!("report cards: couldn't put the plugin back at {}: {back}", waiting.pin),
            }
            Launch::Problem(UPDATE_FAILED)
        }
    }
}

/// A move waiting to be tried again, for the app to do while it stays open. The launch check, but only
/// when [`Pending`] says a move is waiting and the plugin is gone because of it; `None` when it
/// doesn't, which costs one look for a file. A move that failed with the old plugin put back (or never
/// taken) waits for the next launch: trying it every hour would take the working plugin away and put it
/// back each time, for as long as whatever stopped it lasts.
pub(super) fn retry_pending(env: &Env, version: &str) -> Option<Launch> {
    Pending::read(env).filter(|waiting| !waiting.restored).map(|_| launch_check(env, version))
}

/// Whether the tag for `version` is there to be had, looked for read-only with `git ls-remote` (through
/// [`run`], with a time limit) before the plugin is touched. An empty answer, a failure and a timeout all
/// mean it can't be had. A computer with no git skips the look, so a move isn't held back by it.
fn tag_is_reachable(env: &Env, version: &str) -> bool {
    let Some(git) = env.git.as_deref() else {
        log::info!("report cards: git wasn't found, so the new tag wasn't looked for before the move");
        return true;
    };
    let tag = format!("refs/tags/v{version}");
    match run(git, &["ls-remote", &format!("https://github.com/{REPO}.git"), &tag], QUICK) {
        Ok(ran) if ran.ok && ran.out.lines().any(|line| line.split_whitespace().nth(1) == Some(tag.as_str())) => true,
        Ok(ran) => {
            log::warn!(
                "report cards: couldn't find {tag} (timed out: {}): {}{}",
                ran.timed_out,
                for_log(&ran.out),
                for_log(&ran.err)
            );
            false
        }
        Err(e) => {
            log::warn!("report cards: git wouldn't start, so the new tag wasn't looked for before the move: {e}");
            true
        }
    }
}

/// A problem with the downloaded plugin, kept for Settings (see [`note_problem`]) and passed on.
fn found(env: &Env, text: &'static str) -> Launch {
    note_problem(env, text);
    Launch::Problem(text)
}

/// The downloaded plugin doesn't match: removed, and the person is told, or told it couldn't be. The
/// move (if there was one) is over; a plugin that is still there is looked at again next launch.
fn gone(env: &Env, exe: &Path) -> Launch {
    Pending::clear(env);
    found(env, if remove(exe) { MISMATCH } else { MISMATCH_KEPT })
}
