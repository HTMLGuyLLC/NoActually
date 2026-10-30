//! The store's behavior suite: ingest, sync, consent, retention and stats, end to end.

use super::*;
use engine::{Answer, PromptTier, Signal, Signals};
use sources::testing::{cc_ai, cc_correction, cc_user, set_mtime_ago};
use sources::{FileScan, SourceTurn};
use std::collections::BTreeMap;
use std::io::Write;
use std::time::Duration as StdDuration;

fn turn(human: &str, next: &str, model: &str, ts: &str, key: &str) -> SourceTurn {
    SourceTurn {
        source: "test",
        session: "s1".into(),
        ts: ts.parse().unwrap(),
        key: Some(key.into()),
        turn: engine::Turn {
            human: human.into(),
            prev_ai: Some(engine::AiMsg { text: "Here you go.".into(), model: Some(model.into()) }),
            next_ai: Some(engine::AiMsg { text: next.into(), model: Some(model.into()) }),
            chat_model: None,
        },
    }
}

fn scan(turns: Vec<SourceTurn>) -> FileScan {
    FileScan { turns, checkpoint: 100, cards: Vec::new() }
}

fn test_source() -> SourceState {
    SourceState { allowed: true, ..SourceState::new("test") }
}

/// Ingests `turns` as file `path` of the "test" source with no consent limits.
fn ingest(s: &mut Store, path: &str, turns: Vec<SourceTurn>) {
    s.ingest(&test_source(), path, &scan(turns), FileMeta { size: 100, mtime: 1 }, false).unwrap();
}

fn today() -> NaiveDate {
    Local::now().date_naive()
}

fn now_ts(days_ago: i64) -> String {
    (Utc::now() - Duration::days(days_ago)).to_rfc3339()
}

fn stats(s: &Store) -> Stats {
    s.stats(&Filter::default(), today()).unwrap()
}

fn claude_code(dir: &tempfile::TempDir) -> Vec<Box<dyn Source>> {
    vec![sources::claude_code_at(dir.path().to_path_buf())]
}

/// Appends to a history file and moves its mtime, as a tool writing to it would.
fn append(path: &Path, text: &str) {
    let mut f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    f.write_all(text.as_bytes()).unwrap();
    drop(f);
    set_mtime_ago(path, StdDuration::ZERO);
}

#[test]
fn counts_messages_corrections_and_models() {
    let mut s = Store::open_in_memory().unwrap();
    let t = now_ts(0);
    ingest(
        &mut s,
        "f",
        vec![
            turn("that's not a real page", "You're right, it doesn't exist.", "claude-opus-5-5", &t, "a"),
            turn("thanks", "Anytime.", "claude-opus-5-5", &t, "b"),
            turn("still not working", "Checking the logs.", "gpt-5.1", &t, "c"),
        ],
    );
    let all = stats(&s);
    assert_eq!((all.messages, all.corrections, all.corrections_last_7d), (3, 2, 2));
    assert_eq!(all.categories.get("made_it_up"), Some(&1));
    assert_eq!(
        (all.overall.tier, all.overall.verdict, all.overall.corrections),
        (None, Answer::Hmm, 2),
        "too few tasks to score"
    );

    let opus = s
        .stats(
            &Filter { family: Some("Claude Opus".into()), version: Some("5.5".into()), ..Default::default() },
            today(),
        )
        .unwrap();
    assert_eq!((opus.messages, opus.corrections), (2, 1));
    let family = s.stats(&Filter { family: Some("Claude Opus".into()), ..Default::default() }, today()).unwrap();
    assert_eq!(family.messages, 2);
    let none = s.stats(&Filter { version: Some("9.9".into()), ..Default::default() }, today()).unwrap();
    assert_eq!((none.messages, none.first_day), (0, None));
    let openai = s.stats(&Filter { provider: Some("OpenAI".into()), ..Default::default() }, today()).unwrap();
    assert_eq!((openai.messages, openai.corrections), (1, 1));
}

#[test]
fn stats_json_matches_the_ui_contract() {
    let mut s = Store::open_in_memory().unwrap();
    let t = now_ts(0);
    let corrections: Vec<_> =
        (0..7).map(|i| turn("that's not a real page", "You're right.", "m", &t, &i.to_string())).collect();
    ingest(&mut s, "f", corrections);
    let v = serde_json::to_value(stats(&s)).unwrap();
    let o = &v["overall"];
    assert_eq!((o["tier"].is_null(), o["score"].is_null(), &o["verdict"]), (true, true, &serde_json::json!("hmm")));
    assert_eq!((o["corrections"].as_i64(), &o["to_rank"]), (Some(7), &v["prompts"]["to_rank"]));
    let per_model = |k: &str| v["models"].as_array().unwrap().iter().map(|m| m[k].as_i64().unwrap()).sum::<i64>();
    assert_eq!(
        (per_model("tasks"), per_model("continued")),
        (v["prompts"]["tasks"].as_i64().unwrap(), v["prompts"]["continued"].as_i64().unwrap()),
        "every task is under some model"
    );
    let entry = &v["hall_of_fame"][0];
    assert_eq!(
        (entry["category"].as_str(), entry["category_title"].as_str()),
        (Some("made_it_up"), Some("Made it up"))
    );
    assert_eq!(entry["source_name"], "test", "unknown sources fall back to their id");
}

#[test]
fn hall_names_known_sources() {
    let mut s = Store::open_in_memory().unwrap();
    let state = SourceState { allowed: true, ..SourceState::new("claude-code") };
    let t = turn("no, that's wrong", "You're right.", "m", &now_ts(0), "k");
    s.ingest(&state, "f", &scan(vec![t]), FileMeta::default(), false).unwrap();
    assert_eq!(stats(&s).hall_of_fame[0].source_name, "Claude Code");
}

#[test]
fn weekly_buckets_split_on_monday() {
    let mut s = Store::open_in_memory().unwrap();
    // Noon UTC stays on the same local day in every timezone from UTC-11 to UTC+11.
    let sunday = "2026-10-25T12:00:00Z";
    let monday = "2026-10-26T12:00:00Z";
    ingest(
        &mut s,
        "f",
        vec![turn("no, that's wrong", "You're right.", "m", sunday, "a"), turn("thanks", "Sure.", "m", monday, "b")],
    );
    let w = s.stats(&Filter::default(), "2026-10-27".parse().unwrap()).unwrap().weekly;
    let d = |s: &str| s.parse::<NaiveDate>().unwrap();
    let row = |w: &WeekRow| (w.week, w.messages, w.corrections);
    assert_eq!(w.iter().map(row).collect::<Vec<_>>(), vec![(d("2026-10-19"), 1, 1), (d("2026-10-26"), 1, 0)]);
    // The week's breakdown: which kind of correction it was, and steering on that week's tasks.
    assert_eq!(w[0].categories, [("just_wrong".to_string(), 1)].into());
    assert!(w[1].categories.is_empty());
    assert!(w.iter().all(|w| w.steering.values().all(|n| *n == 0)), "first messages have no steering");
}

#[test]
fn streaks_and_peak_hour_come_from_stored_corrections() {
    let mut s = Store::open_in_memory().unwrap();
    let at = |days_ago: i64| (Utc::now() - Duration::days(days_ago)).to_rfc3339();
    let corrections = [0, 1, 2, 5].map(|d| turn("no, that's wrong", "You're right.", "m", &at(d), &d.to_string()));
    ingest(&mut s, "f", corrections.into());
    let st = stats(&s);
    assert_eq!((st.current_streak_days, st.best_streak_days), (3, 3));
    assert!(st.peak_hour.is_some());
    assert_eq!(Store::open_in_memory().unwrap().stats(&Filter::default(), today()).unwrap().peak_hour, None);
}

#[test]
fn copies_are_never_counted_twice() {
    let mut s = Store::open_in_memory().unwrap();
    let t = now_ts(0);
    let one = || vec![turn("no, that's wrong", "You're right.", "claude-opus-5-5", &t, "same-id")];
    ingest(&mut s, "f1", one());
    ingest(&mut s, "f2", one());
    assert_eq!(stats(&s).corrections, 1);
}

#[test]
fn backfill_window_is_respected() {
    let mut s = Store::open_in_memory().unwrap();
    let state = SourceState { backfill_since: Some(today() - Duration::days(7)), ..test_source() };
    let old = turn("no, that's wrong", "You're right.", "m", &now_ts(40), "x");
    s.ingest(&state, "f", &scan(vec![old.clone()]), FileMeta::default(), false).unwrap();
    assert_eq!(stats(&s).messages, 0);

    // Skipped messages aren't marked as seen, so allowing all history later still counts them.
    s.ingest(&test_source(), "f", &scan(vec![old]), FileMeta::default(), false).unwrap();
    assert_eq!(stats(&s).messages, 1);
}

#[test]
fn retention_steps_down_and_pins_hold() {
    let mut s = Store::open_in_memory().unwrap();
    s.set_retention(Retention { level: Level::Full, full_days: 30, snippet_days: 365 }).unwrap();
    ingest(
        &mut s,
        "f",
        vec![
            turn("no, that's wrong. Email me at pat@example.com", "You're right, my mistake.", "m", &now_ts(1), "new"),
            turn("that's not a real page", "You're right.", "m", &now_ts(60), "mid"),
            turn("you missed the tests", "My mistake.", "m", &now_ts(400), "old"),
            turn("you forgot the docs", "My mistake.", "m", &now_ts(400), "pinned"),
        ],
    );
    let pinned_id: i64 =
        s.conn.query_row("SELECT id FROM events WHERE human_full LIKE '%docs%'", [], |r| r.get(0)).unwrap();
    s.pin(pinned_id, true).unwrap();
    s.sweep(Utc::now()).unwrap();

    let level = |needle: &str| -> (String, Option<String>, Option<String>) {
        s.conn
            .query_row(
                "SELECT level, human_full, human_snip FROM events WHERE seen_key = ?1",
                [hash(&["test", needle])],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
    };
    let (l, full, _) = level("new");
    assert_eq!(l, "full");
    assert!(full.unwrap().contains("[email]"), "full text is still redacted");
    assert_eq!(level("mid").0, "snippet");
    assert_eq!(level("old"), ("category".into(), None, None));
    assert_eq!(level("pinned").0, "full");

    // Lowering the level applies to what's already stored, right away.
    s.set_retention(Retention { level: Level::Category, full_days: 30, snippet_days: 365 }).unwrap();
    assert_eq!(level("new"), ("category".into(), None, None));
    assert_eq!(level("pinned").0, "full", "pins still hold");
    s.pin(pinned_id, false).unwrap();
    s.sweep(Utc::now()).unwrap();
    assert_eq!(level("pinned"), ("category".into(), None, None), "unpinned text steps down on the next sweep");
    assert!(stats(&s).hall_of_fame.is_empty(), "no text left to show");
}

#[test]
fn category_level_keeps_no_text_from_the_start() {
    let mut s = Store::open_in_memory().unwrap();
    s.set_retention(Retention { level: Level::Category, ..Retention::default() }).unwrap();
    ingest(&mut s, "f", vec![turn("no, that's wrong", "You're right.", "m", &now_ts(0), "k")]);
    let text: (Option<String>, Option<String>) =
        s.conn.query_row("SELECT human_full, human_snip FROM events", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(text, (None, None));
    assert_eq!(stats(&s).corrections, 1, "still counted");
}

#[test]
fn retention_is_validated() {
    let s = Store::open_in_memory().unwrap();
    let bad = Retention { full_days: 0, ..Retention::default() };
    assert_eq!(bad.validate().unwrap_err().to_string(), "Keep text for at least 1 day.");
    assert!(s.set_retention(bad).is_err());
    assert_eq!(s.retention().unwrap(), Retention::default(), "nothing saved");
    let huge = Retention { snippet_days: u32::MAX, ..Retention::default() };
    assert!(huge.validate().is_err() && s.set_retention(huge).is_err(), "absurd periods are refused");
    assert!(Retention { full_days: 36_500, snippet_days: 36_500, ..Retention::default() }.validate().is_ok());
    // A bad value already on disk skips the sweep instead of panicking.
    s.set_setting("retention", r#"{"level":"snippet","full_days":30,"snippet_days":4000000000}"#).unwrap();
    assert_eq!(s.sweep(Utc::now()).unwrap(), 0);
    assert_eq!("snippet".parse::<Level>().unwrap(), Level::Snippet);
    assert!("everything".parse::<Level>().is_err());
}

#[test]
fn forget_deletes_everything_for_a_source() {
    let mut s = Store::open_in_memory().unwrap();
    s.allow("test", None).unwrap();
    ingest(&mut s, "f", vec![turn("no, that's wrong", "You're right.", "m", &now_ts(0), "k")]);
    s.forget("test").unwrap();
    let left: i64 = s
        .conn
        .query_row("SELECT (SELECT COUNT(*) FROM events)+(SELECT COUNT(*) FROM daily)+(SELECT COUNT(*) FROM seen)+(SELECT COUNT(*) FROM files)+(SELECT COUNT(*) FROM sources)", [], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 0);
}

#[test]
fn consent_paths_and_pause_are_stored() {
    let s = Store::open_in_memory().unwrap();
    s.add_path("chatgpt-export", "/exports/a.json").unwrap();
    s.add_path("chatgpt-export", "/exports/a.json").unwrap();
    s.add_path("chatgpt-export", "/exports/b.json").unwrap();
    let st = s.source_state("chatgpt-export").unwrap().unwrap();
    assert_eq!(st.paths, ["/exports/a.json", "/exports/b.json"], "no duplicates, order kept");
    assert!(!st.allowed, "adding a path alone is not consent");

    s.import("claude-ai-export", "/exports/c.json").unwrap();
    let st = s.source_state("claude-ai-export").unwrap().unwrap();
    assert!(st.allowed && st.paths == ["/exports/c.json"], "importing allows it");

    s.set_paused("claude-ai-export", true).unwrap();
    assert!(s.source_state("claude-ai-export").unwrap().unwrap().paused);
    s.allow("claude-ai-export", None).unwrap();
    let st = s.source_state("claude-ai-export").unwrap().unwrap();
    assert!(!st.paused && st.paths == ["/exports/c.json"], "allowing again unpauses and keeps paths");
}

#[test]
fn imports_are_read_from_their_paths() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("conversations.json");
    let convs = serde_json::json!([{"uuid":"c","chat_messages":[
        {"uuid":"1","sender":"assistant","text":"Use /admin","created_at":now_ts(1)},
        {"uuid":"2","sender":"human","text":"that page doesn't exist","created_at":now_ts(1)},
        {"uuid":"3","sender":"assistant","text":"You're right.","created_at":now_ts(1)}
    ]}]);
    std::fs::write(&p, convs.to_string()).unwrap();
    let mut s = Store::open_in_memory().unwrap();
    s.import("claude-ai-export", &p.to_string_lossy()).unwrap();
    let r = sync(&mut s, &sources::all()).unwrap();
    assert_eq!((r.files_read, r.corrections), (1, 1));
    assert_eq!(sync(&mut s, &sources::all()).unwrap().files_read, 0, "unchanged exports aren't re-read");
}

#[test]
fn nothing_is_read_without_consent_and_live_appends_count_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("abc.jsonl");
    let ts = Utc::now().to_rfc3339();
    std::fs::write(&path, cc_ai("claude-opus-5-5", &ts, "Use /admin.") + &cc_user("u1", &ts, "that's not a real page"))
        .unwrap();

    let mut s = Store::open_in_memory().unwrap();
    let all = claude_code(&dir);
    // Not allowed yet: nothing read.
    assert_eq!(sync(&mut s, &all).unwrap().files_read, 0);

    s.allow("claude-code", None).unwrap();
    let r = sync(&mut s, &all).unwrap();
    assert_eq!((r.files_read, r.messages), (1, 0), "the message waits for its reply");

    append(&path, &cc_ai("claude-opus-5-5", &ts, "You're right, my mistake."));
    let r = sync(&mut s, &all).unwrap();
    assert_eq!((r.messages, r.corrections), (1, 1));
    let again = sync(&mut s, &all).unwrap();
    assert_eq!((again.files_read, again.messages), (0, 0));
    assert_eq!(stats(&s).corrections, 1);
}

#[test]
fn quiet_files_count_their_last_unanswered_message() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("abc.jsonl");
    let ts = (Utc::now() - Duration::hours(1)).to_rfc3339();
    std::fs::write(&path, cc_ai("claude-opus-5-5", &ts, "Done.") + &cc_user("u1", &ts, "you missed the tests"))
        .unwrap();
    let mut s = Store::open_in_memory().unwrap();
    s.allow("claude-code", None).unwrap();
    let all = claude_code(&dir);
    assert_eq!(sync(&mut s, &all).unwrap().messages, 0, "still being written");
    set_mtime_ago(&path, StdDuration::from_secs(20 * 60));
    let r = sync(&mut s, &all).unwrap();
    assert_eq!((r.messages, r.corrections), (1, 1), "a quiet file is final");
    assert_eq!(sync(&mut s, &all).unwrap().files_read, 0);
}

#[test]
fn rewritten_or_shrunk_files_never_double_count() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("abc.jsonl");
    let ts = (Utc::now() - Duration::hours(2)).to_rfc3339();
    let body = cc_ai("claude-opus-5-5", &ts, "Use /admin.")
        + &cc_user("u1", &ts, "that's not a real page")
        + &cc_ai("claude-opus-5-5", &ts, "You're right.");
    std::fs::write(&path, &body).unwrap();
    let all = claude_code(&dir);
    let mut s = Store::open_in_memory().unwrap();
    s.allow("claude-code", None).unwrap();
    sync(&mut s, &all).unwrap();

    // The tool rewrites the file (compaction, re-save): it's re-read from zero, keys block copies.
    std::fs::write(&path, format!("{{\"type\":\"summary\"}}\n{body}")).unwrap();
    set_mtime_ago(&path, StdDuration::ZERO);
    assert_eq!(sync(&mut s, &all).unwrap().files_read, 1);
    std::fs::write(&path, &body[..body.len() / 2]).unwrap(); // shrinks
    set_mtime_ago(&path, StdDuration::ZERO);
    assert_eq!(sync(&mut s, &all).unwrap().files_read, 1);

    let st = stats(&s);
    assert_eq!((st.messages, st.corrections), (1, 1));
}

#[test]
fn unreadable_files_are_reported_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    let ts = Utc::now().to_rfc3339();
    std::fs::write(dir.path().join("a.jsonl"), cc_correction("u", &ts)).unwrap();
    let mut s = Store::open_in_memory().unwrap();
    s.import("chatgpt-export", &dir.path().join("conversations.json").to_string_lossy()).unwrap();
    std::fs::write(dir.path().join("conversations.json"), "{not json").unwrap();
    s.allow("claude-code", None).unwrap();
    let mut all = sources::all();
    all[0] = sources::claude_code_at(dir.path().to_path_buf());
    let r = sync(&mut s, &all).unwrap();
    assert_eq!(r.corrections, 1, "the good file still counts");
    assert_eq!(r.errors.len(), 1);
    assert!(r.errors[0].contains("conversations.json"), "{:?}", r.errors);
}

#[test]
fn paused_and_revoked_sources_are_not_read() {
    let dir = tempfile::tempdir().unwrap();
    let ts = Utc::now().to_rfc3339();
    std::fs::write(dir.path().join("a.jsonl"), cc_correction("u", &ts)).unwrap();
    let all = claude_code(&dir);
    let mut s = Store::open_in_memory().unwrap();
    s.allow("claude-code", None).unwrap();
    s.set_paused("claude-code", true).unwrap();
    assert_eq!(sync(&mut s, &all).unwrap().files_read, 0);
    s.set_paused("claude-code", false).unwrap();
    s.revoke("claude-code").unwrap();
    assert_eq!(sync(&mut s, &all).unwrap().files_read, 0);
    s.allow("claude-code", None).unwrap();
    assert_eq!(sync(&mut s, &all).unwrap().corrections, 1);
}

#[test]
fn rule_changes_rebuild_scores_but_keep_consent() {
    let dir = tempfile::tempdir().unwrap();
    let ts = (Utc::now() - Duration::hours(1)).to_rfc3339();
    std::fs::write(dir.path().join("a.jsonl"), cc_ai("<synthetic>", &ts, "API Error") + &cc_correction("u", &ts))
        .unwrap();
    let all = claude_code(&dir);
    let mut s = Store::open_in_memory().unwrap();
    s.allow("claude-code", None).unwrap();
    assert!(!sync(&mut s, &all).unwrap().rescored, "a fresh store has nothing to rebuild");

    // Pretend these scores came from older rules.
    s.set_setting("rules_version", "1").unwrap();
    let r = sync(&mut s, &all).unwrap();
    assert!(r.rescored);
    assert_eq!(r.corrections, 1, "history was re-read");
    assert!(s.source_state("claude-code").unwrap().unwrap().allowed, "consent survives");
    let st = stats(&s);
    assert_eq!(st.corrections, 1);
    assert_eq!(
        (st.models[0].family.as_str(), st.models[0].version.as_str()),
        ("Claude Opus", "5.5"),
        "never the placeholder"
    );
    assert!(!sync(&mut s, &all).unwrap().rescored, "only once");
}

#[test]
fn from_now_on_skips_existing_history_but_counts_new_messages() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.jsonl");
    // Written earlier today: must not count when the person picks "From now on".
    std::fs::write(&path, cc_correction("old", &(Utc::now() - Duration::minutes(30)).to_rfc3339())).unwrap();
    let all = claude_code(&dir);
    let mut s = Store::open_in_memory().unwrap();
    s.allow("claude-code", None).unwrap();
    assert_eq!(s.start_from_now(all[0].as_ref()).unwrap(), 1);
    assert_eq!(sync(&mut s, &all).unwrap().messages, 0);

    append(&path, &cc_correction("new", &Utc::now().to_rfc3339()));
    let r = sync(&mut s, &all).unwrap();
    assert_eq!((r.messages, r.corrections), (1, 1), "only the new message counts");
}

#[test]
fn allowing_more_history_re_reads_old_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.jsonl");
    std::fs::write(&path, cc_correction("old", &(Utc::now() - Duration::hours(3)).to_rfc3339())).unwrap();
    let all = claude_code(&dir);
    let mut s = Store::open_in_memory().unwrap();
    s.allow("claude-code", None).unwrap();
    s.start_from_now(all[0].as_ref()).unwrap();
    assert_eq!(sync(&mut s, &all).unwrap().messages, 0);

    // "All history" reaches back past the EOF checkpoint, so the old message is found.
    s.allow("claude-code", None).unwrap();
    assert_eq!(sync(&mut s, &all).unwrap().messages, 1);

    // Allowing the same window again keeps checkpoints: nothing is re-read.
    s.allow("claude-code", None).unwrap();
    assert_eq!(sync(&mut s, &all).unwrap().files_read, 0);
}

#[test]
fn from_now_on_survives_a_rules_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.jsonl");
    std::fs::write(&path, cc_correction("old", &(Utc::now() - Duration::hours(3)).to_rfc3339())).unwrap();
    let all = claude_code(&dir);
    let mut s = Store::open_in_memory().unwrap();
    s.allow("claude-code", None).unwrap();
    s.start_from_now(all[0].as_ref()).unwrap();
    append(&path, &cc_correction("new", &Utc::now().to_rfc3339()));
    sync(&mut s, &all).unwrap();
    assert_eq!(stats(&s).messages, 1);

    // A later release changes the rules: scores rebuild from disk, and the old message stays out.
    s.set_setting("rules_version", "1").unwrap();
    assert!(sync(&mut s, &all).unwrap().rescored);
    assert_eq!(stats(&s).messages, 1, "still only the message after the cutoff");

    // Choosing "All history" later clears the cutoff.
    s.allow("claude-code", None).unwrap();
    assert!(s.source_state("claude-code").unwrap().unwrap().start_after.is_none());
}

#[test]
fn db_path_prefers_the_new_variable() {
    let env = |vars: &'static [(&'static str, &'static str)]| {
        move |k: &str| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| OsString::from(v))
    };
    // Absolute on every OS (on Windows "/data" has no drive letter, so it isn't).
    let dir = std::env::temp_dir();
    let data = Some(dir.clone());
    assert_eq!(resolve_db_path(env(&[]), data.clone()).unwrap(), dir.join("app.noactually").join("noactually.db"));
    assert_eq!(resolve_db_path(env(&[("WA_DB", "/old.db")]), data.clone()).unwrap(), PathBuf::from("/old.db"));
    let both = env(&[("WA_DB", "/old.db"), ("NOACTUALLY_DB", "/new.db")]);
    assert_eq!(resolve_db_path(both, data.clone()).unwrap(), PathBuf::from("/new.db"));
    let empty_new = env(&[("WA_DB", "/old.db"), ("NOACTUALLY_DB", "")]);
    assert_eq!(resolve_db_path(empty_new, data).unwrap(), PathBuf::from("/old.db"), "empty means unset");
    assert!(matches!(resolve_db_path(env(&[]), None), Err(Error::NoDataDir)));
    assert!(matches!(resolve_db_path(env(&[]), Some("relative".into())), Err(Error::NoDataDir)), "never relative");
}

#[test]
fn pins_survive_a_rules_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let ts = (Utc::now() - Duration::hours(1)).to_rfc3339();
    let file = dir.path().join("a.jsonl");
    let correction = |n: &str| cc_ai("claude-opus-5-5", &ts, "hi") + &cc_correction(n, &ts);
    std::fs::write(&file, correction("u1") + &correction("u2")).unwrap();
    let all = claude_code(&dir);
    let mut s = Store::open_in_memory().unwrap();
    s.allow("claude-code", None).unwrap();
    sync(&mut s, &all).unwrap();
    let pinned_id: i64 = s.conn.query_row("SELECT min(id) FROM events", [], |r| r.get(0)).unwrap();
    s.pin(pinned_id, true).unwrap();
    let pins = |s: &Store| -> (i64, i64) {
        s.conn
            .query_row("SELECT count(*), coalesce(sum(pinned), 0) FROM events", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
    };

    s.set_setting("rules_version", "1").unwrap(); // pretend older rules
    assert!(sync(&mut s, &all).unwrap().rescored);
    assert_eq!(pins(&s), (2, 1), "both corrections are back, and the pinned one isn't counted twice");
    let still: i64 = s.conn.query_row("SELECT id FROM events WHERE pinned = 1", [], |r| r.get(0)).unwrap();
    assert_eq!(still, pinned_id, "the pinned correction was kept, not re-created");

    std::fs::remove_file(&file).unwrap(); // the tool cleaned up old history
    s.set_setting("rules_version", "1").unwrap();
    assert!(sync(&mut s, &all).unwrap().rescored);
    assert_eq!(pins(&s), (1, 1), "only the pinned correction outlives its deleted history file");
    let text: Option<String> = s.conn.query_row("SELECT human_snip FROM events", [], |r| r.get(0)).unwrap();
    assert!(text.is_some(), "and it keeps its text");

    s.forget("claude-code").unwrap();
    assert_eq!(pins(&s), (0, 0), "forgetting a source deletes its pinned corrections too");
}

#[test]
fn an_unreadable_retention_setting_keeps_no_text() {
    let mut s = Store::open_in_memory().unwrap();
    assert_eq!(s.retention().unwrap(), Retention::default());
    for bad in ["not json", r#"{"level":"full","full_days":0,"snippet_days":9}"#, r#"{"level":"everything"}"#] {
        s.set_setting("retention", bad).unwrap();
        assert_eq!(s.retention().unwrap().level, Level::Category, "{bad}");
    }
    ingest(&mut s, "f", vec![turn("no, that's wrong", "You're right.", "m", &now_ts(1), "k")]);
    let text: Option<String> = s.conn.query_row("SELECT human_snip FROM events", [], |r| r.get(0)).unwrap();
    assert!(text.is_none(), "new corrections keep no text while the setting is unreadable");
}

#[test]
fn an_unreadable_retention_setting_never_deletes_stored_text() {
    let mut s = Store::open_in_memory().unwrap();
    s.set_retention(Retention { level: Level::Full, ..Retention::default() }).unwrap();
    ingest(&mut s, "f", vec![turn("no, that's wrong", "You're right.", "m", &now_ts(1), "k")]);
    s.set_setting("retention", r#"{"level":"everything"}"#).unwrap(); // e.g. saved by a newer version
    assert_eq!(s.sweep(Utc::now()).unwrap(), 0);
    let (level, text): (String, Option<String>) =
        s.conn.query_row("SELECT level, human_full FROM events", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(level, "full");
    assert!(text.is_some(), "text the user chose to keep stays until the setting is readable again");
}

#[test]
fn flush_deleted_truncates_the_log() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(&dir.path().join("t.db")).unwrap();
    s.set_setting("a", "b").unwrap();
    s.flush_deleted().unwrap();
    let wal = std::fs::metadata(dir.path().join("t.db-wal")).map_or(0, |m| m.len());
    assert_eq!(wal, 0);
}

/// A chat's first message (no AI reply before it) in chat `session`.
fn opener(human: &str, session: &str, ts: &str, key: &str) -> SourceTurn {
    let mut t = turn(human, "On it.", "claude-opus-5-5", ts, key);
    t.session = session.into();
    t.turn.prev_ai = None;
    t
}

fn reply(human: &str, session: &str, ts: &str, key: &str) -> SourceTurn {
    SourceTurn { session: session.into(), ..turn(human, "Okay.", "claude-opus-5-5", ts, key) }
}

#[test]
fn prompts_count_tasks_signals_and_steering() {
    let mut s = Store::open_in_memory().unwrap();
    let t = now_ts(0);
    let sharp = "In src/app.ts the button should open settings. Don't touch the router.";
    ingest(
        &mut s,
        "f",
        vec![
            opener("fix it", "a", &t, "1"),
            reply("also, it needs to work offline", "a", &t, "2"),
            reply("thanks", "a", &t, "3"),
            opener(sharp, "b", &t, "4"),
            reply("still not working", "b", &t, "5"),
            reply("great, now add a CSV export", "b", &t, "6"),
            opener("what does this do?", "c", &t, "7"),
            reply("I meant the other file", "c", &t, "8"), // no task in this chat: not counted
        ],
    );
    let b = stats(&s).prompts;
    // The CSV export got no reply yet, so it can't have needed steering: it doesn't count either way.
    assert_eq!((b.tasks, b.continued, b.landed), (3, 2, 1), "only the added requirement counts against you");
    assert_eq!(b.grades, BTreeMap::from([("thin".into(), 2), ("sharp".into(), 1)]));
    assert_eq!(b.signals, BTreeMap::from([("spot".into(), 1), ("done".into(), 1), ("limits".into(), 1)]));
    let steering: BTreeMap<String, i64> =
        [("ai_wrong", 1), ("added", 1), ("clarified", 0), ("redo", 0)].map(|(k, n)| (k.into(), n)).into();
    assert_eq!(b.steering, steering);
    assert_eq!((b.window_tasks, b.tier, b.to_rank), (2, None, 8), "too few tasks to rank");
    let models = stats(&s).models;
    let sum = |f: fn(&ModelRow) -> i64| models.iter().map(f).sum::<i64>();
    assert_eq!(
        (sum(|m| m.tasks), sum(|m| m.continued), sum(|m| m.landed)),
        (3, 2, 1),
        "the picker's per-model prompts"
    );
    let msgs: Vec<i64> = s
        .conn()
        .prepare("SELECT messages FROM tasks ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(msgs, [3, 2, 1], "each chat's replies count toward its latest task");
}

#[test]
fn prompt_tiers_come_from_the_last_28_days_of_tasks_that_continued() {
    let mut s = Store::open_in_memory().unwrap();
    let (now, old) = (now_ts(0), now_ts(40));
    let mut turns = Vec::new();
    for i in 0..12 {
        turns.push(opener("add a CSV export", &format!("n{i}"), &now, &format!("n{i}")));
        turns.push(reply("thanks", &format!("n{i}"), &now, &format!("nr{i}")));
    }
    turns.push(reply("I forgot to mention it should be gzipped", "n0", &now, "x"));
    // One-shot tasks can't need steering, so they don't pad the score.
    for i in 0..3 {
        turns.push(opener("add a CSV export", &format!("q{i}"), &now, &format!("q{i}")));
    }
    // Tasks from before the window don't drag it down.
    for i in 0..5 {
        turns.push(opener("add a CSV export", &format!("o{i}"), &old, &format!("o{i}")));
        turns.push(reply("scrap that", &format!("o{i}"), &old, &format!("ox{i}")));
    }
    ingest(&mut s, "f", turns);
    let b = stats(&s).prompts;
    assert_eq!((b.tasks, b.continued, b.landed), (20, 17, 11));
    assert_eq!((b.window_tasks, b.window_landed), (12, 11));
    assert_eq!((b.tier, b.next_tier, b.to_rank), (Some(PromptTier::Architect), None, 0));
    let tip = b.tip.unwrap();
    assert_eq!((tip.signal.as_str(), tip.pct_of_prompts, tip.landed_with_without), ("spot", 0.0, None));
}

#[test]
fn a_tip_quotes_the_gap_only_when_history_backs_it() {
    let mut s = Store::open_in_memory().unwrap();
    let t = now_ts(0);
    let mut turns = Vec::new();
    // Prompts with a purpose always land; those that don't are steered half the time.
    for i in 0..30 {
        let why = i < 10;
        let human = if why { "add a CSV export because finance needs it" } else { "add a CSV export" };
        turns.push(opener(human, &format!("c{i}"), &t, &format!("c{i}")));
        if !why && i % 2 == 0 {
            turns.push(reply("oh and make it gzipped", &format!("c{i}"), &t, &format!("x{i}")));
        }
    }
    ingest(&mut s, "f", turns);
    let tip = stats(&s).prompts.tip.unwrap();
    // "spot" is carried least (never), so it's the tip; "why" would be the one with a real gap.
    assert_eq!(tip.signal, "spot");
    assert_eq!(tip.landed_with_without, None, "no prompts carried it, so there's nothing to compare");
    let groups =
        [(Signals(Signal::Why.bit()), true, true, 10), (Signals(0), true, true, 10), (Signals(0), true, false, 10)];
    let why_tip = super::stats::tip_for(&groups, Signal::Why);
    assert_eq!(why_tip.landed_with_without, Some((100.0, 50.0)));
    let small = [(Signals(Signal::Why.bit()), true, true, 9), (Signals(0), true, false, 10)];
    assert_eq!(super::stats::tip_for(&small, Signal::Why).landed_with_without, None, "too few to say");
}

#[test]
fn prompts_follow_the_filter_and_are_forgotten_and_rebuilt() {
    let mut s = Store::open_in_memory().unwrap();
    let t = now_ts(0);
    let mut gpt = opener("add a CSV export", "g", &t, "g");
    gpt.turn.next_ai.as_mut().unwrap().model = Some("gpt-5.1".into());
    ingest(&mut s, "f", vec![opener("add a CSV export", "a", &t, "a"), gpt]);
    let only = |provider: &str| Filter { provider: Some(provider.into()), ..Default::default() };
    assert_eq!(s.stats(&only("OpenAI"), today()).unwrap().prompts.tasks, 1);
    assert_eq!(stats(&s).prompts.tasks, 2);
    s.reset_scores().unwrap();
    assert_eq!(stats(&s).prompts.tasks, 0, "a rules rebuild clears tasks with the other scores");
    ingest(&mut s, "f", vec![opener("add a CSV export", "a", &t, "a")]);
    s.forget("test").unwrap();
    let b = stats(&s).prompts;
    assert_eq!((b.tasks, b.to_rank, b.steering.values().sum::<i64>(), b.tip), (0, 10, 0, None));
}

#[test]
fn every_steer_cause_is_a_tasks_column() {
    let s = Store::open_in_memory().unwrap();
    for c in engine::Steer::ALL {
        s.conn().execute(&format!("UPDATE tasks SET {0} = {0} + 1", c.as_str()), []).unwrap();
    }
}

#[test]
fn live_tracking_reads_prompts_like_a_full_rebuild() {
    // Synced after every reply, as the app does: each follow-up must still know what it answered.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.jsonl");
    let ts = Utc::now().to_rfc3339();
    let steps = [
        cc_user("u1", &ts, "add a CSV export to src/report.ts") + &cc_ai("claude-opus-5-5", &ts, "Done."),
        cc_user("u2", &ts, "still not working") + &cc_ai("claude-opus-5-5", &ts, "Fixed the header row."),
        cc_user("u3", &ts, "also, it needs to work offline") + &cc_ai("claude-opus-5-5", &ts, "Added a cache."),
        cc_user("u4", &ts, "thanks") + &cc_ai("claude-opus-5-5", &ts, "Anytime."),
    ];
    std::fs::write(&path, "").unwrap();
    let mut live = Store::open_in_memory().unwrap();
    let all = claude_code(&dir);
    live.allow("claude-code", None).unwrap();
    for step in &steps {
        append(&path, step);
        sync(&mut live, &all).unwrap();
    }
    let mut whole = Store::open_in_memory().unwrap();
    whole.allow("claude-code", None).unwrap();
    sync(&mut whole, &all).unwrap();
    for b in [stats(&live).prompts, stats(&whole).prompts] {
        assert_eq!((b.tasks, b.landed), (1, 0));
        assert_eq!((b.steering["ai_wrong"], b.steering["added"]), (1, 1));
    }
}

#[test]
fn no_tip_once_every_signal_is_common() {
    let mut s = Store::open_in_memory().unwrap();
    let t = now_ts(0);
    let all_five = "In `src/a.ts` it says \"Bad thing happened\" but should return 200. Don't touch the API because our users rely on it.";
    ingest(&mut s, "f", (0..4).map(|i| opener(all_five, &format!("c{i}"), &t, &format!("c{i}"))).collect());
    let b = stats(&s).prompts;
    assert_eq!(b.grades.get("sharp"), Some(&4));
    assert_eq!(b.tip, None);
}

#[test]
fn a_window_moves_the_counts_and_the_score() {
    let s = Store::open_in_memory().unwrap();
    let day = |ago: i64| (today() - chrono::Duration::days(ago)).to_string();
    let count = |ago: i64, kind: &str, n: i64| {
        s.conn()
            .execute(
                "INSERT INTO daily (day, source, provider, family, version, kind, category, n)
                 VALUES (?1, 'claude-code', 'Anthropic', 'Claude Opus', '5.5', ?2, ?3, ?4)",
                rusqlite::params![day(ago), kind, if kind == "message" { "" } else { "made_it_up" }, n],
            )
            .unwrap();
    };
    for ago in [1, 20, 200] {
        count(ago, "message", 10);
        count(ago, "confirmed", 2);
    }
    for ago in [2, 3, 15, 16, 17, 18, 19, 21, 22, 23, 24, 300] {
        s.conn()
            .execute(
                "INSERT INTO tasks (day, source, session, provider, family, version, signals, messages)
                 VALUES (?1, 'claude-code', 's', 'Anthropic', 'Claude Opus', '5.5', 4, 2)",
                [day(ago)],
            )
            .unwrap();
    }
    let all = s.stats(&Filter::default(), today()).unwrap();
    let week =
        s.stats(&Filter { since: Some(today() - chrono::Duration::days(6)), ..Default::default() }, today()).unwrap();
    assert_eq!((all.messages, all.corrections, all.prompts.tasks), (30, 6, 12));
    assert_eq!((week.messages, week.corrections, week.prompts.tasks), (10, 2, 2), "counts follow the window");
    assert_eq!(week.weekly.len(), 1);
    // Tasks land in the week they started; all 12 here continued and nothing steered them.
    let continued: i64 = all.weekly.iter().map(|w| w.continued).sum();
    let landed: i64 = all.weekly.iter().map(|w| w.landed).sum();
    assert_eq!((continued, landed), (12, 12), "weekly tasks add up to the scorecard's");
    // The score follows the window too: 2 tasks this week, too few to score.
    assert_eq!((week.prompts.window_tasks, week.overall.score, week.overall.corrections), (2, None, 2));
    assert_eq!(all.prompts.window_tasks, 11, "no window: the last 28 days");
    let year =
        s.stats(&Filter { since: Some(today() - chrono::Duration::days(364)), ..Default::default() }, today()).unwrap();
    assert_eq!((year.prompts.window_tasks, year.overall.corrections), (12, 6));
    assert_eq!(week.corrections_last_7d, all.corrections_last_7d, "this week never moves");
}

#[test]
fn the_hall_of_fame_keeps_the_best_few_per_award() {
    let s = Store::open_in_memory().unwrap();
    let event = |id: i64,
                 label: &str,
                 category: &str,
                 human: Option<&str>,
                 ai: Option<&str>,
                 pinned: bool,
                 ts: &str| {
        s.conn()
            .execute(
                "INSERT INTO events (id, ts, day, source, session, provider, family, version, label, category, rules, level,
                   human_snip, ai_snip, pinned, seen_key)
                 VALUES (?1, ?2, substr(?2, 1, 10), 'claude-code', 's', 'Anthropic', 'Claude Opus', '5.5', ?3, ?4, '[]',
                   'snippet', ?5, ?6, ?7, ?1)",
                rusqlite::params![id, ts, label, category, human, ai, pinned],
            )
            .unwrap();
    };
    let punchy = "that endpoint doesn't exist, you made it up out of thin air";
    event(1, "claimed", "made_it_up", Some(punchy), Some("Let me check."), false, "2026-10-20T10:00:00Z");
    event(2, "confirmed", "made_it_up", Some(punchy), Some("You're right."), false, "2026-10-01T10:00:00Z");
    event(3, "confirmed", "made_it_up", Some("no"), Some("You're right."), false, "2026-10-21T10:00:00Z");
    event(4, "claimed", "made_it_up", Some("meh"), None, true, "2026-09-01T10:00:00Z");
    for id in 10..20 {
        event(id, "claimed", "still_broken", Some("still broken"), None, false, "2026-10-10T10:00:00Z");
    }
    event(
        30,
        "humbled",
        "",
        Some("use the old flag"),
        Some("Actually, that flag was removed."),
        false,
        "2026-10-22T10:00:00Z",
    );
    let hall = stats(&s).hall_of_fame;
    let ids = |cat: &str| hall.iter().filter(|e| e.category == cat).map(|e| e.id).collect::<Vec<_>>();
    // Pinned first; then admitted beats claimed; then the punchier line; then the newest.
    assert_eq!(ids("made_it_up"), [4, 2, 3, 1]);
    assert_eq!(ids("still_broken").len(), 5, "the best few per award, not all of them");
    assert_eq!(ids(""), [30], "being humbled is an award too");
}
