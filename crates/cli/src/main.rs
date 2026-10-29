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

const HELP: &str = "noactually: could AI replace you? Score how you work with your AI

USAGE
  noactually sources                     List sources, where they read from, and whether they're allowed
  noactually allow <source> [--since YYYY-MM-DD | --from-now]
  noactually revoke <source>             Stop reading (keeps scores)
  noactually forget <source>             Stop reading and delete everything from it
  noactually import <source> <path>      Import an export file or folder (chatgpt-export, claude-ai-export)
  noactually sync                        Read anything new from allowed sources
  noactually stats [filters] [--json]    Scorecard. Filters: --source --provider --family --version --since
  noactually check <prompt>              What a draft prompt includes, graded like your history (nothing is saved)
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
    Check { text: String },
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
            let a = Args::split(rest, &["--source", "--provider", "--family", "--version", "--since"], &["--json"])?;
            let since =
                a.value("--since").map(|d| d.parse::<NaiveDate>()).transpose().context("--since wants YYYY-MM-DD")?;
            let filter = Filter {
                source: a.value("--source"),
                provider: a.value("--provider"),
                family: a.value("--family"),
                version: a.value("--version"),
                since,
            };
            Command::Stats { filter, json: a.has("--json") }
        }
        // The whole rest is the prompt, so it's never read as flags.
        "check" if !rest.is_empty() => Command::Check { text: rest.join(" ") },
        "check" => bail!("missing prompt: noactually check \"Fix the crash in checkout.ts…\""),
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
                "Read {} files: {} messages, {} corrections ({:.1}s)",
                r.files_read,
                r.messages,
                r.corrections,
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
                print_card(&s, filter.since);
            }
        }
        Command::Check { text } => {
            for line in check_lines(&text) {
                println!("{line}");
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

fn print_card(s: &store::Stats, since: Option<NaiveDate>) {
    if s.messages == 0 {
        println!("No messages yet. Allow a source and run `noactually sync`.");
        return;
    }
    let window = window_label(since);
    for line in overall_lines(&s.overall, &window) {
        println!("{line}");
    }
    println!("  You vs AI       {} – {}  (all time)", s.corrections, s.humbled);
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

/// The span the score looks at: from `--since` when given, else the last [`engine::WINDOW_DAYS`] days.
fn window_label(since: Option<NaiveDate>) -> String {
    since.map_or_else(|| format!("in {} days", engine::WINDOW_DAYS), |d| format!("since {d}"))
}

/// "Could AI replace you?": the overall score and what it's made of, over `window`.
fn overall_lines(o: &store::Overall, window: &str) -> Vec<String> {
    let verdict = match o.verdict {
        Answer::No => "No.",
        Answer::Hmm => "Maybe…",
    };
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
            "Could AI replace you? {verdict} Not scored yet: {} more tasks with a follow-up {window}.\n",
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
        None => format!("Hmm. {} more tasks with a follow-up to get ranked", p.to_rank),
    };
    let mut out = vec![
        String::new(),
        format!("Do you make it guess? {tier}"),
        format!(
            "  Clear first time {}% of {} prompts with a follow-up needed no adding, clarifying or redoing",
            pct(p.landed, p.continued),
            p.continued
        ),
        format!(
            "  Sharp prompts    {}%  ({}+ of the {} signals)",
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
        let Command::Stats { filter, json } =
            parse("stats --family Claude --version 5.5 --json --since 2026-10-01").unwrap()
        else {
            panic!("not stats")
        };
        assert_eq!(filter.since, Some("2026-10-01".parse().unwrap()));
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
    fn check_reads_the_whole_rest_as_the_prompt() {
        assert_eq!(parse("check --fix the thing").unwrap(), Command::Check { text: "--fix the thing".into() });
        assert!(err("check").starts_with("missing prompt"));
        let lines = check_lines("Fix the crash in `checkout.ts`. Done when the tests pass. Don't touch the API.");
        assert_eq!(lines[0], "Sharp (3 of 5 things a prompt can include)");
        assert!(lines.iter().any(|l| l.starts_with("  ✓ Points at it")));
        assert!(lines.iter().any(|l| l.starts_with("  · Says why")));
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
        assert!(text.contains("you 41 · it 2 in 28 days → 95, Hawk-eyed (30% of the score)"), "{text}");
        let unranked = store::Overall { score: None, tier: None, verdict: Answer::Hmm, to_rank: 4, ..o };
        assert!(overall_lines(&unranked, &window_label(None))[0].contains("Not scored yet: 4 more tasks"));
        let since = window_label(Some("2026-09-01".parse().unwrap()));
        let text = overall_lines(&o, &since).join("\n");
        assert!(text.contains("83% clear the first time since 2026-09-01"), "{text}");
        assert!(text.contains("you 41 · it 2 since 2026-09-01 →"), "{text}");
        assert!(!text.contains("28 days"), "{text}");
        assert!(overall_lines(&unranked, &since)[0].contains("with a follow-up since 2026-09-01"));
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
        assert!(text.contains("Do you make it guess? Hmm. 2 more tasks with a follow-up to get ranked"), "{text}");
        assert!(text.contains("Clear first time 75% of 8 prompts with a follow-up"), "{text}");
        assert!(text.contains("Sharp prompts    30%"), "{text}");
        assert!(text.contains("Says why              50%"), "{text}");
        assert!(text.contains("You added more       1"), "{text}");
        b.tier = Some(engine::PromptTier::TechLead);
        b.next_tier = Some(engine::PromptTier::Architect);
        (b.window_tasks, b.window_landed) = (40, 34);
        assert!(prompt_lines(&b, &window_label(None))[1]
            .ends_with("Sometimes. Tech lead  (85% clear the first time in 28 days; Architect at 90%)"));
        let since = window_label(Some("2026-09-01".parse().unwrap()));
        assert!(prompt_lines(&b, &since)[1]
            .ends_with("Sometimes. Tech lead  (85% clear the first time since 2026-09-01; Architect at 90%)"));
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
