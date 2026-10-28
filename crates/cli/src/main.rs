//! `noactually`: the command-line scorecard. Same engine, sources and database as the desktop
//! app, so either one can allow a source and both see the same numbers.
//!
//! Arguments are parsed by hand ([`parse_args`]): eight commands and a handful of flags don't
//! justify a parser dependency, and the help text stays exactly as written.

use anyhow::{anyhow, bail, Context, Result};
use chrono::{Local, NaiveDate};
use engine::Answer;
use std::path::PathBuf;
use store::{Filter, Level, Retention, Store};

const HELP: &str = "noactually: count the times you caught your AI being wrong

USAGE
  noactually sources                     List sources, where they read from, and whether they're allowed
  noactually allow <source> [--since YYYY-MM-DD | --from-now]
  noactually revoke <source>             Stop reading (keeps scores)
  noactually forget <source>             Stop reading and delete everything from it
  noactually import <source> <path>      Import an export file or folder (chatgpt-export, claude-ai-export)
  noactually sync                        Read anything new from allowed sources
  noactually stats [filters] [--json]    Scorecard. Filters: --source --provider --family --version
  noactually retention <full|snippet|category> [--full-days N] [--snippet-days N]

Data lives in NOACTUALLY_DB, or app.noactually/noactually.db in your app data folder (shared with the desktop app).
Nothing is read until you allow a source. Nothing leaves this machine.";

/// One parsed command line.
#[derive(Debug, PartialEq)]
enum Command {
    Help,
    Sources,
    Allow { id: String, since: Option<NaiveDate>, from_now: bool },
    Revoke { id: String },
    Forget { id: String },
    Import { id: String, path: PathBuf },
    Sync,
    Stats { filter: Filter, json: bool },
    Retention { level: Level, full_days: Option<u32>, snippet_days: Option<u32> },
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

/// Parses the arguments after the program name.
fn parse_args(args: &[String]) -> Result<Command> {
    let Some((cmd, rest)) = args.split_first() else { return Ok(Command::Help) };
    Ok(match cmd.as_str() {
        "sources" => Command::Sources,
        "sync" => Command::Sync,
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
        "revoke" => Command::Revoke { id: Args::split(rest, &[], &[])?.arg(0, "source")? },
        "forget" => Command::Forget { id: Args::split(rest, &[], &[])?.arg(0, "source")? },
        "import" => {
            let a = Args::split(rest, &[], &[])?;
            Command::Import { id: a.arg(0, "source")?, path: a.arg(1, "path")?.into() }
        }
        "stats" => {
            let a = Args::split(rest, &["--source", "--provider", "--family", "--version"], &["--json"])?;
            let filter = Filter {
                source: a.value("--source"),
                provider: a.value("--provider"),
                family: a.value("--family"),
                version: a.value("--version"),
            };
            Command::Stats { filter, json: a.has("--json") }
        }
        "retention" => {
            let a = Args::split(rest, &["--full-days", "--snippet-days"], &[])?;
            Command::Retention {
                level: a.arg(0, "level")?.parse()?,
                full_days: a.number("--full-days")?,
                snippet_days: a.number("--snippet-days")?,
            }
        }
        _ => Command::Help,
    })
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = parse_args(&args)?;
    if cmd == Command::Help {
        println!("{HELP}");
        return Ok(());
    }
    if store::legacy_db_env_in_use() {
        eprintln!("note: {} is deprecated; set {} instead.", store::LEGACY_DB_ENV, store::DB_ENV);
    }
    let mut store = Store::open(&store::default_db_path()?)?;
    run(cmd, &mut store, &sources::all())
}

fn run(cmd: Command, store: &mut Store, all: &[Box<dyn sources::Source>]) -> Result<()> {
    match cmd {
        Command::Help => println!("{HELP}"),
        Command::Sources => print_sources(store, all)?,
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
        Command::Revoke { id } => {
            known(all, &id)?;
            store.revoke(&id)?;
            println!("Stopped reading. Scores so far are kept.");
        }
        Command::Forget { id } => {
            known(all, &id)?;
            store.forget(&id)?;
            println!("Deleted everything from that source.");
        }
        Command::Import { id, path } => {
            known(all, &id)?;
            let path = std::fs::canonicalize(&path).with_context(|| format!("can't find {}", path.display()))?;
            store.import(&id, &path.to_string_lossy())?;
            println!("Added {} and allowed {id}. Run `noactually sync` to import it.", path.display());
        }
        Command::Sync => {
            let t = std::time::Instant::now();
            let r = store::sync(store, all)?;
            println!(
                "Read {} files: {} messages, {} catches ({:.1}s)",
                r.files_read,
                r.messages,
                r.catches,
                t.elapsed().as_secs_f64()
            );
            for e in r.errors.iter().take(5) {
                eprintln!("  skipped {e}");
            }
        }
        Command::Stats { filter, json } => {
            let s = store.stats(&filter, Local::now().date_naive())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&s)?);
            } else {
                print_card(&s);
            }
        }
        Command::Retention { level, full_days, snippet_days } => {
            let old = store.retention()?;
            let r = Retention {
                level,
                full_days: full_days.unwrap_or(old.full_days),
                snippet_days: snippet_days.unwrap_or(old.snippet_days),
            };
            store.set_retention(r)?;
            println!("Keeping {:?} text; full text {} days, snippets {} days.", r.level, r.full_days, r.snippet_days);
        }
    }
    Ok(())
}

/// The source with this id, or an error that says how to list them.
fn known<'a>(all: &'a [Box<dyn sources::Source>], id: &str) -> Result<&'a dyn sources::Source> {
    sources::find(all, id).map_err(|e| anyhow!("{e}. Run `noactually sources` to see them."))
}

fn print_sources(store: &Store, all: &[Box<dyn sources::Source>]) -> Result<()> {
    let decided = store.source_states()?;
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
        let found = if places.is_empty() { "nothing found".to_string() } else { places.join(", ") };
        println!("{:<18} {:<12} {}", s.id(), status, found);
    }
    Ok(())
}

fn print_card(s: &store::Stats) {
    if s.messages == 0 {
        println!("No messages yet. Allow a source and run `noactually sync`.");
        return;
    }
    let verdict = match s.verdict {
        Answer::No => "No.",
        Answer::Hmm => "Maybe…",
    };
    println!("Could AI replace you? {verdict} You caught it {} times in the last 7 days.\n", s.catches_last_7d);
    println!(
        "  Tier            {}{}",
        s.tier.name(),
        s.next_tier.map_or(String::new(), |(n, k)| format!("  ({k} more this week for {})", n.name()))
    );
    println!("  You vs AI       {} – {}", s.catches, s.humbled);
    println!(
        "  Messages        {}  ({} to {})",
        s.messages,
        s.first_day.as_deref().unwrap_or("?"),
        s.last_day.as_deref().unwrap_or("?")
    );
    if let Some(n) = s.messages_per_catch {
        println!("  1 catch every   {n} messages");
    }
    if let Some(p) = s.pct_real_chats_with_catch {
        println!("  Real chats      {p}% of {} had a catch", s.real_chats);
    }
    println!("  Streak          {} days now, best {}", s.current_streak_days, s.best_streak_days);
    println!("\n  By model");
    for m in s.models.iter().filter(|m| m.messages > 0).take(10) {
        let name = if m.version.is_empty() { m.family.clone() } else { format!("{} {}", m.family, m.version) };
        let rate = 100.0 * m.catches as f64 / m.messages as f64;
        println!("    {:<24} {:>5} catches  {:>6} msgs  {:>4.1}%", name, m.catches, m.messages, rate);
    }
    println!("\n  Catches by kind");
    for c in engine::Category::ALL {
        if let Some(n) = s.categories.get(c.as_str()) {
            println!("    {:<24} {n}", c.title());
        }
    }
    for line in brief_lines(&s.briefs) {
        println!("{line}");
    }
}

/// The "Do you make it guess?" half of the card.
fn brief_lines(b: &store::Briefs) -> Vec<String> {
    if b.tasks == 0 {
        return Vec::new();
    }
    let pct = |n: i64, of: i64| if of > 0 { 100 * n / of } else { 0 };
    let recent = pct(b.landed_last_28d, b.tasks_last_28d);
    let tier = match b.tier {
        Some(t) => format!(
            "{} {}  ({recent}% landed in {} days{})",
            t.verdict(),
            t.name(),
            engine::WINDOW_DAYS,
            b.next_tier.map_or(String::new(), |n| format!("; {} at {}%", n.name(), n.floor()))
        ),
        None => format!("Hmm. {} more tasks with a follow-up to get ranked", b.to_rank),
    };
    let mut out = vec![
        String::new(),
        format!("Do you make it guess? {tier}"),
        format!(
            "  Landed           {}% of {} tasks with a follow-up needed no adding, clarifying or redoing",
            pct(b.landed, b.continued),
            b.continued
        ),
        format!(
            "  Sharp briefs     {}%  ({}+ of the 5 signals)",
            pct(b.grades.get("sharp").copied().unwrap_or(0), b.tasks),
            engine::Grade::SHARP
        ),
        "\n  Your briefs".into(),
    ];
    for sig in engine::Signal::ALL {
        let n = b.signals.get(sig.as_str()).copied().unwrap_or(0);
        out.push(format!("    {:<20} {:>3}%  {}", sig.title(), pct(n, b.tasks), sig.hint()));
    }
    out.push("\n  Steering".into());
    for c in engine::Steer::ALL {
        out.push(format!("    {:<20} {}", c.title(), b.steering.get(c.as_str()).copied().unwrap_or(0)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Result<Command> {
        let args: Vec<String> = line.split_whitespace().map(String::from).collect();
        parse_args(&args)
    }

    fn err(line: &str) -> String {
        format!("{:#}", parse(line).unwrap_err())
    }

    #[test]
    fn commands_and_flags() {
        assert_eq!(parse("").unwrap(), Command::Help);
        assert_eq!(parse("frobnicate").unwrap(), Command::Help);
        assert_eq!(parse("sync").unwrap(), Command::Sync);
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
        let Command::Stats { filter, json } = parse("stats --family Claude --version 5.5 --json").unwrap() else {
            panic!("not stats")
        };
        assert!(json);
        assert_eq!(
            (filter.family.as_deref(), filter.version.as_deref(), filter.source),
            (Some("Claude"), Some("5.5"), None)
        );
        assert_eq!(
            parse("retention full --snippet-days 90").unwrap(),
            Command::Retention { level: Level::Full, full_days: None, snippet_days: Some(90) }
        );
    }

    #[test]
    fn mistakes_are_explained() {
        assert!(err("allow").starts_with("missing source"));
        assert!(err("import codex").starts_with("missing path"));
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
    fn brief_lines_say_the_tier_or_how_far_off_it_is() {
        assert!(brief_lines(&store::Briefs::default()).is_empty(), "nothing before the first task");
        let mut b = store::Briefs { tasks: 10, continued: 8, landed: 6, to_rank: 2, ..Default::default() };
        b.grades.insert("sharp".into(), 3);
        b.signals.insert("why".into(), 5);
        b.steering.insert("added".into(), 1);
        let text = brief_lines(&b).join("\n");
        assert!(text.contains("Do you make it guess? Hmm. 2 more tasks with a follow-up to get ranked"), "{text}");
        assert!(text.contains("Landed           75% of 8 tasks with a follow-up"), "{text}");
        assert!(text.contains("Sharp briefs     30%"), "{text}");
        assert!(text.contains("Says why              50%"), "{text}");
        assert!(text.contains("You added more       1"), "{text}");
        b.tier = Some(engine::BriefTier::TechLead);
        b.next_tier = Some(engine::BriefTier::Architect);
        (b.tasks_last_28d, b.landed_last_28d) = (40, 34);
        assert!(brief_lines(&b)[1].ends_with("Sometimes. Tech lead  (85% landed in 28 days; Architect at 90%)"));
    }

    #[test]
    fn runs_against_a_store() {
        let mut s = Store::open_in_memory().unwrap();
        let all = sources::all();
        let unknown = run(Command::Revoke { id: "nope".into() }, &mut s, &all).unwrap_err();
        assert_eq!(unknown.to_string(), "unknown source 'nope'. Run `noactually sources` to see them.");
        run(Command::Allow { id: "codex".into(), since: None, from_now: false }, &mut s, &all).unwrap();
        assert!(s.source_state("codex").unwrap().unwrap().allowed);
        let zero = Command::Retention { level: Level::Full, full_days: Some(0), snippet_days: None };
        assert!(run(zero, &mut s, &all).is_err());
        assert_eq!(s.retention().unwrap(), Retention::default());

        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("conversations.json");
        std::fs::write(&p, "[]").unwrap();
        run(Command::Import { id: "chatgpt-export".into(), path: p }, &mut s, &all).unwrap();
        assert!(s.source_state("chatgpt-export").unwrap().unwrap().allowed, "import allows, like the app");
    }
}
