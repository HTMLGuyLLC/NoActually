use super::*;
use sources::{FileScan, SourceTurn};
use std::io::Write;

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

fn today() -> NaiveDate {
    Local::now().date_naive()
}

fn now_ts(days_ago: i64) -> String {
    (Utc::now() - Duration::days(days_ago)).to_rfc3339()
}

#[test]
fn counts_messages_catches_and_models() {
    let mut s = Store::open_in_memory().unwrap();
    let t = now_ts(0);
    s.ingest(
        "test",
        "f",
        &scan(vec![
            turn("that's not a real page", "You're right, it doesn't exist.", "claude-opus-5-5", &t, "a"),
            turn("thanks", "Anytime.", "claude-opus-5-5", &t, "b"),
            turn("still not working", "Checking the logs.", "gpt-5.1", &t, "c"),
        ]),
        100,
        1,
        None,
    )
    .unwrap();
    let all = s.stats(&Filter::default(), today()).unwrap();
    assert_eq!((all.messages, all.catches, all.catches_last_7d), (3, 2, 2));
    assert_eq!(all.categories.get("made_it_up"), Some(&1));

    let opus = s
        .stats(
            &Filter { family: Some("Claude Opus".into()), version: Some("5.5".into()), ..Default::default() },
            today(),
        )
        .unwrap();
    assert_eq!((opus.messages, opus.catches), (2, 1));
    let openai = s.stats(&Filter { provider: Some("OpenAI".into()), ..Default::default() }, today()).unwrap();
    assert_eq!((openai.messages, openai.catches), (1, 1));
}

#[test]
fn copies_are_never_counted_twice() {
    let mut s = Store::open_in_memory().unwrap();
    let t = now_ts(0);
    let one = || scan(vec![turn("no, that's wrong", "You're right.", "claude-opus-5-5", &t, "same-id")]);
    s.ingest("test", "f1", &one(), 100, 1, None).unwrap();
    s.ingest("test", "f2", &one(), 100, 1, None).unwrap();
    assert_eq!(s.stats(&Filter::default(), today()).unwrap().catches, 1);
}

#[test]
fn backfill_window_is_respected() {
    let mut s = Store::open_in_memory().unwrap();
    let old = now_ts(40);
    let since = Some(today() - Duration::days(7));
    s.ingest("test", "f", &scan(vec![turn("no, that's wrong", "You're right.", "m", &old, "x")]), 1, 1, since).unwrap();
    assert_eq!(s.stats(&Filter::default(), today()).unwrap().messages, 0);
}

#[test]
fn retention_steps_down_and_pins_hold() {
    let mut s = Store::open_in_memory().unwrap();
    s.set_retention(Retention { level: Level::Full, full_days: 30, snippet_days: 365 }).unwrap();
    s.ingest(
        "test",
        "f",
        &scan(vec![
            turn("no, that's wrong. Email me at pat@example.com", "You're right, my mistake.", "m", &now_ts(1), "new"),
            turn("that's not a real page", "You're right.", "m", &now_ts(60), "mid"),
            turn("you missed the tests", "My mistake.", "m", &now_ts(400), "old"),
            turn("you forgot the docs", "My mistake.", "m", &now_ts(400), "pinned"),
        ]),
        1,
        1,
        None,
    )
    .unwrap();
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

    // Lowering the level applies to what's already stored.
    s.set_retention(Retention { level: Level::Category, full_days: 30, snippet_days: 365 }).unwrap();
    assert_eq!(level("new"), ("category".into(), None, None));
}

#[test]
fn forget_deletes_everything_for_a_source() {
    let mut s = Store::open_in_memory().unwrap();
    s.allow("test", None).unwrap();
    s.ingest("test", "f", &scan(vec![turn("no, that's wrong", "You're right.", "m", &now_ts(0), "k")]), 1, 1, None)
        .unwrap();
    s.forget("test").unwrap();
    let left: i64 = s
        .conn
        .query_row("SELECT (SELECT COUNT(*) FROM events)+(SELECT COUNT(*) FROM daily)+(SELECT COUNT(*) FROM seen)+(SELECT COUNT(*) FROM files)+(SELECT COUNT(*) FROM sources)", [], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 0);
}

#[test]
fn nothing_is_read_without_consent_and_live_appends_count_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("abc.jsonl");
    let line = |v: serde_json::Value| format!("{v}\n");
    let ts = Utc::now().to_rfc3339();
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(line(serde_json::json!({"type":"assistant","timestamp":ts,"message":{"model":"claude-opus-5-5","content":[{"type":"text","text":"Use /admin."}]}})).as_bytes()).unwrap();
    f.write_all(line(serde_json::json!({"type":"user","uuid":"u1","timestamp":ts,"message":{"content":"that's not a real page"}})).as_bytes()).unwrap();

    let mut s = Store::open_in_memory().unwrap();
    let all = vec![sources::claude_code_at(dir.path().to_path_buf())];
    // Not allowed yet: nothing read.
    assert_eq!(sync(&mut s, &all).unwrap().files_read, 0);

    s.allow("claude-code", None).unwrap();
    let r = sync(&mut s, &all).unwrap();
    assert_eq!((r.files_read, r.messages), (1, 0), "the message waits for its reply");

    f.write_all(line(serde_json::json!({"type":"assistant","timestamp":ts,"message":{"model":"claude-opus-5-5","content":[{"type":"text","text":"You're right, my mistake."}]}})).as_bytes()).unwrap();
    f.flush().unwrap();
    // Make sure the mtime/size change is visible even on coarse filesystems.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let r = sync(&mut s, &all).unwrap();
    assert_eq!((r.messages, r.catches), (1, 1));
    let again = sync(&mut s, &all).unwrap();
    assert_eq!(again.messages, 0);
    assert_eq!(s.stats(&Filter::default(), today()).unwrap().catches, 1);
}

#[test]
fn rewritten_or_shrunk_files_never_double_count() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("abc.jsonl");
    let ts = (Utc::now() - Duration::hours(2)).to_rfc3339();
    let body = [
        serde_json::json!({"type":"assistant","timestamp":ts,"message":{"model":"claude-opus-5-5","content":[{"type":"text","text":"Use /admin."}]}}),
        serde_json::json!({"type":"user","uuid":"u1","timestamp":ts,"message":{"content":"that's not a real page"}}),
        serde_json::json!({"type":"assistant","timestamp":ts,"message":{"model":"claude-opus-5-5","content":[{"type":"text","text":"You're right."}]}}),
    ]
    .iter()
    .map(|v| format!("{v}\n"))
    .collect::<String>();
    std::fs::write(&path, &body).unwrap();
    let all = vec![sources::claude_code_at(dir.path().to_path_buf())];
    let mut s = Store::open_in_memory().unwrap();
    s.allow("claude-code", None).unwrap();
    sync(&mut s, &all).unwrap();

    // The tool rewrites the file (compaction, re-save): it's re-read from zero, keys block copies.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(&path, format!("{{\"type\":\"summary\"}}\n{body}")).unwrap();
    sync(&mut s, &all).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(&path, &body[..body.len() / 2]).unwrap(); // shrinks
    sync(&mut s, &all).unwrap();

    let st = s.stats(&Filter::default(), today()).unwrap();
    assert_eq!((st.messages, st.catches), (1, 1));
}

#[test]
fn paused_and_revoked_sources_are_not_read() {
    let dir = tempfile::tempdir().unwrap();
    let ts = Utc::now().to_rfc3339();
    let line = |v: serde_json::Value| format!("{v}\n");
    std::fs::write(
        dir.path().join("a.jsonl"),
        line(serde_json::json!({"type":"user","uuid":"u","timestamp":ts,"message":{"content":"no, that's wrong"}}))
            + &line(serde_json::json!({"type":"assistant","timestamp":ts,"message":{"content":[{"type":"text","text":"You're right."}]}})),
    )
    .unwrap();
    let all = vec![sources::claude_code_at(dir.path().to_path_buf())];
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
    let line = |v: serde_json::Value| format!("{v}\n");
    std::fs::write(
        dir.path().join("a.jsonl"),
        line(serde_json::json!({"type":"assistant","timestamp":ts,"message":{"model":"<synthetic>","content":[{"type":"text","text":"API Error"}]}}))
            + &line(serde_json::json!({"type":"user","uuid":"u","timestamp":ts,"message":{"content":"no, that's wrong"}}))
            + &line(serde_json::json!({"type":"assistant","timestamp":ts,"message":{"model":"claude-opus-5-5","content":[{"type":"text","text":"You're right."}]}})),
    )
    .unwrap();
    let all = vec![sources::claude_code_at(dir.path().to_path_buf())];
    let mut s = Store::open_in_memory().unwrap();
    s.allow("claude-code", None).unwrap();
    assert!(!sync(&mut s, &all).unwrap().rescored, "a fresh store has nothing to rebuild");

    // Pretend these scores came from older rules.
    s.set_setting("rules_version", "1").unwrap();
    let r = sync(&mut s, &all).unwrap();
    assert!(r.rescored);
    assert_eq!(r.catches, 1, "history was re-read");
    assert!(s.source_state("claude-code").unwrap().unwrap().allowed, "consent survives");
    let st = s.stats(&Filter::default(), today()).unwrap();
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
    let ts = (Utc::now() - Duration::minutes(30)).to_rfc3339();
    let line = |v: serde_json::Value| format!("{v}\n");
    let turn = |id: &str| {
        line(serde_json::json!({"type":"user","uuid":id,"timestamp":ts,"message":{"content":"no, that's wrong"}}))
            + &line(
                serde_json::json!({"type":"assistant","timestamp":ts,"message":{"model":"claude-opus-5-5","content":[{"type":"text","text":"You're right."}]}}),
            )
    };
    // Written earlier today: must not count when the person picks "From now on".
    std::fs::write(&path, turn("old")).unwrap();
    let src = sources::claude_code_at(dir.path().to_path_buf());
    let all = vec![sources::claude_code_at(dir.path().to_path_buf())];
    let mut s = Store::open_in_memory().unwrap();
    s.allow("claude-code", None).unwrap();
    assert_eq!(s.start_from_now(src.as_ref()).unwrap(), 1);
    assert_eq!(sync(&mut s, &all).unwrap().messages, 0);

    std::thread::sleep(std::time::Duration::from_millis(1100));
    let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
    f.write_all(turn("new").as_bytes()).unwrap();
    drop(f);
    let r = sync(&mut s, &all).unwrap();
    assert_eq!((r.messages, r.catches), (1, 1), "only the new message counts");
}
