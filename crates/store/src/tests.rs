//! The store's behavior suite: ingest, sync, consent, retention and stats, end to end.

use super::*;
use engine::{Answer, Tier};
use sources::testing::{cc_ai, cc_catch, cc_user, set_mtime_ago};
use sources::{FileScan, SourceTurn};
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
    FileScan { turns, checkpoint: 100 }
}

fn test_source() -> SourceState {
    SourceState { allowed: true, ..SourceState::new("test") }
}

/// Ingests `turns` as file `path` of the "test" source with no consent limits.
fn ingest(s: &mut Store, path: &str, turns: Vec<SourceTurn>) {
    s.ingest(&test_source(), path, &scan(turns), FileMeta { size: 100, mtime: 1 }).unwrap();
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
fn counts_messages_catches_and_models() {
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
    assert_eq!((all.messages, all.catches, all.catches_last_7d), (3, 2, 2));
    assert_eq!(all.categories.get("made_it_up"), Some(&1));
    assert_eq!((all.tier, all.next_tier, all.verdict), (Tier::RubberStamp, Some((Tier::Coworker, 1)), Answer::Hmm));

    let opus = s
        .stats(
            &Filter { family: Some("Claude Opus".into()), version: Some("5.5".into()), ..Default::default() },
            today(),
        )
        .unwrap();
    assert_eq!((opus.messages, opus.catches), (2, 1));
    let family = s.stats(&Filter { family: Some("Claude Opus".into()), ..Default::default() }, today()).unwrap();
    assert_eq!(family.messages, 2);
    let none = s.stats(&Filter { version: Some("9.9".into()), ..Default::default() }, today()).unwrap();
    assert_eq!((none.messages, none.first_day), (0, None));
    let openai = s.stats(&Filter { provider: Some("OpenAI".into()), ..Default::default() }, today()).unwrap();
    assert_eq!((openai.messages, openai.catches), (1, 1));
}

#[test]
fn stats_json_matches_the_ui_contract() {
    let mut s = Store::open_in_memory().unwrap();
    let t = now_ts(0);
    let catches: Vec<_> =
        (0..7).map(|i| turn("that's not a real page", "You're right.", "m", &t, &i.to_string())).collect();
    ingest(&mut s, "f", catches);
    let v = serde_json::to_value(stats(&s)).unwrap();
    assert_eq!(v["tier"], "sharp_eyed");
    assert_eq!(v["next_tier"], serde_json::json!(["irreplaceable", 8]));
    assert_eq!(v["verdict"], "no");
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
    s.ingest(&state, "f", &scan(vec![t]), FileMeta::default()).unwrap();
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
    assert_eq!(
        w,
        vec![
            WeekRow { week: d("2026-10-19"), messages: 1, catches: 1 },
            WeekRow { week: d("2026-10-26"), messages: 1, catches: 0 }
        ]
    );
}

#[test]
fn streaks_and_peak_hour_come_from_stored_catches() {
    let mut s = Store::open_in_memory().unwrap();
    let at = |days_ago: i64| (Utc::now() - Duration::days(days_ago)).to_rfc3339();
    let catches = [0, 1, 2, 5].map(|d| turn("no, that's wrong", "You're right.", "m", &at(d), &d.to_string()));
    ingest(&mut s, "f", catches.into());
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
    assert_eq!(stats(&s).catches, 1);
}

#[test]
fn backfill_window_is_respected() {
    let mut s = Store::open_in_memory().unwrap();
    let state = SourceState { backfill_since: Some(today() - Duration::days(7)), ..test_source() };
    let old = turn("no, that's wrong", "You're right.", "m", &now_ts(40), "x");
    s.ingest(&state, "f", &scan(vec![old.clone()]), FileMeta::default()).unwrap();
    assert_eq!(stats(&s).messages, 0);

    // Skipped messages aren't marked as seen, so allowing all history later still counts them.
    s.ingest(&test_source(), "f", &scan(vec![old]), FileMeta::default()).unwrap();
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
    assert_eq!(stats(&s).catches, 1, "still counted");
}

#[test]
fn retention_is_validated() {
    let s = Store::open_in_memory().unwrap();
    let bad = Retention { full_days: 0, ..Retention::default() };
    assert_eq!(bad.validate().unwrap_err().to_string(), "Keep text for at least 1 day.");
    assert!(s.set_retention(bad).is_err());
    assert_eq!(s.retention().unwrap(), Retention::default(), "nothing saved");
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
    assert_eq!((r.files_read, r.catches), (1, 1));
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
    assert_eq!((r.messages, r.catches), (1, 1));
    let again = sync(&mut s, &all).unwrap();
    assert_eq!((again.files_read, again.messages), (0, 0));
    assert_eq!(stats(&s).catches, 1);
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
    assert_eq!((r.messages, r.catches), (1, 1), "a quiet file is final");
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
    assert_eq!((st.messages, st.catches), (1, 1));
}

#[test]
fn unreadable_files_are_reported_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    let ts = Utc::now().to_rfc3339();
    std::fs::write(dir.path().join("a.jsonl"), cc_catch("u", &ts)).unwrap();
    let mut s = Store::open_in_memory().unwrap();
    s.import("chatgpt-export", &dir.path().join("conversations.json").to_string_lossy()).unwrap();
    std::fs::write(dir.path().join("conversations.json"), "{not json").unwrap();
    s.allow("claude-code", None).unwrap();
    let mut all = sources::all();
    all[0] = sources::claude_code_at(dir.path().to_path_buf());
    let r = sync(&mut s, &all).unwrap();
    assert_eq!(r.catches, 1, "the good file still counts");
    assert_eq!(r.errors.len(), 1);
    assert!(r.errors[0].contains("conversations.json"), "{:?}", r.errors);
}

#[test]
fn paused_and_revoked_sources_are_not_read() {
    let dir = tempfile::tempdir().unwrap();
    let ts = Utc::now().to_rfc3339();
    std::fs::write(dir.path().join("a.jsonl"), cc_catch("u", &ts)).unwrap();
    let all = claude_code(&dir);
    let mut s = Store::open_in_memory().unwrap();
    s.allow("claude-code", None).unwrap();
    s.set_paused("claude-code", true).unwrap();
    assert_eq!(sync(&mut s, &all).unwrap().files_read, 0);
    s.set_paused("claude-code", false).unwrap();
    s.revoke("claude-code").unwrap();
    assert_eq!(sync(&mut s, &all).unwrap().files_read, 0);
    s.allow("claude-code", None).unwrap();
    assert_eq!(sync(&mut s, &all).unwrap().catches, 1);
}

#[test]
fn rule_changes_rebuild_scores_but_keep_consent() {
    let dir = tempfile::tempdir().unwrap();
    let ts = (Utc::now() - Duration::hours(1)).to_rfc3339();
    std::fs::write(dir.path().join("a.jsonl"), cc_ai("<synthetic>", &ts, "API Error") + &cc_catch("u", &ts)).unwrap();
    let all = claude_code(&dir);
    let mut s = Store::open_in_memory().unwrap();
    s.allow("claude-code", None).unwrap();
    assert!(!sync(&mut s, &all).unwrap().rescored, "a fresh store has nothing to rebuild");

    // Pretend these scores came from older rules.
    s.set_setting("rules_version", "1").unwrap();
    let r = sync(&mut s, &all).unwrap();
    assert!(r.rescored);
    assert_eq!(r.catches, 1, "history was re-read");
    assert!(s.source_state("claude-code").unwrap().unwrap().allowed, "consent survives");
    let st = stats(&s);
    assert_eq!(st.catches, 1);
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
    std::fs::write(&path, cc_catch("old", &(Utc::now() - Duration::minutes(30)).to_rfc3339())).unwrap();
    let all = claude_code(&dir);
    let mut s = Store::open_in_memory().unwrap();
    s.allow("claude-code", None).unwrap();
    assert_eq!(s.start_from_now(all[0].as_ref()).unwrap(), 1);
    assert_eq!(sync(&mut s, &all).unwrap().messages, 0);

    append(&path, &cc_catch("new", &Utc::now().to_rfc3339()));
    let r = sync(&mut s, &all).unwrap();
    assert_eq!((r.messages, r.catches), (1, 1), "only the new message counts");
}

#[test]
fn from_now_on_survives_a_rules_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.jsonl");
    std::fs::write(&path, cc_catch("old", &(Utc::now() - Duration::hours(3)).to_rfc3339())).unwrap();
    let all = claude_code(&dir);
    let mut s = Store::open_in_memory().unwrap();
    s.allow("claude-code", None).unwrap();
    s.start_from_now(all[0].as_ref()).unwrap();
    append(&path, &cc_catch("new", &Utc::now().to_rfc3339()));
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
    let data = Some(PathBuf::from("/data"));
    assert_eq!(resolve_db_path(env(&[]), data.clone()).unwrap(), PathBuf::from("/data/app.noactually/noactually.db"));
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
    let catch = |n: &str| cc_ai("claude-opus-5-5", &ts, "hi") + &cc_catch(n, &ts);
    std::fs::write(&file, catch("u1") + &catch("u2")).unwrap();
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
    assert_eq!(pins(&s), (2, 1), "both catches are back, and the pinned one isn't counted twice");
    let still: i64 = s.conn.query_row("SELECT id FROM events WHERE pinned = 1", [], |r| r.get(0)).unwrap();
    assert_eq!(still, pinned_id, "the pinned catch was kept, not re-created");

    std::fs::remove_file(&file).unwrap(); // the tool cleaned up old history
    s.set_setting("rules_version", "1").unwrap();
    assert!(sync(&mut s, &all).unwrap().rescored);
    assert_eq!(pins(&s), (1, 1), "only the pinned catch outlives its deleted history file");
    let text: Option<String> = s.conn.query_row("SELECT human_snip FROM events", [], |r| r.get(0)).unwrap();
    assert!(text.is_some(), "and it keeps its text");

    s.forget("claude-code").unwrap();
    assert_eq!(pins(&s), (0, 0), "forgetting a source deletes its pinned catches too");
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
    assert!(text.is_none(), "new catches keep no text while the setting is unreadable");
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
