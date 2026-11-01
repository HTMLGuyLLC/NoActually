//! `noactually`: the command-line scorecard. Same engine, sources and database as the desktop
//! app, so either one can allow a source and both see the same numbers.
//!
//! Arguments are parsed by hand ([`parse_args`]): eleven commands and a handful of flags don't
//! justify a parser dependency, and the help text stays exactly as written.
//!
//! `digest` and `hook` are not for people. The report-card plugin and the Codex hooks call them
//! (see [`helper`]); they are left out of the help text.

mod helper;
mod report_cards;
mod stderr_log;

use helper::{is_helper, is_helper_command};
pub use helper::{QUIET_FILE, TIPS_FILE, TIPS_OFF_FILE};

use anyhow::{anyhow, bail, Context, Result};
use chrono::{Local, NaiveDate};
use std::ffi::OsString;
use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use store::{Filter, Level, Retention, Store};

const HELP: &str = "noactually: could AI replace you? Score how you work with your AI

USAGE
  noactually sources [--json]            List sources, where they read from, and whether they're allowed
  noactually allow <source> [--since YYYY-MM-DD | --from-now]
  noactually pause <source>              Stop reading for now (keeps consent and scores)
  noactually resume <source>             Start reading again after a pause (never allows a source)
  noactually revoke <source>             Stop reading (keeps scores)
  noactually forget <source> [--yes]     Stop reading and delete what it produced. Finished goal weeks
                                         and trophies stay, as totals. Asks first; --yes skips the question
  noactually import <source> <path>      Import an export file or folder (chatgpt-export, claude-ai-export)
  noactually sync [--json]               Read anything new from allowed sources. Exits 1 if any file was skipped
  noactually stats [filters] [--json]    Scorecard. Filters: --source --provider --family --version --since
  noactually stats --report-cards [filters] [--json]
                                         Report cards instead: graded tasks, % A or B, extra rounds, cost and minutes,
                                         the habit left out most, and the last 5 cards. Same filters
  noactually check <prompt> [--json]     What a draft prompt includes, graded like your history (nothing is saved)
  noactually retention <full|snippet|category> [--full-days N] [--snippet-days N] [--yes]
                                         Keeping less deletes older text. Asks first; --yes skips the question

  noactually --help | --version          This text, or the version. `<command> --help` prints this text too

Data lives in NOACTUALLY_DB, or app.noactually/noactually.db in your app data folder (shared with the desktop app).
Nothing is read until you allow a source. Nothing leaves this machine.";

/// What a command line asks for.
#[derive(Debug, PartialEq)]
enum Parsed {
    Help,
    Version,
    Run(Command),
}

/// A command that does something.
#[derive(Debug, PartialEq)]
enum Command {
    Sources { json: bool },
    Allow { id: String, since: Option<NaiveDate>, from_now: bool },
    Pause { id: String },
    Resume { id: String },
    Revoke { id: String },
    Forget { id: String, yes: bool },
    Import { id: String, path: PathBuf },
    Sync { json: bool },
    Stats { filter: Filter, json: bool, report_cards: bool },
    Check { text: String, json: bool },
    Retention { level: Level, full_days: Option<u32>, snippet_days: Option<u32>, yes: bool },
}

/// The arguments after the command, split into positionals and `--flag [value]` pairs.
struct Args<'a> {
    positional: Vec<&'a str>,
    flags: Vec<(&'a str, Option<&'a str>)>,
}

impl<'a> Args<'a> {
    /// `with_value` lists the flags that take a value; every other flag is a switch.
    fn split(args: &'a [String], with_value: &[&str], switches: &[&str]) -> Result<Self> {
        let mut out = Args { positional: Vec::new(), flags: Vec::new() };
        let mut it = args.iter().map(String::as_str);
        while let Some(a) = it.next() {
            if with_value.contains(&a) {
                let v = it.next().with_context(|| format!("{a} needs a value"))?;
                out.flags.push((a, Some(v)));
            } else if switches.contains(&a) {
                out.flags.push((a, None));
            } else if a.starts_with("--") {
                bail!("unknown option '{a}'\n\n{HELP}");
            } else {
                out.positional.push(a);
            }
        }
        Ok(out)
    }

    fn arg(&self, i: usize, what: &str) -> Result<String> {
        self.positional.get(i).map(|s| (*s).to_string()).with_context(|| format!("missing {what}\n\n{HELP}"))
    }

    fn value(&self, name: &str) -> Option<String> {
        self.flags.iter().rev().find(|(n, _)| *n == name).and_then(|(_, v)| v.map(str::to_string))
    }

    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|(n, _)| *n == name)
    }

    fn number(&self, name: &str) -> Result<Option<u32>> {
        self.value(name).map(|v| v.parse().with_context(|| format!("{name} wants a whole number of days"))).transpose()
    }
}

/// `a`, unless it has a word that is no option: `sync now` should say so, not sync.
fn no_positional(a: Args<'_>) -> Result<Args<'_>> {
    match a.positional.first() {
        Some(word) => bail!("unexpected '{word}'\n\n{HELP}"),
        None => Ok(a),
    }
}

/// Parses the arguments after the program name.
fn parse_args(args: &[String]) -> Result<Parsed> {
    let Some((cmd, rest)) = args.split_first() else { return Ok(Parsed::Help) };
    // Asking for help must never run anything. `check` takes the whole rest as its prompt, so only
    // its first word can ask.
    let asks_help = |a: &String| a == "--help" || a == "-h";
    if matches!(cmd.as_str(), "--help" | "-h" | "help") || (cmd.as_str() != "check" && rest.iter().any(asks_help)) {
        return Ok(Parsed::Help);
    }
    if matches!(cmd.as_str(), "--version" | "-V" | "version") {
        return Ok(Parsed::Version);
    }
    Ok(Parsed::Run(match cmd.as_str() {
        "sources" => Command::Sources { json: no_positional(Args::split(rest, &[], &["--json"])?)?.has("--json") },
        "sync" => Command::Sync { json: no_positional(Args::split(rest, &[], &["--json"])?)?.has("--json") },
        "allow" => {
            let a = Args::split(rest, &["--since"], &["--from-now"])?;
            let since =
                a.value("--since").map(|d| d.parse::<NaiveDate>()).transpose().context("--since wants YYYY-MM-DD")?;
            let from_now = a.has("--from-now");
            if since.is_some() && from_now {
                bail!("use --since or --from-now, not both");
            }
            Command::Allow { id: a.arg(0, "source")?, since, from_now }
        }
        "pause" => Command::Pause { id: Args::split(rest, &[], &[])?.arg(0, "source")? },
        "resume" => Command::Resume { id: Args::split(rest, &[], &[])?.arg(0, "source")? },
        "revoke" => Command::Revoke { id: Args::split(rest, &[], &[])?.arg(0, "source")? },
        "forget" => {
            let a = Args::split(rest, &[], &["--yes", "-y"])?;
            Command::Forget { id: a.arg(0, "source")?, yes: a.has("--yes") || a.has("-y") }
        }
        "import" => {
            let a = Args::split(rest, &[], &[])?;
            Command::Import { id: a.arg(0, "source")?, path: a.arg(1, "path")?.into() }
        }
        "stats" => {
            let a = Args::split(
                rest,
                &["--source", "--provider", "--family", "--version", "--since"],
                &["--json", "--report-cards"],
            )?;
            let since =
                a.value("--since").map(|d| d.parse::<NaiveDate>()).transpose().context("--since wants YYYY-MM-DD")?;
            let filter = Filter {
                source: a.value("--source"),
                provider: a.value("--provider"),
                family: a.value("--family"),
                version: a.value("--version"),
                since,
            };
            Command::Stats { filter, json: a.has("--json"), report_cards: a.has("--report-cards") }
        }
        // The whole rest is the prompt, so it's never read as flags, except a last `--json`.
        "check" if rest.last().is_some_and(|a| a == "--json") && rest.len() > 1 => {
            Command::Check { text: rest[..rest.len() - 1].join(" "), json: true }
        }
        "check" if !rest.is_empty() => Command::Check { text: rest.join(" "), json: false },
        "check" => bail!("missing prompt: noactually check \"Fix the crash in checkout.ts…\""),
        "retention" => {
            let a = Args::split(rest, &["--full-days", "--snippet-days"], &["--yes", "-y"])?;
            Command::Retention {
                level: a.arg(0, "level")?.parse()?,
                full_days: a.number("--full-days")?,
                snippet_days: a.number("--snippet-days")?,
                yes: a.has("--yes") || a.has("-y"),
            }
        }
        other => bail!("unknown command '{other}'\n\n{HELP}"),
    }))
}

/// The helper command in `args` (everything after the program name), as text, or `None` when the
/// first argument isn't `hook` or `digest`. The desktop app asks this before anything else, so it
/// looks at the first argument alone: an argument that isn't valid text (a path, say) must never
/// stop the window from opening. In a helper command it becomes lossy text and finds nothing.
pub fn helper_args(args: &[OsString]) -> Option<Vec<String>> {
    let first = args.first()?.to_str()?;
    is_helper_command(first).then(|| args.iter().map(|a| a.to_string_lossy().into_owned()).collect())
}

/// [`run`] for the real command line. A word that isn't valid text is an error, not a panic.
pub fn run_os(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    let args: Vec<OsString> = args.into_iter().collect();
    if let Some(helper) = helper_args(&args) {
        return run(helper);
    }
    match args.into_iter().map(OsString::into_string).collect::<Result<Vec<String>, OsString>>() {
        Ok(args) => run(args),
        Err(bad) => {
            eprintln!("Error: {} isn't valid text. Command line words must be UTF-8.", bad.display());
            ExitCode::FAILURE
        }
    }
}

/// Runs the command line `args` (everything after the program name) and says how it ended.
///
/// The helper commands print nothing and succeed whatever happens. Every other command reports
/// its error on stderr and fails.
pub fn run(args: impl IntoIterator<Item = String>) -> ExitCode {
    let args: Vec<String> = args.into_iter().collect();
    if is_helper(&args) {
        return helper::main(&args);
    }
    stderr_log::init();
    match command(&args) {
        Ok(true) => ExitCode::SUCCESS,
        // The command ran but part of it didn't (a sync that skipped files): it said so already.
        Ok(false) => ExitCode::FAILURE,
        // What returning an error from `main` prints.
        Err(e) => {
            eprintln!("Error: {e:?}");
            ExitCode::FAILURE
        }
    }
}

/// Parses and runs `args`. `Ok(false)` means it ran but only in part.
fn command(args: &[String]) -> Result<bool> {
    let cmd = match parse_args(args)? {
        Parsed::Help => {
            println!("{HELP}");
            return Ok(true);
        }
        Parsed::Version => {
            println!("noactually {}", env!("CARGO_PKG_VERSION"));
            return Ok(true);
        }
        Parsed::Run(cmd) => cmd,
    };
    if store::legacy_db_env_in_use() {
        eprintln!("note: {} is deprecated; set {} instead.", store::LEGACY_DB_ENV, store::DB_ENV);
    }
    let mut store = Store::open(&store::default_db_path()?)?;
    execute(cmd, &mut store, &sources::all())
}

/// Deletes a source, once the person has confirmed it.
fn forget(store: &mut Store, all: &[Box<dyn sources::Source>], id: &str, yes: bool) -> Result<()> {
    known(all, id)?;
    let stdin = std::io::stdin();
    if !confirm_forget(id, yes, stdin.is_terminal(), &mut stdin.lock())? {
        println!("Cancelled. Nothing was deleted.");
        return Ok(());
    }
    store.forget(id)?;
    println!("Deleted what this source produced. Finished goal weeks and trophies stay, as totals.");
    Ok(())
}

/// Saves new retention settings. Keeping less deletes older text for good, so that needs a yes.
fn set_retention(store: &Store, r: Retention, yes: bool) -> Result<()> {
    r.validate()?;
    let trims = r.keeps_less_than(&store.retention()?);
    if trims {
        let stdin = std::io::stdin();
        if !confirm_retention(yes, stdin.is_terminal(), &mut stdin.lock())? {
            println!("Cancelled. Nothing was changed.");
            return Ok(());
        }
    }
    store.set_retention(r)?;
    println!("Keeping {:?} text; full text {} days, snippets {} days.", r.level, r.full_days, r.snippet_days);
    if trims {
        println!("Deleted any older text past these limits. Pinned corrections keep theirs.");
    }
    Ok(())
}

/// True when the person agreed to delete what `id` produced.
fn confirm_forget(id: &str, yes: bool, interactive: bool, input: &mut impl BufRead) -> Result<bool> {
    let question = format!(
        "Delete what {id} produced, including its scores? Finished goal weeks and trophies stay, as totals. \
         This can't be undone."
    );
    confirm(&question, &format!("delete what {id} produced"), yes, interactive, input)
}

/// True when the person agreed to delete older text.
fn confirm_retention(yes: bool, interactive: bool, input: &mut impl BufRead) -> Result<bool> {
    let question = "Keep less text? Older text past the new limits is deleted, except on pinned corrections. \
                    This can't be undone.";
    confirm(question, "delete older text", yes, interactive, input)
}

/// True on `--yes`, or a "y" at the prompt. With neither (input isn't a terminal and no `--yes`) it
/// refuses to `what` rather than guess.
fn confirm(question: &str, what: &str, yes: bool, interactive: bool, input: &mut impl BufRead) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    if !interactive {
        bail!("refusing to {what} without confirmation. Run it in a terminal, or add --yes.");
    }
    eprint!("{question} [y/N] ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    input.read_line(&mut line).context("couldn't read your answer")?;
    Ok(matches!(line.trim().to_lowercase().as_str(), "y" | "yes"))
}

/// Runs one command. `Ok(false)` means it ran but only in part: a sync that had to skip files.
fn execute(cmd: Command, store: &mut Store, all: &[Box<dyn sources::Source>]) -> Result<bool> {
    let mut complete = true;
    match cmd {
        Command::Sources { json } => print_sources(store, all, json)?,
        Command::Allow { id, since, from_now } => {
            let src = known(all, &id)?;
            store.allow(&id, since)?;
            if from_now {
                let n = store.start_from_now(src)?;
                println!("Allowed {id} from now on ({n} existing files skipped).");
            } else {
                println!("Allowed {id}. Run `noactually sync` to read it.");
            }
        }
        Command::Pause { id } => {
            known(all, &id)?;
            allowed_only(store, &id, "pause")?;
            store.set_paused(&id, true)?;
            println!("Paused {id}. Nothing is read until `noactually resume {id}`. Scores so far are kept.");
        }
        Command::Resume { id } => {
            known(all, &id)?;
            allowed_only(store, &id, "resume")?;
            store.set_paused(&id, false)?;
            println!("Resumed {id}. Run `noactually sync` to read what came in meanwhile.");
        }
        Command::Revoke { id } => {
            known(all, &id)?;
            store.revoke(&id)?;
            println!("Stopped reading. Scores so far are kept.");
        }
        Command::Forget { id, yes } => forget(store, all, &id, yes)?,
        Command::Import { id, path } => {
            known(all, &id)?;
            let path = std::fs::canonicalize(&path).with_context(|| format!("can't find {}", path.display()))?;
            store.import(&id, &path.to_string_lossy())?;
            println!("Added {} and allowed {id}. Run `noactually sync` to import it.", path.display());
        }
        Command::Sync { json } => {
            let t = std::time::Instant::now();
            let r = store::sync(store, all)?;
            complete = r.errors.is_empty();
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                println!(
                    "Read {} files: {} messages, {} corrections ({:.1}s)",
                    r.files_read,
                    r.messages,
                    r.corrections,
                    t.elapsed().as_secs_f64()
                );
                for e in r.errors.iter().take(5) {
                    eprintln!("  skipped {e}");
                }
                if !complete {
                    eprintln!(
                        "{} files were skipped (the first {} are listed).",
                        r.errors.len(),
                        r.errors.len().min(5)
                    );
                }
            }
        }
        Command::Stats { filter, json, report_cards: true } => print_report_cards(store, all, &filter, json)?,
        Command::Stats { filter, json, report_cards: false } => {
            let s = store.stats(&filter, Local::now().date_naive())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&s)?);
            } else {
                print_card(&s, filter.since);
            }
        }
        Command::Check { text, json } => {
            if json {
                println!("{}", serde_json::to_string_pretty(&check_json(&text))?);
            } else {
                for line in check_lines(&text) {
                    println!("{line}");
                }
            }
        }
        Command::Retention { level, full_days, snippet_days, yes } => {
            let old = store.retention()?;
            let r = Retention {
                level,
                full_days: full_days.unwrap_or(old.full_days),
                snippet_days: snippet_days.unwrap_or(old.snippet_days),
            };
            set_retention(store, r, yes)?;
        }
    }
    Ok(complete)
}

/// Fails unless `id` is allowed: pausing or resuming a source nobody allowed (or one that was
/// stopped for good) must not read as, or turn into, consent. `allow` is the only way to give it.
fn allowed_only(store: &Store, id: &str, what: &str) -> Result<()> {
    if store.source_state(id)?.is_some_and(|s| s.allowed) {
        Ok(())
    } else {
        bail!("can't {what} {id}: it isn't allowed. Run `noactually allow {id}` to allow it.")
    }
}

/// `noactually stats --report-cards`: graded tasks instead of corrections.
fn print_report_cards(store: &Store, all: &[Box<dyn sources::Source>], filter: &Filter, json: bool) -> Result<()> {
    let habits = store::meta(all).habits;
    let totals = store.report_card_totals(filter)?;
    let (tip_trend, top_tip) = store.tip_summary(filter, Local::now().date_naive())?;
    let left_out = all.iter().map(|src| store.report_cards_left_out(src.id())).sum::<store::Result<i64>>()?;
    let s = report_cards::Summary {
        tip_trend,
        top_tip,
        left_out,
        ..report_cards::summary(&totals, store.recent_report_cards(filter, store::RECENT_REPORT_CARDS)?, &habits)
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&s)?);
    } else {
        let window = filter.since.map_or("in all history".to_string(), |d| format!("since {d}"));
        for line in report_cards::lines(&s, &window, &habits) {
            println!("{line}");
        }
    }
    Ok(())
}

/// The source with this id, or an error that says how to list them.
fn known<'a>(all: &'a [Box<dyn sources::Source>], id: &str) -> Result<&'a dyn sources::Source> {
    sources::find(all, id).map_err(|e| anyhow!("{e}. Run `noactually sources` to see them."))
}

fn print_sources(store: &Store, all: &[Box<dyn sources::Source>], json: bool) -> Result<()> {
    let decided = store.source_states()?;
    let mut rows = Vec::new();
    for s in all {
        let st = decided.iter().find(|x| x.id == s.id());
        let status = match st {
            Some(x) if x.allowed && x.paused => "paused",
            Some(x) if x.allowed => "allowed",
            _ => "not allowed",
        };
        let mut places: Vec<String> =
            s.roots().iter().filter(|r| r.exists()).map(|r| r.display().to_string()).collect();
        places.extend(st.map(|x| x.paths.clone()).unwrap_or_default());
        if json {
            rows.push(serde_json::json!({ "id": s.id(), "name": s.name(), "status": status, "places": places }));
        } else {
            let found = if places.is_empty() { "nothing found".to_string() } else { places.join(", ") };
            println!("{:<18} {:<12} {}", s.id(), status, found);
        }
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
    }
    Ok(())
}

fn print_card(s: &store::Stats, since: Option<NaiveDate>) {
    if s.messages == 0 {
        println!("No messages yet. Allow a source and run `noactually sync`.");
        return;
    }
    let window = window_label(since);
    for line in overall_lines(&s.overall, &window) {
        println!("{line}");
    }
    println!("{}", totals_line(s.corrections, s.humbled, since));
    println!(
        "  Messages        {}  ({} to {})",
        s.messages,
        s.first_day.as_deref().unwrap_or("?"),
        s.last_day.as_deref().unwrap_or("?")
    );
    if let Some(n) = s.messages_per_correction {
        println!("  1 correction every {n} messages");
    }
    if let Some(p) = s.pct_real_chats_with_correction {
        println!("  Real chats      {p}% of {} had a correction", s.real_chats);
    }
    println!("  Streak          {} days now, best {}", s.current_streak_days, s.best_streak_days);
    println!("\n  By model");
    for m in s.models.iter().filter(|m| m.messages > 0).take(10) {
        let name = if m.version.is_empty() { m.family.clone() } else { format!("{} {}", m.family, m.version) };
        let rate = 100.0 * m.corrections as f64 / m.messages as f64;
        println!("    {:<24} {:>5} corrections  {:>6} msgs  {:>4.1}%", name, m.corrections, m.messages, rate);
    }
    println!("\n  Corrections by kind");
    for c in engine::Category::ALL {
        if let Some(n) = s.categories.get(c.as_str()) {
            println!("    {:<24} {n}", c.title());
        }
    }
    for line in prompt_lines(&s.prompts, &window) {
        println!("{line}");
    }
}

/// Who corrected whom across all the stats cover: all time, or from `--since`.
fn totals_line(corrections: i64, humbled: i64, since: Option<NaiveDate>) -> String {
    let span = since.map_or("All time".to_string(), |d| window_label(Some(d)));
    format!("  {span:<15} you corrected it {corrections} · it corrected you {humbled}")
}

/// The span the score looks at: from `--since` when given, else the last [`engine::WINDOW_DAYS`] days.
fn window_label(since: Option<NaiveDate>) -> String {
    since.map_or_else(|| format!("in the last {} days", engine::WINDOW_DAYS), |d| format!("since {d}"))
}

/// "Could AI replace you?": the overall score and what it's made of, over `window`.
fn overall_lines(o: &store::Overall, window: &str) -> Vec<String> {
    let verdict = o.verdict.label();
    match (o.score, o.tier, o.direction, o.judgment) {
        (Some(score), Some(tier), Some(direction), Some(judgment)) => vec![
            format!(
                "Could AI replace you? {verdict} {} · score {score}{}\n",
                tier.name(),
                o.next_tier.map_or(String::new(), |n| format!("  ({} at {})", n.name(), n.floor()))
            ),
            format!(
                "  Your prompts    {direction}% clear the first time {window} ({}% of the score)",
                engine::DIRECTION_WEIGHT
            ),
            format!(
                "  Who corrects    you {} · it {} {window} → {judgment}{} ({}% of the score)",
                o.corrections,
                o.humbled,
                o.correction_tier.map_or(String::new(), |t| format!(", {}", t.name())),
                100 - engine::DIRECTION_WEIGHT
            ),
        ],
        _ => vec![format!(
            "Could AI replace you? {verdict} Not scored yet. Give it {} more tasks with more than one message {window}.\n",
            o.to_rank
        )],
    }
}

/// `noactually check "<prompt>"`: what a draft prompt carries, with the same rules scoring uses.
fn check_lines(text: &str) -> Vec<String> {
    let c = engine::check(text);
    let mut out = vec![format!(
        "{} ({} of {} things a prompt can include)",
        c.grade.title(),
        c.signals.count(),
        engine::Signal::ALL.len()
    )];
    for s in engine::Signal::ALL {
        let mark = if c.signals.has(s) { "✓" } else { "·" };
        out.push(format!("  {mark} {:<18} {}", s.title(), s.hint()));
    }
    if c.not_a_prompt {
        out.push("  (Reads as a question or a hello: your history doesn't grade those as prompts.)".into());
    }
    out
}

/// `noactually check --json`: the same reading as [`check_lines`], with ids instead of words.
fn check_json(text: &str) -> serde_json::Value {
    let c = engine::check(text);
    let (carried, missing): (Vec<_>, Vec<_>) = engine::Signal::ALL.into_iter().partition(|s| c.signals.has(*s));
    let ids = |v: Vec<engine::Signal>| v.into_iter().map(engine::Signal::as_str).collect::<Vec<_>>();
    serde_json::json!({
        "grade": c.grade.as_str(),
        "signals": ids(carried),
        "missing": ids(missing),
        "not_a_prompt": c.not_a_prompt,
    })
}

/// The "Do you make it guess?" half of the card; the tier's rate is over `window`.
fn prompt_lines(p: &store::Prompts, window: &str) -> Vec<String> {
    if p.tasks == 0 {
        return Vec::new();
    }
    let pct = store::floor_pct;
    let recent = pct(p.window_landed, p.window_tasks);
    let tier = match p.tier {
        Some(t) => format!(
            "{} {}  ({recent}% clear the first time {window}{})",
            t.verdict(),
            t.name(),
            p.next_tier.map_or(String::new(), |n| format!("; {} at {}%", n.name(), n.floor()))
        ),
        None => format!("Hmm. Give it {} more tasks with more than one message to get ranked", p.to_rank),
    };
    let mut out = vec![
        String::new(),
        format!("Do you make it guess? {tier}"),
        format!(
            "  Clear first time {}% of {} tasks with more than one message needed no adding, clarifying or redoing",
            pct(p.landed, p.continued),
            p.continued
        ),
        format!(
            "  Specific prompts {}%  ({}+ of the {} things a prompt can include)",
            pct(p.grades.get("sharp").copied().unwrap_or(0), p.tasks),
            engine::Grade::SHARP,
            engine::Signal::ALL.len()
        ),
        "\n  Your prompts".into(),
    ];
    for sig in engine::Signal::ALL {
        let n = p.signals.get(sig.as_str()).copied().unwrap_or(0);
        out.push(format!("    {:<20} {:>3}%  {}", sig.title(), pct(n, p.tasks), sig.hint()));
    }
    out.push("\n  Why you followed up".into());
    for c in engine::Steer::ALL {
        out.push(format!("    {:<20} {}", c.title(), p.steering.get(c.as_str()).copied().unwrap_or(0)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::Answer;

    fn parsed(line: &str) -> Result<Parsed> {
        let args: Vec<String> = line.split_whitespace().map(String::from).collect();
        parse_args(&args)
    }

    fn parse(line: &str) -> Result<Command> {
        parsed(line).map(|p| match p {
            Parsed::Run(cmd) => cmd,
            other => panic!("{line}: {other:?}"),
        })
    }

    fn err(line: &str) -> String {
        format!("{:#}", parse(line).unwrap_err())
    }

    #[test]
    fn commands_and_flags() {
        assert_eq!(parsed("").unwrap(), Parsed::Help, "bare `noactually` is help");
        assert!(err("frobnicate").starts_with("unknown command 'frobnicate'"));
        assert_eq!(parse("sync").unwrap(), Command::Sync { json: false });
        assert_eq!(parse("sync --json").unwrap(), Command::Sync { json: true });
        assert_eq!(parse("sources --json").unwrap(), Command::Sources { json: true });
        assert_eq!(parse("pause codex").unwrap(), Command::Pause { id: "codex".into() });
        assert_eq!(parse("resume codex").unwrap(), Command::Resume { id: "codex".into() });
        assert_eq!(
            parse("allow codex --since 2026-10-01").unwrap(),
            Command::Allow { id: "codex".into(), since: Some("2026-10-01".parse().unwrap()), from_now: false }
        );
        assert_eq!(
            parse("allow --from-now claude-code").unwrap(),
            Command::Allow { id: "claude-code".into(), since: None, from_now: true },
            "flags may come first"
        );
        assert_eq!(
            parse("import chatgpt-export ./conversations.json").unwrap(),
            Command::Import { id: "chatgpt-export".into(), path: "./conversations.json".into() }
        );
        let Command::Stats { filter, json, report_cards } =
            parse("stats --family Claude --version 5.5 --json --since 2026-10-01").unwrap()
        else {
            panic!("not stats")
        };
        assert_eq!(filter.since, Some("2026-10-01".parse().unwrap()));
        assert!(json && !report_cards);
        assert_eq!(
            (filter.family.as_deref(), filter.version.as_deref(), filter.source),
            (Some("Claude"), Some("5.5"), None)
        );
        let Command::Stats { filter, json, report_cards } = parse("stats --report-cards --source codex").unwrap()
        else {
            panic!("not stats")
        };
        assert_eq!((filter.source.as_deref(), json, report_cards), (Some("codex"), false, true));
        assert_eq!(
            parse("retention full --snippet-days 90").unwrap(),
            Command::Retention { level: Level::Full, full_days: None, snippet_days: Some(90), yes: false }
        );
        assert_eq!(
            parse("retention category -y").unwrap(),
            Command::Retention { level: Level::Category, full_days: None, snippet_days: None, yes: true }
        );
    }

    #[test]
    fn only_a_helper_command_is_turned_into_text() {
        let os = |s: &str| OsString::from(s);
        let text = |v: &[&str]| Some(v.iter().copied().map(String::from).collect::<Vec<_>>());
        assert_eq!(helper_args(&[os("digest"), os("--session"), os("abc")]), text(&["digest", "--session", "abc"]));
        assert_eq!(helper_args(&[os("hook"), os("stop")]), text(&["hook", "stop"]));
        assert_eq!(helper_args(&[]), None);
        assert_eq!(helper_args(&[os("stats"), os("--json")]), None);
        assert_eq!(helper_args(&[os("hook-ish")]), None);
        assert_eq!(helper_args(&[os("--hidden")]), None, "the app's own flag is not a helper");
    }

    #[cfg(unix)]
    #[test]
    fn an_argument_that_is_not_text_never_panics() {
        use std::os::unix::ffi::OsStringExt;
        let os = |s: &str| OsString::from(s);
        let bad = OsString::from_vec(vec![b'x', 0xff, 0xfe]);
        assert_eq!(
            helper_args(std::slice::from_ref(&bad)),
            None,
            "a first argument that isn't text is no helper command"
        );
        assert_eq!(helper_args(&[os("--hidden"), bad.clone()]), None, "and the app's flags are left alone");
        assert_eq!(helper_args(&[os("sync"), bad.clone()]), None);
        let helper = helper_args(&[os("digest"), os("--transcript"), bad.clone()]).unwrap();
        assert_eq!(helper[2], "x\u{fffd}\u{fffd}", "inside a helper command it is lossy text, so it finds nothing");
        let _ = run_os([os("digest"), os("--transcript"), bad]); // prints nothing, succeeds
    }

    #[test]
    fn help_and_version_never_run_a_command() {
        for line in
            ["--help", "-h", "help", "sync --help", "sync -h", "sync --json --help", "forget codex --help", "stats -h"]
        {
            assert_eq!(parsed(line).unwrap(), Parsed::Help, "{line}");
        }
        for line in ["--version", "-V", "version"] {
            assert_eq!(parsed(line).unwrap(), Parsed::Version, "{line}");
        }
        assert_eq!(
            parse("check --help me").unwrap(),
            Command::Check { text: "--help me".into(), json: false },
            "a prompt is a prompt"
        );
        assert!(err("sync now").starts_with("unexpected 'now'"));
        assert!(err("sources extra").starts_with("unexpected 'extra'"));
    }

    #[test]
    fn forget_needs_a_yes() {
        assert_eq!(parse("forget codex").unwrap(), Command::Forget { id: "codex".into(), yes: false });
        assert_eq!(parse("forget codex --yes").unwrap(), Command::Forget { id: "codex".into(), yes: true });
        assert_eq!(parse("forget -y codex").unwrap(), Command::Forget { id: "codex".into(), yes: true });
        let ask = |yes, tty, typed: &str| confirm_forget("codex", yes, tty, &mut typed.as_bytes());
        assert!(ask(true, false, "").unwrap(), "--yes needs no terminal");
        assert!(ask(false, true, "y\n").unwrap());
        assert!(ask(false, true, " YES \n").unwrap());
        assert!(!ask(false, true, "n\n").unwrap());
        assert!(!ask(false, true, "\n").unwrap(), "enter alone is no");
        assert!(!ask(false, true, "").unwrap(), "closed input is no");
        let e = ask(false, false, "y\n").unwrap_err().to_string();
        assert!(e.starts_with("refusing to delete what codex produced"), "{e}");
        assert!(e.contains("--yes"), "{e}");
    }

    #[test]
    fn retention_that_keeps_less_needs_a_yes() {
        let ask = |yes, tty, typed: &str| confirm_retention(yes, tty, &mut typed.as_bytes());
        assert!(ask(true, false, "").unwrap(), "--yes needs no terminal");
        assert!(ask(false, true, "y\n").unwrap());
        assert!(!ask(false, true, "\n").unwrap(), "enter alone is no");
        let e = ask(false, false, "y\n").unwrap_err().to_string();
        assert!(e.starts_with("refusing to delete older text"), "{e}");
        assert!(e.contains("--yes"), "{e}");

        let mut s = Store::open_in_memory().unwrap();
        let all = sources::all();
        let run = |s: &mut Store, level, full_days, yes| {
            execute(Command::Retention { level, full_days, snippet_days: None, yes }, s, &all)
        };
        // The test's stdin isn't a terminal, so without --yes anything that keeps less is refused.
        let refused = run(&mut s, Level::Category, None, false).unwrap_err();
        assert!(refused.to_string().contains("--yes"), "{refused}");
        assert_eq!(s.retention().unwrap(), Retention::default(), "nothing changed");
        run(&mut s, Level::Full, Some(60), false).unwrap();
        assert_eq!(s.retention().unwrap().level, Level::Full, "keeping more deletes nothing, so it just saves");
        assert!(run(&mut s, Level::Full, Some(7), false).is_err(), "fewer days keep less too");
        run(&mut s, Level::Category, None, true).unwrap();
        assert_eq!(s.retention().unwrap().level, Level::Category, "--yes saves it");
    }

    #[test]
    fn the_totals_line_names_its_span() {
        assert_eq!(totals_line(7, 2, None), "  All time        you corrected it 7 · it corrected you 2");
        let since = "2026-09-01".parse().unwrap();
        assert_eq!(totals_line(7, 2, Some(since)), "  since 2026-09-01 you corrected it 7 · it corrected you 2");
    }

    #[test]
    fn forget_deletes_only_when_confirmed() {
        let mut s = Store::open_in_memory().unwrap();
        let all = sources::all();
        execute(Command::Allow { id: "codex".into(), since: None, from_now: false }, &mut s, &all).unwrap();
        // The test's stdin isn't a terminal, so without --yes it refuses.
        let refused = execute(Command::Forget { id: "codex".into(), yes: false }, &mut s, &all).unwrap_err();
        assert!(refused.to_string().contains("--yes"), "{refused}");
        assert!(s.source_state("codex").unwrap().unwrap().allowed, "nothing was deleted");
        execute(Command::Forget { id: "codex".into(), yes: true }, &mut s, &all).unwrap();
        assert!(s.source_state("codex").unwrap().is_none(), "--yes deleted it");
    }

    #[test]
    fn check_reads_the_whole_rest_as_the_prompt() {
        assert_eq!(
            parse("check --fix the thing").unwrap(),
            Command::Check { text: "--fix the thing".into(), json: false }
        );
        assert_eq!(parse("check fix it --json").unwrap(), Command::Check { text: "fix it".into(), json: true });
        assert_eq!(parse("check --json").unwrap(), Command::Check { text: "--json".into(), json: false });
        assert!(err("check").starts_with("missing prompt"));
        let lines = check_lines("Fix the crash in `checkout.ts`. Done when the tests pass. Don't touch the API.");
        assert_eq!(lines[0], "Specific (3 of 5 things a prompt can include)");
        assert!(lines.iter().any(|l| l.starts_with("  ✓ Target")));
        assert!(lines.iter().any(|l| l.starts_with("  · Purpose")));
        assert!(check_lines("how does this work?").last().unwrap().contains("question"));
    }

    #[test]
    fn overall_lines_show_the_score_or_how_far_off_it_is() {
        let o = store::Overall {
            score: Some(86),
            direction: Some(83),
            judgment: Some(95),
            tier: Some(engine::Tier::SharpEyed),
            next_tier: Some(engine::Tier::Irreplaceable),
            verdict: Answer::No,
            correction_tier: Some(engine::CorrectionTier::HawkEyed),
            next_correction_tier: None,
            corrections: 41,
            humbled: 2,
            to_rank: 0,
        };
        let text = overall_lines(&o, &window_label(None)).join("\n");
        assert!(text.starts_with("Could AI replace you? No. Sharp-eyed · score 86  (Irreplaceable at 90)"), "{text}");
        assert!(text.contains("you 41 · it 2 in the last 28 days → 95, Hawk-eyed (30% of the score)"), "{text}");
        let unranked = store::Overall { score: None, tier: None, verdict: Answer::Hmm, to_rank: 4, ..o };
        assert!(overall_lines(&unranked, &window_label(None))[0]
            .contains("Not scored yet. Give it 4 more tasks with more than one message in the last 28 days."));
        let since = window_label(Some("2026-09-01".parse().unwrap()));
        let text = overall_lines(&o, &since).join("\n");
        assert!(text.contains("83% clear the first time since 2026-09-01"), "{text}");
        assert!(text.contains("you 41 · it 2 since 2026-09-01 →"), "{text}");
        assert!(!text.contains("28 days"), "{text}");
        assert!(overall_lines(&unranked, &since)[0].contains("with more than one message since 2026-09-01"));
    }

    #[test]
    fn mistakes_are_explained() {
        assert!(err("allow").starts_with("missing source"));
        assert!(err("import codex").starts_with("missing path"));
        assert!(err("pause").starts_with("missing source"));
        assert!(err("sync --jsno").starts_with("unknown option '--jsno'"));
        assert_eq!(err("allow codex --since yesterday"), "--since wants YYYY-MM-DD: input contains invalid characters");
        assert_eq!(err("allow codex --since 2026-10-01 --from-now"), "use --since or --from-now, not both");
        assert_eq!(err("stats --source"), "--source needs a value");
        assert!(err("stats --sorce codex").starts_with("unknown option '--sorce'"));
        assert_eq!(err("retention everything"), "Pick full, snippet or category.");
        assert_eq!(
            err("retention full --full-days soon"),
            "--full-days wants a whole number of days: invalid digit found in string"
        );
    }

    #[test]
    fn prompt_lines_say_the_tier_or_how_far_off_it_is() {
        assert!(
            prompt_lines(&store::Prompts::default(), &window_label(None)).is_empty(),
            "nothing before the first task"
        );
        let mut b = store::Prompts { tasks: 10, continued: 8, landed: 6, to_rank: 2, ..Default::default() };
        b.grades.insert("sharp".into(), 3);
        b.signals.insert("why".into(), 5);
        b.steering.insert("added".into(), 1);
        let text = prompt_lines(&b, &window_label(None)).join("\n");
        assert!(
            text.contains("Do you make it guess? Hmm. Give it 2 more tasks with more than one message to get ranked"),
            "{text}"
        );
        assert!(text.contains("Clear first time 75% of 8 tasks with more than one message"), "{text}");
        assert!(text.contains("Specific prompts 30%"), "{text}");
        assert!(text.contains("Purpose               50%"), "{text}");
        assert!(text.contains("You added more       1"), "{text}");
        b.tier = Some(engine::PromptTier::TechLead);
        b.next_tier = Some(engine::PromptTier::Architect);
        (b.window_tasks, b.window_landed) = (40, 34);
        assert!(prompt_lines(&b, &window_label(None))[1]
            .ends_with("Sometimes. Tech lead  (85% clear the first time in the last 28 days; Architect at 90%)"));
        let since = window_label(Some("2026-09-01".parse().unwrap()));
        assert!(prompt_lines(&b, &since)[1]
            .ends_with("Sometimes. Tech lead  (85% clear the first time since 2026-09-01; Architect at 90%)"));
    }

    #[test]
    fn runs_against_a_store() {
        let mut s = Store::open_in_memory().unwrap();
        let all = sources::all();
        let unknown = execute(Command::Revoke { id: "nope".into() }, &mut s, &all).unwrap_err();
        assert_eq!(unknown.to_string(), "unknown source 'nope'. Run `noactually sources` to see them.");
        execute(Command::Allow { id: "codex".into(), since: None, from_now: false }, &mut s, &all).unwrap();
        assert!(s.source_state("codex").unwrap().unwrap().allowed);
        let zero = Command::Retention { level: Level::Full, full_days: Some(0), snippet_days: None, yes: false };
        assert_eq!(execute(zero, &mut s, &all).unwrap_err().to_string(), "Keep text for at least 1 day.");
        assert_eq!(s.retention().unwrap(), Retention::default());

        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("conversations.json");
        std::fs::write(&p, "[]").unwrap();
        execute(Command::Import { id: "chatgpt-export".into(), path: p }, &mut s, &all).unwrap();
        assert!(s.source_state("chatgpt-export").unwrap().unwrap().allowed, "import allows, like the app");
    }

    #[test]
    fn pause_and_resume_never_allow_anything() {
        let mut s = Store::open_in_memory().unwrap();
        let all = sources::all();
        let run = |s: &mut Store, cmd| execute(cmd, s, &all);
        let pause = || Command::Pause { id: "codex".into() };
        let resume = || Command::Resume { id: "codex".into() };
        assert!(run(&mut s, pause()).unwrap_err().to_string().contains("isn't allowed"));
        assert!(run(&mut s, resume()).is_err());
        assert!(s.source_state("codex").unwrap().is_none(), "nothing was decided by trying");

        run(&mut s, Command::Allow { id: "codex".into(), since: None, from_now: false }).unwrap();
        run(&mut s, pause()).unwrap();
        let state = s.source_state("codex").unwrap().unwrap();
        assert!(state.allowed && state.paused);
        run(&mut s, resume()).unwrap();
        let state = s.source_state("codex").unwrap().unwrap();
        assert!(state.allowed && !state.paused);

        // A stopped source stays stopped: resume is not a way back from revoke.
        run(&mut s, Command::Revoke { id: "codex".into() }).unwrap();
        assert!(run(&mut s, resume()).is_err());
        assert!(!s.source_state("codex").unwrap().unwrap().allowed);
    }

    #[test]
    fn a_sync_that_skipped_a_file_is_not_a_success() {
        let mut s = Store::open_in_memory().unwrap();
        let all = sources::all();
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("conversations.json");
        std::fs::write(&p, "[]").unwrap();
        execute(Command::Import { id: "chatgpt-export".into(), path: p.clone() }, &mut s, &all).unwrap();
        assert!(execute(Command::Sync { json: false }, &mut s, &all).unwrap(), "a clean pass");
        std::fs::write(&p, "this is not json").unwrap();
        assert!(!execute(Command::Sync { json: true }, &mut s, &all).unwrap(), "one file skipped");
    }

    #[test]
    fn check_json_names_what_a_prompt_carries_and_lacks() {
        let v = check_json("Fix the crash in `checkout.ts`. Done when the tests pass. Don't touch the API.");
        assert_eq!(v["grade"], "sharp");
        assert_eq!(v["signals"], serde_json::json!(["spot", "done", "limits"]));
        assert_eq!(v["missing"], serde_json::json!(["proof", "why"]));
        assert_eq!(v["not_a_prompt"], false);
        assert_eq!(check_json("how does this work?")["not_a_prompt"], true);
    }
}
