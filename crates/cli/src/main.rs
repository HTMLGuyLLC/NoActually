use anyhow::{bail, Context, Result};
use chrono::{Local, NaiveDate};
use std::path::PathBuf;
use store::{Filter, Level, Retention, Store};

const HELP: &str = "noactually: count the times you caught your AI being wrong

USAGE
  noactually sources                     List sources, where they read from, and whether they're allowed
  noactually allow <source> [--since YYYY-MM-DD | --from-now]
  noactually revoke <source>             Stop reading (keeps scores)
  noactually forget <source>             Stop reading and delete everything from it
  noactually import <source> <path>      Add an export file or folder (chatgpt-export, claude-ai-export)
  noactually sync                        Read anything new from allowed sources
  noactually stats [filters] [--json]    Scorecard. Filters: --source --provider --family --version
  noactually retention <full|snippet|category> [--full-days N] [--snippet-days N]

Data lives in WA_DB, or app.noactually/noactually.db in your app data folder (shared with the desktop app).
Nothing is read until you allow a source. Nothing leaves this machine.";

fn db_path() -> PathBuf {
    if let Some(p) = std::env::var_os("WA_DB") {
        return p.into();
    }
    // Same file the desktop app uses (Tauri's app data dir for the app.noactually identifier).
    dirs::data_dir().unwrap_or_default().join("app.noactually").join("noactually.db")
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    let arg = |i: usize, what: &str| args.get(i).cloned().with_context(|| format!("missing {what}\n\n{HELP}"));
    let all = sources::all();
    let known = |id: &str| -> Result<()> {
        if all.iter().any(|s| s.id() == id) {
            Ok(())
        } else {
            bail!("unknown source '{id}'. Run `noactually sources`.")
        }
    };
    let mut store = Store::open(&db_path())?;

    match cmd {
        "sources" => {
            let states = store.source_states()?;
            for s in &all {
                let st = states.iter().find(|x| x.id == s.id());
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
        }
        "allow" => {
            let id = arg(1, "source")?;
            known(&id)?;
            let since = flag(&args, "--since")
                .map(|d| d.parse::<NaiveDate>())
                .transpose()
                .context("--since wants YYYY-MM-DD")?;
            store.allow(&id, since)?;
            if args.iter().any(|a| a == "--from-now") {
                let src = all.iter().find(|s| s.id() == id).expect("checked above");
                let n = store.start_from_now(src.as_ref())?;
                println!("Allowed {id} from now on ({n} existing files skipped).");
            } else {
                println!("Allowed {id}. Run `noactually sync` to read it.");
            }
        }
        "revoke" => {
            store.revoke(&arg(1, "source")?)?;
            println!("Stopped reading. Scores so far are kept.");
        }
        "forget" => {
            store.forget(&arg(1, "source")?)?;
            println!("Deleted everything from that source.");
        }
        "import" => {
            let id = arg(1, "source")?;
            known(&id)?;
            let path = std::fs::canonicalize(arg(2, "path")?)?;
            store.add_path(&id, &path.to_string_lossy())?;
            println!("Added {}. Allow {id} and sync to import it.", path.display());
        }
        "sync" => {
            let t = std::time::Instant::now();
            let r = store::sync(&mut store, &all)?;
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
        "stats" => {
            let f = Filter {
                source: flag(&args, "--source"),
                provider: flag(&args, "--provider"),
                family: flag(&args, "--family"),
                version: flag(&args, "--version"),
            };
            let s = store.stats(&f, Local::now().date_naive())?;
            if args.iter().any(|a| a == "--json") {
                println!("{}", serde_json::to_string_pretty(&s)?);
            } else {
                print_card(&s);
            }
        }
        "retention" => {
            let level = match arg(1, "level")?.as_str() {
                "full" => Level::Full,
                "snippet" => Level::Snippet,
                "category" => Level::Category,
                other => bail!("unknown level '{other}'"),
            };
            let mut r = Retention { level, ..store.retention()? };
            if let Some(d) = flag(&args, "--full-days") {
                r.full_days = d.parse()?;
            }
            if let Some(d) = flag(&args, "--snippet-days") {
                r.snippet_days = d.parse()?;
            }
            store.set_retention(r)?;
            println!("Keeping {:?} text; full text {} days, snippets {} days.", r.level, r.full_days, r.snippet_days);
        }
        _ => println!("{HELP}"),
    }
    Ok(())
}

fn print_card(s: &store::Stats) {
    if s.messages == 0 {
        println!("No messages yet. Allow a source and run `noactually sync`.");
        return;
    }
    let verdict = if s.catches_last_7d >= 3 { "No." } else { "Maybe…" };
    println!("Could AI replace you? {verdict} You caught it {} times in the last 7 days.\n", s.catches_last_7d);
    println!(
        "  Tier            {}{}",
        s.tier,
        s.next_tier.map_or(String::new(), |(n, k)| format!("  ({k} more this week for {n})"))
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
        let rate = if m.messages > 0 { 100.0 * m.catches as f64 / m.messages as f64 } else { 0.0 };
        println!("    {:<24} {:>5} catches  {:>6} msgs  {:>4.1}%", name, m.catches, m.messages, rate);
    }
    println!("\n  Catches by kind");
    for c in engine::Category::ALL {
        if let Some(n) = s.categories.get(c.as_str()) {
            println!("    {:<24} {n}", c.title());
        }
    }
}
