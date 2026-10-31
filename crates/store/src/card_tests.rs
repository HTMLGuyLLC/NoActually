//! Report cards in the store: reading a card's block against its transcript, the merge rule, and
//! the numbers the Improve tab reads. Transcripts are made-up chats in Claude Code's real format.

use super::*;
use crate::milestones::straight_as;
use engine::{habit_bit, seal, Usage};
use serde_json::{json, Value};
use sources::testing::{cc_user, line, set_mtime_ago};
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration as StdDuration;

// ---- transcripts ---------------------------------------------------------------------------

/// `m` minutes past noon UTC on October `day` 2026 (the 5th is a Monday). Noon is the same local
/// day everywhere from UTC-11 to UTC+11.
fn at(day: u32, m: u32) -> String {
    format!("2026-10-{day:02}T12:{m:02}:00Z")
}

fn t(m: u32) -> String {
    at(5, m)
}

/// An AI reply record with its own uuid, message id and token counts, from Claude Opus 5.5.
fn reply(uuid: &str, ts: &str, text: &str, usage: (u64, u64, u64, u64)) -> String {
    reply_on("claude-opus-5-5", uuid, ts, text, usage)
}

/// The same, from `model`.
fn reply_on(model: &str, uuid: &str, ts: &str, text: &str, usage: (u64, u64, u64, u64)) -> String {
    line(&json!({"type":"assistant","uuid":uuid,"timestamp":ts,"message":{"id":format!("m-{uuid}"),
        "model":model,"content":[{"type":"text","text":text}],"usage":{"input_tokens":usage.0,
        "cache_creation_input_tokens":usage.1,"cache_read_input_tokens":usage.2,"output_tokens":usage.3}}}))
}

/// Four prompts ten minutes apart, `p1` to `p4`: a request, two follow-ups and a sign-off.
fn work(p: &str, day: u32) -> String {
    let a = |m| at(day, m);
    cc_user(&format!("{p}1"), &a(0), "build the settings page with tabs")
        + &reply(&format!("{p}r1"), &a(2), "Built it.", (10, 100, 1000, 50))
        + &cc_user(&format!("{p}2"), &a(10), "use tailwind for the styles")
        + &reply(&format!("{p}r2"), &a(12), "Restyled.", (20, 0, 2000, 80))
        + &cc_user(&format!("{p}3"), &a(20), "also keep the old route working")
        + &reply(&format!("{p}r3"), &a(22), "Done.", (5, 0, 500, 20))
        + &cc_user(&format!("{p}4"), &a(30), "thanks")
        + &reply(&format!("{p}r4"), &a(31), "Welcome.", (1, 0, 100, 5))
}

/// The short id a card uses for the prompt whose record has this uuid.
fn id(uuid: &str) -> String {
    engine::short_id(&hash(&["claude-code", uuid])).to_string()
}

/// A task over `work(p, _)`: the whole chat, one avoidable follow-up (half the work redone) and one
/// late addition, missing the finish line and context.
fn task(p: &str) -> Value {
    json!({"at":id(&format!("{p}1")),"to":id(&format!("{p}4")),
        "waste":[[id(&format!("{p}2")),50],[id(&format!("{p}3")),0]],"miss":["finish","context"]})
}

/// A card reply record: the words, then the block sealed with `seal`.
fn card(uuid: &str, ts: &str, seal: &str, tasks: &[Value]) -> String {
    let block = json!({"v":1,"k":seal,"tasks":tasks});
    reply(uuid, ts, &format!("Report card: B\n\n```noactually\n{block}\n```\n"), (500, 500, 500, 500))
}

// ---- the lab: a real database, a real key and real files -------------------------------------

struct Lab {
    dir: tempfile::TempDir,
    store: Store,
    secret: Vec<u8>,
}

/// A chat that has gone quiet, so its last message is final.
const SETTLED: StdDuration = StdDuration::from_secs(20 * 60);

impl Lab {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let secret = card_secret(&dir.path().join("data").join("noactually.db"), true).unwrap().unwrap();
        Self { store: Self::open(&dir), dir, secret }
    }

    /// No helper has run yet, so there's no key.
    fn without_key() -> Self {
        let dir = tempfile::tempdir().unwrap();
        Self { store: Self::open(&dir), dir, secret: Vec::new() }
    }

    fn open(dir: &tempfile::TempDir) -> Store {
        let store = Store::open(&dir.path().join("data").join("noactually.db")).unwrap();
        store.allow("claude-code", None).unwrap();
        store
    }

    fn sources(&self) -> Vec<Box<dyn Source>> {
        vec![sources::claude_code_at(self.dir.path().join("projects"))]
    }

    fn path(&self, session: &str) -> PathBuf {
        self.dir.path().join("projects").join("p").join(format!("{session}.jsonl"))
    }

    fn write(&self, session: &str, body: &str) {
        let path = self.path(session);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        set_mtime_ago(&path, SETTLED);
    }

    fn append(&self, session: &str, body: &str) {
        let path = self.path(session);
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        drop(f);
        set_mtime_ago(&path, SETTLED);
    }

    fn seal(&self, session: &str) -> String {
        seal(&self.secret, session)
    }

    fn sync(&mut self) -> SyncReport {
        let sources = self.sources();
        let report = sync(&mut self.store, &sources).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        report
    }
}

/// A stored row, as the tests look at it.
#[derive(Debug, PartialEq)]
struct Stored {
    key: String,
    card: String,
    source: String,
    session: String,
    first: String,
    day: String,
    start_ts: String,
    end_ts: String,
    provider: String,
    family: String,
    version: String,
    grade: String,
    rounds: i64,
    avoidable: i64,
    missing: u8,
    tokens: cards::Tokens,
    minutes: i64,
    graded_at: String,
}

fn stored(s: &Store) -> Vec<Stored> {
    let mut stmt = s
        .conn()
        .prepare(
            "SELECT key, card, source, session, first, day, start_ts, end_ts, provider, family, version, grade, rounds,
                avoidable, missing, tokens, minutes, graded_at FROM report_cards ORDER BY key",
        )
        .unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok(Stored {
                key: r.get(0)?,
                card: r.get(1)?,
                source: r.get(2)?,
                session: r.get(3)?,
                first: r.get(4)?,
                day: r.get(5)?,
                start_ts: r.get(6)?,
                end_ts: r.get(7)?,
                provider: r.get(8)?,
                family: r.get(9)?,
                version: r.get(10)?,
                grade: r.get(11)?,
                rounds: r.get(12)?,
                avoidable: r.get(13)?,
                missing: r.get(14)?,
                tokens: serde_json::from_str(&r.get::<_, String>(15)?).unwrap(),
                minutes: r.get(16)?,
                graded_at: r.get(17)?,
            })
        })
        .unwrap();
    rows.map(Result::unwrap).collect()
}

fn count(s: &Store, table: &str) -> i64 {
    s.conn().query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap()
}

fn habit(name: &str) -> u8 {
    habit_bit(name).unwrap()
}

/// Tokens as the store keeps them for a task that used Claude Opus 5.5 throughout.
fn opus(u: Usage) -> cards::Tokens {
    [("claude-opus-5-5".to_string(), u)].into()
}

// ---- reading a card -------------------------------------------------------------------------

#[test]
fn a_card_becomes_numbers_worked_out_from_the_transcript() {
    let mut lab = Lab::new();
    lab.write("chat1", &(work("u", 5) + &card("c1", &t(40), &lab.seal("chat1"), &[task("u")])));
    lab.sync();
    let rows = stored(&lab.store);
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!(r.key, hash(&["c1", &hash(&["claude-code", "u1"])]), "card message key + first prompt key");
    assert_eq!(r.card, hash(&["c1"]));
    assert_eq!(r.first, hash(&["claude-code", "u1"]), "the task's first prompt key, the same in a resumed copy");
    assert_eq!((r.source.as_str(), r.session.as_str()), ("claude-code", hash(&["claude-code", "chat1"]).as_str()));
    assert_eq!(r.day, "2026-10-05");
    assert_eq!(
        (r.start_ts.as_str(), r.end_ts.as_str(), r.graded_at.as_str()),
        ("2026-10-05T12:00:00.000Z", "2026-10-05T12:30:00.000Z", "2026-10-05T12:40:00.000Z")
    );
    assert_eq!(
        (r.provider.as_str(), r.family.as_str(), r.version.as_str()),
        ("Anthropic", "Claude Opus", "5.5"),
        "the model of the task's first round"
    );
    // Four prompts, but "thanks" closes the task rather than being a round of work.
    assert_eq!((r.grade.as_str(), r.rounds, r.avoidable), ("B", 3, 1), "a 0% follow-up is a tip, not lost work");
    assert_eq!(r.missing, habit("finish") | habit("context"));
    // Half of the first round's work (10 minutes, 1,160 tokens) was redone because of the follow-up.
    assert_eq!(r.tokens, opus(Usage { input: 5, cache_write: 50, cache_read: 500, output: 25 }));
    assert_eq!(r.minutes, 5);
    assert_eq!(count(&lab.store, "seen"), 4, "the turns are counted as before");
}

#[test]
fn each_follow_up_redoes_a_share_of_the_work_before_it() {
    let mut lab = Lab::new();
    let redo = json!({"at":id("u1"),"to":id("u4"),"waste":[[id("u2"),100],[id("u3"),25]]});
    lab.write("chat1", &(work("u", 5) + &card("c1", &t(40), &lab.seal("chat1"), &[redo])));
    lab.sync();
    let r = &stored(&lab.store)[0];
    assert_eq!((r.grade.as_str(), r.avoidable, r.rounds, r.missing), ("C", 2, 3, 0));
    // All of the first round, and a quarter of the first two (rounded down per kind): the follow-up's own round isn't counted.
    assert_eq!(r.tokens, opus(Usage { input: 17, cache_write: 125, cache_read: 1750, output: 82 }));
    assert_eq!(r.minutes, 15, "(10 × 100% + 20 × 25%) minutes");
}

#[test]
fn a_task_can_be_one_prompt_and_a_wrong_goal_is_a_d() {
    let mut lab = Lab::new();
    let one = json!({"at":id("u1"),"to":id("u1"),"wrong":true});
    lab.write("chat1", &(work("u", 5) + &card("c1", &t(40), &lab.seal("chat1"), &[one])));
    lab.sync();
    let r = &stored(&lab.store)[0];
    assert_eq!((r.grade.as_str(), r.rounds, r.avoidable, r.minutes), ("D", 1, 0, 0), "never worse than a D on its own");
    assert_eq!(r.tokens, cards::Tokens::new(), "nothing was redone");
    assert_eq!((r.start_ts.as_str(), r.end_ts.as_str()), ("2026-10-05T12:00:00.000Z", "2026-10-05T12:00:00.000Z"));
}

#[test]
fn a_card_with_no_message_id_or_time_still_stores() {
    let mut lab = Lab::new();
    let block = json!({"v":1,"k":lab.seal("chat1"),"tasks":[task("u")]});
    let bare = line(&json!({"type":"assistant","message":{"model":"claude-opus-5-5","content":[
        {"type":"text","text":format!("Card.\n```noactually\n{block}\n```")}]}}));
    lab.write("chat1", &(work("u", 5) + &bare));
    lab.sync();
    let r = &stored(&lab.store)[0];
    // The key falls back to where the reply is in the file, and the time to the task's last prompt.
    let pos = work("u", 5).len();
    assert_eq!(r.key, hash(&[&hash(&["claude-code", "chat1", &pos.to_string()]), &hash(&["claude-code", "u1"])]));
    assert_eq!(r.graded_at, "2026-10-05T12:30:00.000Z");
}

// ---- what is left out ------------------------------------------------------------------------

#[test]
fn a_card_that_isnt_this_chats_is_left_out_and_the_turns_still_count() {
    let mut lab = Lab::new();
    let theirs = lab.seal("chat2");
    let other_key = seal(b"another secret", "chat1");
    // Sealed for another chat, sealed with another secret, and a chat holding a copy of chat1's card.
    lab.write("chat1", &(work("a", 5) + &card("c1", &t(40), &theirs, &[task("a")])));
    lab.write("chat3", &(work("b", 5) + &card("c2", &t(40), &other_key, &[task("b")])));
    lab.write("chat4", &(work("c", 5) + &card("c3", &t(40), &lab.seal("chat1"), &[task("c")])));
    lab.sync();
    assert_eq!(count(&lab.store, "report_cards"), 0);
    assert_eq!(count(&lab.store, "seen"), 12, "their prompts still count");

    // The right seal for the right chat is kept.
    lab.write("chat5", &(work("d", 5) + &card("c4", &t(40), &lab.seal("chat5"), &[task("d")])));
    lab.sync();
    assert_eq!(count(&lab.store, "report_cards"), 1);
}

#[test]
fn without_a_key_nothing_is_trusted_until_there_is_one() {
    let mut lab = Lab::without_key();
    let forged = seal(b"", "chat1");
    lab.write("chat1", &(work("u", 5) + &card("c1", &t(40), &forged, &[task("u")])));
    lab.sync();
    assert_eq!(count(&lab.store, "report_cards"), 0);
    assert_eq!(count(&lab.store, "seen"), 4);

    // The helper makes the key after the app started: the next card is checked against it.
    lab.secret = card_secret(&lab.dir.path().join("data").join("noactually.db"), true).unwrap().unwrap();
    lab.append("chat1", &card("c2", &t(50), &lab.seal("chat1"), &[task("u")]));
    lab.sync();
    assert_eq!(stored(&lab.store).len(), 1);
}

#[test]
fn a_card_that_could_not_be_checked_is_not_remembered_as_read() {
    let mut lab = Lab::new();
    let key = lab.dir.path().join("data").join(CARD_KEY_FILE);
    let secret = std::fs::read(&key).unwrap();
    lab.write("chat1", &(work("u", 5) + &card("c1", &t(40), &lab.seal("chat1"), &[task("u")])));
    // The key can't be read this time (the helper's file is missing, or locked for a moment).
    std::fs::remove_file(&key).unwrap();
    lab.sync();
    assert_eq!(count(&lab.store, "report_cards"), 0);
    let cp = lab.store.file_checkpoint(&lab.path("chat1").to_string_lossy()).unwrap().unwrap();
    assert_eq!(cp.cards_to, None, "the card wasn't checked, so it isn't marked read");

    // With the key back, the next pass checks the same card.
    std::fs::write(&key, &secret).unwrap();
    lab.append("chat1", &cc_user("n1", &t(45), "now add a dark mode"));
    lab.sync();
    assert_eq!(stored(&lab.store).len(), 1);
    let cp = lab.store.file_checkpoint(&lab.path("chat1").to_string_lossy()).unwrap().unwrap();
    assert!(cp.cards_to.is_some(), "and now it is");
}

#[test]
fn a_card_that_could_not_be_checked_is_read_again_even_after_a_whole_turn_that_follows_it() {
    let mut lab = Lab::new();
    let key = lab.dir.path().join("data").join(CARD_KEY_FILE);
    let secret = std::fs::read(&key).unwrap();
    let before = work("u", 5);
    let card_and_a_turn = card("c1", &t(40), &lab.seal("chat1"), &[task("u")])
        + &cc_user("n1", &t(45), "now add a dark mode")
        + &reply("r9", &t(46), "Done.", (1, 1, 1, 1));
    lab.write("chat1", &(before.clone() + &card_and_a_turn));
    std::fs::remove_file(&key).unwrap();
    lab.sync();
    assert_eq!(count(&lab.store, "report_cards"), 0);
    let prompts = count(&lab.store, "seen");
    assert_eq!(prompts, 5, "the turns around the card still count");
    let cp = lab.store.file_checkpoint(&lab.path("chat1").to_string_lossy()).unwrap().unwrap();
    assert_eq!((cp.at, cp.cards_to), (before.len() as u64, None), "resumes at the card, not past the turn after it");

    // With the key back, the next pass reads the card again, and the turn after it is not counted twice.
    std::fs::write(&key, &secret).unwrap();
    lab.append("chat1", &cc_user("n2", &t(47), "and a light one"));
    lab.sync();
    assert_eq!(stored(&lab.store).len(), 1);
    assert_eq!(count(&lab.store, "seen"), prompts + 1, "only the new message");
    let cp = lab.store.file_checkpoint(&lab.path("chat1").to_string_lossy()).unwrap().unwrap();
    assert!(cp.cards_to.is_some(), "and now it is read");
}

#[test]
fn only_a_card_that_was_not_checked_holds_the_checkpoint_back() {
    let mut lab = Lab::new();
    let key = lab.dir.path().join("data").join(CARD_KEY_FILE);
    let first = work("u", 5);
    lab.write("chat1", &(first.clone() + &card("c1", &t(40), &lab.seal("chat1"), &[task("u")])));
    lab.sync();
    assert_eq!(stored(&lab.store).len(), 1);

    // The key is lost; a whole turn and a second card come, then another whole turn.
    std::fs::remove_file(&key).unwrap();
    lab.append("chat1", &(cc_user("n1", &t(45), "now add a dark mode") + &reply("r9", &t(46), "Done.", (1, 1, 1, 1))));
    let second = std::fs::metadata(lab.path("chat1")).unwrap().len();
    lab.append("chat1", &card("c2", &t(50), &lab.seal("chat1"), &[task("u")]));
    lab.append("chat1", &(cc_user("n2", &t(55), "and a light one") + &reply("r10", &t(56), "Ok.", (1, 1, 1, 1))));
    lab.sync();
    let cp = lab.store.file_checkpoint(&lab.path("chat1").to_string_lossy()).unwrap().unwrap();
    assert_eq!(cp.at, second, "at the second card: the first was read and is stored");
    assert_eq!(cp.cards_to, Some(first.len() as u64), "and what was read stays read");
    assert_eq!(stored(&lab.store).len(), 1);
}

#[test]
fn an_in_memory_database_has_no_key_and_so_no_cards() {
    let mut s = Store::open_in_memory().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chat1.jsonl");
    std::fs::write(&path, work("u", 5) + &card("c1", &t(40), &seal(b"x", "chat1"), &[task("u")])).unwrap();
    let src = sources::claude_code_at(dir.path().to_path_buf());
    let state = SourceState { allowed: true, ..SourceState::new("claude-code") };
    let scan = src.scan(&path, 0, true).unwrap();
    let chat = src.view(&path).unwrap().unwrap();
    s.ingest(&state, "f", &scan, Some(&chat), Pass::new(FileMeta::default(), true)).unwrap();
    assert_eq!((count(&s, "report_cards"), count(&s, "seen")), (0, 4));
}

#[test]
fn only_the_task_with_a_bad_id_is_left_out() {
    let mut lab = Lab::new();
    let good = json!({"at":id("u1"),"to":id("u2"),"waste":[[id("u2"),25]]});
    let unknown_start = json!({"at":"deadbeef","to":id("u2")});
    let unknown_end = json!({"at":id("u1"),"to":"deadbeef"});
    let backwards = json!({"at":id("u3"),"to":id("u1")});
    let unknown_waste = json!({"at":id("u1"),"to":id("u3"),"waste":[["deadbeef",50]]});
    let outside = json!({"at":id("u1"),"to":id("u2"),"waste":[[id("u3"),50]]});
    let before = json!({"at":id("u2"),"to":id("u3"),"waste":[[id("u1"),50]]});
    let tasks = [good, unknown_start, unknown_end, backwards, unknown_waste, outside, before];
    lab.write("chat1", &(work("u", 5) + &card("c1", &t(40), &lab.seal("chat1"), &tasks)));
    // A block that isn't one, and one for a version this app doesn't read.
    let junk = reply("c2", &t(41), "```noactually\nnot json\n```\n", (1, 1, 1, 1));
    let v2 = reply("c3", &t(42), &format!("```noactually\n{}\n```\n", json!({"v":2,"k":"x"})), (1, 1, 1, 1));
    lab.append("chat1", &(junk + &v2));
    lab.sync();
    let rows = stored(&lab.store);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].key, hash(&["c1", &hash(&["claude-code", "u1"])]));
    assert_eq!((rows[0].rounds, rows[0].grade.as_str()), (2, "B"));
}

#[test]
fn a_task_that_overlaps_an_earlier_task_of_the_same_card_is_left_out() {
    let mut lab = Lab::new();
    let span = |from: &str, to: &str| json!({"at":id(from),"to":id(to)});
    // u1..u3 is kept. u2..u4 overlaps it, and so does u1..u1 (the same first prompt), so both go. u3..u4
    // only shares an endpoint with the first (and the dropped u2..u4 doesn't count), so it stays.
    let tasks = [span("u1", "u3"), span("u2", "u4"), span("u1", "u1"), span("u3", "u4")];
    lab.write("chat1", &(work("u", 5) + &card("c1", &t(40), &lab.seal("chat1"), &tasks)));
    lab.sync();
    let mut firsts: Vec<String> = stored(&lab.store).into_iter().map(|r| r.first).collect();
    firsts.sort();
    let mut want = vec![hash(&["claude-code", "u1"]), hash(&["claude-code", "u3"])];
    want.sort();
    assert_eq!(firsts, want);

    // The reason is logged, and only for the two.
    let state = SourceState { allowed: true, ..SourceState::new("claude-code") };
    let view = lab.sources().remove(0).view(&lab.path("chat1")).unwrap().unwrap();
    let read = cards::read(&state, &view, &lab.secret, None);
    assert_eq!(read.rows.len(), 2);
    assert_eq!(read.dropped.get("overlaps another task in this card"), Some(&2), "{:?}", read.dropped);
    assert_eq!(read.dropped.len(), 1);

    // Two cards may grade the same work: the merge rule picks between them.
    lab.append("chat1", &card("c2", &t(50), &lab.seal("chat1"), &[span("u1", "u3")]));
    lab.sync();
    assert_eq!(count(&lab.store, "report_cards"), 3);
}

#[test]
fn sign_offs_close_a_task_but_are_not_rounds() {
    let mut lab = Lab::new();
    let upto = |to: &str| json!({"at":id("u1"),"to":id(to)});
    lab.write("chat1", &(work("u", 5) + &card("c1", &t(40), &lab.seal("chat1"), &[upto("u3")])));
    lab.append("chat1", &card("c2", &t(41), &lab.seal("chat1"), &[upto("u4")]));
    lab.sync();
    let rounds: Vec<i64> = stored(&lab.store).iter().map(|r| r.rounds).collect();
    assert_eq!(rounds, [3, 3], "the sign-off adds no round");
}

// ---- consent ---------------------------------------------------------------------------------

#[test]
fn a_backfill_limit_skips_tasks_that_started_before_it() {
    let mut lab = Lab::new();
    lab.store.allow("claude-code", Some("2026-10-10".parse().unwrap())).unwrap();
    lab.write("chat1", &(work("u", 5) + &card("c1", &t(40), &lab.seal("chat1"), &[task("u")])));
    lab.write("chat2", &(work("v", 12) + &card("c2", &at(12, 40), &lab.seal("chat2"), &[task("v")])));
    lab.sync();
    let rows = stored(&lab.store);
    assert_eq!(rows.iter().map(|r| r.day.as_str()).collect::<Vec<_>>(), ["2026-10-12"]);
}

#[test]
fn from_now_on_skips_tasks_that_started_before_the_cutoff() {
    let mut lab = Lab::new();
    lab.write("chat1", &(work("u", 5) + &card("c1", &t(40), &lab.seal("chat1"), &[task("u")])));
    let src = lab.sources().remove(0);
    lab.store.start_from_now(&*src).unwrap();
    lab.sync();
    assert_eq!(count(&lab.store, "report_cards"), 0, "history before the cutoff isn't read");

    // A later card covers the old task and a new one: only the new one is kept.
    let later = |m| "2099-01-05T12:".to_string() + &format!("{m:02}:00Z");
    let new = cc_user("n1", &later(0), "add a dark mode")
        + &reply("nr1", &later(2), "Added.", (1, 1, 1, 1))
        + &cc_user("n2", &later(10), "thanks");
    let both = [task("u"), json!({"at":id("n1"),"to":id("n2")})];
    lab.append("chat1", &(new + &card("c2", &later(20), &lab.seal("chat1"), &both)));
    lab.sync();
    let rows = stored(&lab.store);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].key, hash(&["c2", &hash(&["claude-code", "n1"])]));
    assert_eq!(rows[0].rounds, 1);
}

// ---- keeping them ------------------------------------------------------------------------------

#[test]
fn reading_a_card_again_never_changes_anything() {
    let mut lab = Lab::new();
    let c1 = card("c1", &t(40), &lab.seal("chat1"), &[task("u")]);
    lab.write("chat1", &(work("u", 5) + &c1));
    lab.sync();
    let first = stored(&lab.store);
    assert_eq!(lab.sync().files_read, 0, "an unchanged file isn't read again");

    // A checkpoint rewind or a resumed copy shows the same reply again: same card, same row.
    lab.append("chat1", &c1);
    lab.sync();
    assert_eq!(stored(&lab.store), first);

    // A new card for the same task is a second row; the merge rule picks between them.
    lab.append("chat1", &card("c2", &t(50), &lab.seal("chat1"), &[task("u")]));
    lab.sync();
    assert_eq!(count(&lab.store, "report_cards"), 2);
}

/// Claude Code's adapter, counting how often a whole chat is read for its cards.
struct Counting {
    inner: Box<dyn Source>,
    views: Arc<AtomicUsize>,
}

impl Source for Counting {
    fn id(&self) -> &'static str {
        self.inner.id()
    }
    fn name(&self) -> &'static str {
        self.inner.name()
    }
    fn kind(&self) -> Kind {
        self.inner.kind()
    }
    fn roots(&self) -> Vec<PathBuf> {
        self.inner.roots()
    }
    fn files(&self, root: &Path) -> Vec<PathBuf> {
        self.inner.files(root)
    }
    fn scan(&self, path: &Path, from: u64, finalize: bool) -> sources::Result<sources::FileScan> {
        self.inner.scan(path, from, finalize)
    }
    fn append_only(&self, path: &Path) -> bool {
        self.inner.append_only(path)
    }
    fn view(&self, path: &Path) -> Option<sources::Result<ChatView>> {
        self.views.fetch_add(1, Ordering::SeqCst);
        self.inner.view(path)
    }
}

impl Lab {
    /// A sync that counts the whole-chat reads it made.
    fn sync_counting(&mut self) -> usize {
        let views = Arc::new(AtomicUsize::new(0));
        let counting: Vec<Box<dyn Source>> =
            vec![Box::new(Counting { inner: self.sources().remove(0), views: views.clone() })];
        let report = sync(&mut self.store, &counting).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        views.load(Ordering::SeqCst)
    }
}

#[test]
fn a_card_already_read_does_not_make_the_chat_be_read_again() {
    let mut lab = Lab::new();
    lab.write("chat1", &(work("u", 5) + &card("c1", &t(40), &lab.seal("chat1"), &[task("u")])));
    assert_eq!(lab.sync_counting(), 1, "the first read");
    let first = stored(&lab.store);

    // The file ends on the card, so every pass resumes at it and sees it again. A message nobody
    // has answered yet keeps it that way until a reply comes.
    lab.append("chat1", &cc_user("n1", &t(45), "now add a dark mode"));
    assert_eq!(lab.sync_counting(), 0, "same card, same row: nothing to read for");
    lab.append("chat1", &cc_user("n2", &t(46), "and a light one"));
    assert_eq!(lab.sync_counting(), 0);
    assert_eq!(stored(&lab.store), first);

    // A card past that point is read, and what it grades is kept.
    lab.append("chat1", &card("c2", &t(50), &lab.seal("chat1"), &[task("u")]));
    assert_eq!(lab.sync_counting(), 1);
    assert_eq!(count(&lab.store, "report_cards"), 2);
    lab.append("chat1", &reply("r9", &t(55), "Ok.", (1, 1, 1, 1)));
    assert_eq!(lab.sync_counting(), 0);
}

#[test]
fn a_file_read_again_from_the_top_reads_its_cards_again() {
    let mut lab = Lab::new();
    let c1 = card("c1", &t(40), &lab.seal("chat1"), &[task("u")]);
    lab.write("chat1", &(work("u", 5) + &c1));
    assert_eq!(lab.sync_counting(), 1);
    let first = stored(&lab.store);
    let remembered = |lab: &Lab| -> Option<i64> {
        lab.store.conn().query_row("SELECT cards_to FROM files", [], |r| r.get(0)).unwrap()
    };
    assert!(remembered(&lab).is_some());

    // Rewritten shorter, without the card: it starts over and remembers nothing.
    lab.write("chat1", &work("u", 5));
    assert_eq!(lab.sync_counting(), 0);
    assert_eq!(remembered(&lab), None);
    // Written back with the card, it is read again.
    lab.write("chat1", &(work("u", 5) + &c1 + &cc_user("n1", &t(45), "more")));
    assert_eq!(lab.sync_counting(), 1);
    assert_eq!(stored(&lab.store), first);

    // A rebuild and forgetting a source drop the checkpoints, and the memory with them.
    lab.store.reset_scores().unwrap();
    assert_eq!(lab.sync_counting(), 1);
    lab.store.forget("claude-code").unwrap();
    lab.store.allow("claude-code", None).unwrap();
    assert_eq!(lab.sync_counting(), 1);
    assert_eq!(stored(&lab.store).len(), 1);
}

#[test]
fn a_rebuild_keeps_the_rows_and_doesnt_double_them() {
    let mut lab = Lab::new();
    lab.write("chat1", &(work("u", 5) + &card("c1", &t(40), &lab.seal("chat1"), &[task("u")])));
    lab.sync();
    let before = stored(&lab.store);
    lab.store.reset_scores().unwrap();
    assert_eq!((count(&lab.store, "seen"), count(&lab.store, "files")), (0, 0), "scores are cleared");
    assert_eq!(stored(&lab.store), before, "but not the report cards");
    lab.sync();
    assert_eq!(stored(&lab.store), before, "reading the files again writes the same rows");
    assert_eq!(count(&lab.store, "seen"), 4);

    // The same through a rules change, and once the transcript itself is gone.
    lab.store.set_setting("rules_version", "0").unwrap();
    assert!(lab.sync().rescored);
    assert_eq!(stored(&lab.store), before);
    std::fs::remove_file(lab.path("chat1")).unwrap();
    lab.store.reset_scores().unwrap();
    lab.sync();
    assert_eq!(stored(&lab.store), before, "Claude Code deletes old chats; the history stays");
}

#[test]
fn forget_deletes_that_sources_cards_and_allowing_again_rebuilds_them() {
    let mut lab = Lab::new();
    lab.write("chat1", &(work("u", 5) + &card("c1", &t(40), &lab.seal("chat1"), &[task("u")])));
    lab.sync();
    Seed { source: "codex", ..Seed::new("other") }.save(&lab.store);
    lab.store.forget("claude-code").unwrap();
    let left: Vec<String> = stored(&lab.store).into_iter().map(|r| r.source).collect();
    assert_eq!(left, ["codex"], "only the forgotten source's rows go");

    lab.store.allow("claude-code", None).unwrap();
    lab.sync();
    assert_eq!(stored(&lab.store).len(), 2, "the files are still there, so its cards come back");
}

#[test]
fn deleting_the_history_stays_deleted_even_when_files_are_read_again() {
    let mut lab = Lab::new();
    lab.write("chat1", &(work("u", 5) + &card("c1", &t(40), &lab.seal("chat1"), &[task("u")])));
    lab.sync();
    assert_eq!(lab.store.delete_report_cards().unwrap(), 1);
    assert_eq!(count(&lab.store, "report_cards"), 0);
    assert_eq!(lab.store.delete_report_cards().unwrap(), 0);

    lab.store.reset_scores().unwrap();
    lab.sync();
    assert_eq!(count(&lab.store, "report_cards"), 0, "a rebuild doesn't bring back what was deleted");
    assert_eq!(count(&lab.store, "seen"), 4, "everything else is read as usual");

    // A card graded after the deletion is kept.
    let later = "2099-01-05T12:00:00Z";
    let new = cc_user("n1", later, "add a dark mode") + &reply("nr1", later, "Added.", (1, 1, 1, 1));
    lab.append("chat1", &(new + &card("c2", later, &lab.seal("chat1"), &[json!({"at":id("n1"),"to":id("n1")})])));
    lab.sync();
    assert_eq!(count(&lab.store, "report_cards"), 1);
    lab.store.reset_scores().unwrap();
    lab.sync();
    assert_eq!(count(&lab.store, "report_cards"), 1, "and so is it after a rebuild");
}

// ---- the merge rule --------------------------------------------------------------------------

/// A row to put in the table as it is, for the merge rule and the reports.
#[derive(Clone)]
struct Seed {
    key: String,
    card: String,
    source: &'static str,
    session: String,
    first: String,
    day: &'static str,
    start: String,
    end: String,
    family: &'static str,
    version: &'static str,
    grade: &'static str,
    rounds: i64,
    avoidable: i64,
    missing: u8,
    /// What the task redid, on the model its family names; nothing redone is an empty map.
    tokens: Usage,
    minutes: i64,
    graded: String,
}

fn stamp(day: u32, m: u32) -> String {
    cards::ts_str(at(day, m).parse().unwrap())
}

impl Seed {
    fn new(key: &str) -> Seed {
        Seed {
            key: key.into(),
            card: key.into(),
            source: "claude-code",
            session: "s1".into(),
            first: format!("first-{key}"),
            day: "2026-10-05",
            start: stamp(5, 0),
            end: stamp(5, 0),
            family: "Claude Opus",
            version: "5.5",
            grade: "A",
            rounds: 1,
            avoidable: 0,
            missing: 0,
            tokens: Usage::default(),
            minutes: 0,
            graded: stamp(5, 59),
        }
    }

    /// A task over minutes `from` to `to` of the 5th, graded at minute `graded`, by card `card`.
    fn span(key: &str, card: &str, (from, to): (u32, u32), graded: u32) -> Seed {
        Seed { card: card.into(), start: stamp(5, from), end: stamp(5, to), graded: stamp(5, graded), ..Seed::new(key) }
    }

    fn save(&self, s: &Store) {
        let (provider, raw) =
            if self.family.starts_with("Claude") { ("Anthropic", "claude-opus-5-5") } else { ("OpenAI", "gpt-5.1") };
        let tokens: cards::Tokens = if self.tokens == Usage::default() {
            cards::Tokens::new()
        } else {
            [(raw.to_string(), self.tokens)].into()
        };
        s.conn()
            .execute(
                "INSERT INTO report_cards (key, card, source, session, first, day, start_ts, end_ts, provider, family,
                    version, grade, rounds, avoidable, missing, auto, tokens, minutes, graded_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,0,?16,?17,?18)",
                params![
                    self.key,
                    self.card,
                    self.source,
                    self.session,
                    self.first,
                    self.day,
                    self.start,
                    self.end,
                    provider,
                    self.family,
                    self.version,
                    self.grade,
                    self.rounds,
                    self.avoidable,
                    self.missing,
                    serde_json::to_string(&tokens).unwrap(),
                    self.minutes,
                    self.graded,
                ],
            )
            .unwrap();
    }
}

/// The keys that count after `rows` are saved in this order.
fn survivors(rows: &[Seed]) -> Vec<String> {
    let s = Store::open_in_memory().unwrap();
    for r in rows {
        r.save(&s);
    }
    let mut stmt = s.conn().prepare("SELECT key FROM current_report_cards ORDER BY key").unwrap();
    stmt.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
}

fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut out = Vec::new();
    for i in 0..items.len() {
        let mut rest = items.to_vec();
        let first = rest.remove(i);
        for mut p in permutations(&rest) {
            p.insert(0, first.clone());
            out.push(p);
        }
    }
    out
}

/// What counts is the same however the rows were written.
fn survivors_in_any_order(rows: &[Seed]) -> Vec<String> {
    let all: Vec<Vec<String>> = permutations(rows).iter().map(|p| survivors(p)).collect();
    assert!(all.windows(2).all(|w| w[0] == w[1]), "the order cards are read in changed the result: {all:?}");
    all.into_iter().next().unwrap()
}

#[test]
fn a_newer_card_replaces_a_task_it_grades_again() {
    let old = Seed::span("old", "x", (0, 30), 40);
    let new = Seed::span("new", "y", (0, 30), 50);
    assert_eq!(survivors_in_any_order(&[old, new]), ["new"]);
}

#[test]
fn overlap_counts_from_both_sides() {
    // The old task runs from minute 10 to 30; the newer card grades a task over these minutes.
    let cases = [
        ("starts inside", (20, 50), true),
        ("ends inside", (0, 20), true),
        ("covers it", (0, 59), true),
        ("inside it", (15, 20), true),
        ("shares only an endpoint", (30, 50), false),
        ("shares only the other endpoint", (0, 10), false),
        ("just after", (31, 50), false),
        ("just before", (0, 9), false),
    ];
    for (name, new, overlaps) in cases {
        let old = Seed::span("old", "x", (10, 30), 40);
        let newer = Seed::span("new", "y", new, 50);
        let want: &[&str] = if overlaps { &["new"] } else { &["new", "old"] };
        assert_eq!(survivors_in_any_order(&[old, newer]), want, "{name}");
    }
}

#[test]
fn an_older_card_never_hides_a_newer_one() {
    // The same overlap with the cards the other way round: the newer card always wins.
    let a = Seed::span("a", "x", (0, 30), 50);
    let b = Seed::span("b", "y", (20, 50), 40);
    assert_eq!(survivors_in_any_order(&[a, b]), ["a"]);
}

#[test]
fn a_short_card_leaves_the_other_tasks_alone() {
    let first = Seed::span("first", "x", (0, 10), 40);
    let second = Seed::span("second", "x", (20, 30), 40);
    let third = Seed::span("third", "x", (40, 50), 40);
    let regrade = Seed::span("regrade", "y", (0, 10), 55);
    assert_eq!(survivors_in_any_order(&[first, second, third, regrade]), ["regrade", "second", "third"]);
}

#[test]
fn a_tie_goes_to_more_rounds_then_to_the_key() {
    let at_same_time = |key: &str, card: &str, rounds| Seed { rounds, ..Seed::span(key, card, (0, 10), 40) };
    assert_eq!(survivors_in_any_order(&[at_same_time("a", "x", 2), at_same_time("b", "y", 5)]), ["b"], "more rounds");
    assert_eq!(survivors_in_any_order(&[at_same_time("a", "x", 3), at_same_time("b", "y", 3)]), ["b"], "then the key");
    assert_eq!(survivors_in_any_order(&[at_same_time("b", "x", 3), at_same_time("a", "y", 3)]), ["b"]);
    // The time comes before either.
    let later = Seed { rounds: 1, ..Seed::span("z", "z", (0, 10), 41) };
    assert_eq!(survivors_in_any_order(&[at_same_time("b", "x", 9), later]), ["z"]);
}

#[test]
fn rows_of_one_card_and_of_other_chats_dont_hide_each_other() {
    // One card with two tasks that overlap (a model slip) keeps both.
    let a = Seed::span("a", "x", (0, 20), 40);
    let b = Seed::span("b", "x", (10, 30), 41);
    assert_eq!(survivors_in_any_order(&[a, b]), ["a", "b"]);
    let here = Seed::span("here", "x", (0, 10), 40);
    let there = Seed { session: "s2".into(), ..Seed::span("there", "y", (0, 10), 50) };
    assert_eq!(survivors_in_any_order(&[here, there]), ["here", "there"]);
}

#[test]
fn a_chain_of_regrades_leaves_only_the_newest() {
    let rows = [
        Seed::span("a", "c1", (0, 30), 10),
        Seed::span("b", "c2", (0, 30), 20),
        Seed::span("c", "c3", (0, 10), 30),
        Seed::span("d", "c4", (40, 50), 35),
        Seed::span("e", "c5", (45, 59), 36),
    ];
    // b hides a; c hides b and a; e hides d.
    assert_eq!(survivors_in_any_order(&rows), ["c", "e"]);
}

#[test]
fn the_same_first_prompt_is_the_same_task_whatever_the_span() {
    // A newer card's task that starts at the same prompt replaces it, however far it runs.
    let mut old = Seed::span("old", "x", (0, 10), 40);
    let mut newer = Seed::span("new", "y", (20, 30), 50);
    (old.first, newer.first) = ("same".into(), "same".into());
    assert_eq!(survivors_in_any_order(&[old.clone(), newer.clone()]), ["new"]);
    // Not another source's prompt, though: its key can only match by accident.
    let elsewhere = Seed { source: "codex", ..newer };
    assert_eq!(survivors_in_any_order(&[old, elsewhere]), ["new", "old"]);
}

#[test]
fn a_task_ending_where_the_next_starts_is_not_wiped_by_a_card_for_the_next() {
    // A model put task A's last prompt and task B's first prompt on the same prompt, minute 30.
    let a = Seed::span("a", "x", (0, 30), 40);
    let b = Seed::span("b", "y", (30, 50), 50);
    assert_eq!(survivors_in_any_order(&[a, b]), ["a", "b"]);
}

#[test]
fn a_resumed_copy_graded_again_replaces_the_original_card() {
    let mut lab = Lab::new();
    // The resumed chat is a new file and a new session, with the same prompts and the old card.
    let c1 = card("c1", &t(40), &lab.seal("chat1"), &[task("u")]);
    lab.write("chat1", &(work("u", 5) + &c1));
    lab.write("chat2", &(work("u", 5) + &c1 + &card("c2", &t(50), &lab.seal("chat2"), &[task("u")])));
    lab.sync();
    let rows = stored(&lab.store);
    assert_eq!(rows.len(), 2, "the original card, and the re-grade in the resumed copy");
    assert_ne!(rows[0].session, rows[1].session);
    assert_eq!(rows[0].first, rows[1].first, "the same task");
    let current: Vec<String> = lab
        .store
        .conn()
        .prepare("SELECT card FROM current_report_cards")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(current, [hash(&["c2"])], "only the newer grade counts");
}

#[test]
fn the_view_and_the_engine_agree_on_what_overlaps() {
    // Every pair of spans over five prompts, with and without a shared first prompt: the older
    // row stays exactly when the engine says the newer card's task doesn't overlap it.
    let spans: Vec<(u32, u32)> = (0..5).flat_map(|from| (from..5).map(move |to| (from, to))).collect();
    for old in &spans {
        for newer in &spans {
            for same_first in [false, true] {
                let a = Seed { first: "a".into(), ..Seed::span("old", "x", *old, 40) };
                let b = Seed { first: if same_first { "a" } else { "b" }.into(), ..Seed::span("new", "y", *newer, 50) };
                let want: &[&str] =
                    if engine::tasks_overlap(*old, *newer, same_first) { &["new"] } else { &["new", "old"] };
                assert_eq!(survivors(&[a, b]), want, "old {old:?}, newer {newer:?}, same first prompt: {same_first}");
            }
        }
    }
}

// ---- the numbers the Improve tab reads ---------------------------------------------------------

fn d(s: &str) -> NaiveDate {
    s.parse().unwrap()
}

/// One row in a chat of its own, so nothing hides it.
fn row(key: &str, day: &'static str, grade: &'static str) -> Seed {
    Seed { session: format!("session-{key}"), day, grade, ..Seed::new(key) }
}

fn task_row(s: &Store, day: &str, source: &str, family: &str) {
    s.conn()
        .execute(
            "INSERT INTO tasks (day, source, session, provider, family, version, signals)
             VALUES (?1, ?2, 's', 'x', ?3, '5.5', 0)",
            params![day, source, family],
        )
        .unwrap();
}

#[test]
fn weeks_sum_the_cards_that_still_count() {
    let s = Store::open_in_memory().unwrap();
    let opus = Usage { input: 1_000_000, cache_write: 0, cache_read: 0, output: 100_000 };
    let gpt = Usage { input: 1_000, cache_write: 0, cache_read: 0, output: 500 };
    let rows = [
        Seed { tokens: Usage::default(), ..row("a", "2026-10-05", "A") },
        Seed {
            avoidable: 1,
            missing: habit("finish") | habit("context"),
            tokens: opus,
            minutes: 7,
            ..row("b", "2026-10-11", "B") // a Sunday: the same week as the Monday
        },
        Seed {
            avoidable: 2,
            missing: habit("target"),
            tokens: gpt,
            minutes: 3,
            family: "GPT",
            ..row("c", "2026-10-07", "C")
        },
        Seed { avoidable: 1, family: "GPT", ..row("d", "2026-10-13", "B") },
        // Older than "a", the same task in the same chat: replaced, so it counts for nothing.
        Seed {
            session: "session-a".into(),
            first: "first-a".into(),
            graded: stamp(5, 1),
            grade: "F",
            avoidable: 4,
            ..Seed::new("old")
        },
    ];
    for r in &rows {
        r.save(&s);
    }
    for _ in 0..3 {
        task_row(&s, "2026-10-06", "claude-code", "Claude Opus");
    }
    task_row(&s, "2026-10-13", "codex", "GPT");
    for _ in 0..5 {
        task_row(&s, "2026-10-06", "chatgpt-export", "GPT"); // can't have a card
    }

    let p = s.progress(&Filter::default(), d("2026-10-14")).unwrap();
    let weeks: Vec<_> = p.weeks.iter().map(|w| (w.week.to_string(), &w.report_cards)).collect();
    assert_eq!(weeks.iter().map(|w| w.0.as_str()).collect::<Vec<_>>(), ["2026-10-05", "2026-10-12"]);
    let w = weeks[0].1;
    assert_eq!((w.graded, w.avoidable, w.gradable_tasks), (3, 3, 3));
    assert_eq!(w.grades, [("A", 1), ("B", 1), ("C", 1), ("D", 0), ("F", 0)].map(|(g, n)| (g.to_string(), n)).into());
    assert_eq!((w.extra_tokens, w.extra_minutes), (1_100_000 + 1_500, 10));
    // Only the Opus task has a price: 1M input at $4 and 100k output at $20, per million.
    assert!((w.extra_cost.unwrap() - 6.0).abs() < 1e-9, "{:?}", w.extra_cost);
    // Two of the week's three tasks used Opus: a cost per task is over those, not all three.
    assert_eq!((w.graded, w.priced), (3, 2));
    let missing: Vec<_> = w.missing.iter().map(|(k, n)| (k.as_str(), *n)).collect();
    assert_eq!(
        missing,
        [("context", 1), ("evidence", 0), ("finish", 1), ("guardrails", 0), ("purpose", 0), ("target", 1)]
    );

    let w = weeks[1].1;
    assert_eq!((w.graded, w.avoidable, w.extra_tokens, w.gradable_tasks), (1, 1, 0, 1));
    assert_eq!((w.extra_cost, w.priced), (None, 0), "no priced model, so no cost to show");

    // The model filter narrows the cards and the tasks alike.
    let opus_only = Filter { family: Some("Claude Opus".into()), ..Filter::default() };
    let p = s.progress(&opus_only, d("2026-10-14")).unwrap();
    assert_eq!((p.weeks[0].report_cards.graded, p.weeks[0].report_cards.gradable_tasks), (2, 3));
    assert_eq!(p.weeks[1].report_cards, ReportCardWeek::default());
    assert_eq!(p.weeks[0].report_cards.extra_cost.map(|c| (c * 1000.0).round()), Some(6000.0));
}

#[test]
fn the_totals_add_up_the_weeks_in_the_range() {
    let s = Store::open_in_memory().unwrap();
    let opus = Usage { input: 1_000_000, cache_write: 0, cache_read: 0, output: 100_000 };
    let gpt = Usage { input: 1_000, cache_write: 0, cache_read: 0, output: 500 };
    for r in [
        Seed { avoidable: 1, tokens: opus, minutes: 7, missing: habit("finish"), ..row("a", "2026-10-05", "B") },
        Seed {
            avoidable: 2,
            tokens: gpt,
            minutes: 3,
            family: "GPT",
            missing: habit("finish") | habit("target"),
            ..row("b", "2026-10-13", "C")
        },
        Seed { ..row("c", "2026-08-03", "A") },
    ] {
        r.save(&s);
    }
    let all = s.report_card_totals(&Filter::default()).unwrap();
    assert_eq!((all.graded, all.priced, all.avoidable, all.extra_minutes), (3, 2, 3, 10));
    assert_eq!(all.grades, [("A", 1), ("B", 1), ("C", 1), ("D", 0), ("F", 0)].map(|(g, n)| (g.to_string(), n)).into());
    assert!((all.extra_cost.unwrap() - 6.0).abs() < 1e-9, "priced tasks only: {:?}", all.extra_cost);
    assert_eq!(all.extra_tokens, 1_100_000 + 1_500);
    assert_eq!((all.missing["finish"], all.missing["target"], all.missing["context"]), (2, 1, 0));

    // The same range and models as the rest of the stats.
    let since = Filter { since: Some(d("2026-10-01")), ..Filter::default() };
    assert_eq!(s.report_card_totals(&since).unwrap().graded, 2);
    let gpt_only = Filter { family: Some("GPT".into()), ..Filter::default() };
    let t = s.report_card_totals(&gpt_only).unwrap();
    assert_eq!((t.graded, t.priced, t.extra_cost), (1, 0, None), "no price, no cost");
    let none = s.report_card_totals(&Filter { source: Some("codex".into()), ..Filter::default() }).unwrap();
    assert_eq!(none, ReportCardWeek::default());
}

#[test]
fn weeks_with_no_cards_are_empty_and_the_shape_is_stable() {
    let s = Store::open_in_memory().unwrap();
    task_row(&s, "2026-10-06", "claude-code", "Claude Opus");
    let p = s.progress(&Filter::default(), d("2026-10-14")).unwrap();
    let rc = &p.weeks[0].report_cards;
    assert_eq!((rc.graded, rc.gradable_tasks, rc.extra_cost), (0, 1, None));
    assert!(rc.grades.is_empty() && rc.missing.is_empty(), "no keys until something is graded");
    let json = serde_json::to_value(HistoryWeek::default()).unwrap();
    let mut keys: Vec<_> = json["report_cards"].as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "avoidable",
            "extra_cost",
            "extra_minutes",
            "extra_tokens",
            "gradable_tasks",
            "graded",
            "grades",
            "missing",
            "priced"
        ]
    );

    // Cards for tasks whose own rows are gone (a rebuild, a deleted chat) still make their week.
    Seed::new("k").save(&s);
    let p = s.progress(&Filter::default(), d("2026-10-14")).unwrap();
    assert_eq!(p.weeks[0].report_cards.graded, 1);
}

#[test]
fn the_range_filter_applies_to_the_weeks() {
    let s = Store::open_in_memory().unwrap();
    row("a", "2026-10-05", "A").save(&s);
    row("b", "2026-10-12", "A").save(&s);
    let since = Filter { since: Some(d("2026-10-10")), ..Filter::default() };
    let p = s.progress(&since, d("2026-10-14")).unwrap();
    let graded: Vec<i64> = p.weeks.iter().map(|w| w.report_cards.graded).collect();
    assert_eq!(graded, [1], "only the range's weeks");
}

#[test]
fn recent_cards_come_newest_first_each_with_its_own_first_missing_habit() {
    let s = Store::open_in_memory().unwrap();
    let tokens = Usage { input: 1_000_000, cache_write: 0, cache_read: 0, output: 0 };
    let miss = |key, graded, missing| Seed { graded: stamp(5, graded), missing, ..row(key, "2026-10-05", "B") };
    miss("oldest", 10, habit("evidence") | habit("guardrails")).save(&s);
    miss("none", 20, 0).save(&s);
    Seed { tokens, minutes: 7, ..miss("middle", 30, habit("target") | habit("finish")) }.save(&s);
    Seed { family: "GPT", day: "2026-10-06", source: "codex", ..miss("newest", 40, habit("target") | habit("finish")) }
        .save(&s);
    miss("fifth", 5, habit("finish")).save(&s);

    let all = s.recent_report_cards(&Filter::default(), 10).unwrap();
    // finish is missed on 3 cards and target on 2, but each card shows its own first miss.
    let got: Vec<_> = all.iter().map(|r| (r.day.as_str(), r.source.as_str(), r.missing.as_deref())).collect();
    assert_eq!(
        got,
        [
            ("2026-10-06", "codex", Some("target")),
            ("2026-10-05", "claude-code", Some("target")),
            ("2026-10-05", "claude-code", None),
            ("2026-10-05", "claude-code", Some("evidence")),
            ("2026-10-05", "claude-code", Some("finish")),
        ],
        "the first habit on the card, whatever the other cards miss"
    );
    assert_eq!(all[0].extra_cost, None, "GPT has no price");
    assert_eq!((all[1].extra_tokens, all[1].grade.as_str()), (1_000_000, "B"));
    assert_eq!((all[0].extra_minutes, all[1].extra_minutes), (0, 7), "minutes come with every card, priced or not");
    assert!((all[1].extra_cost.unwrap() - 4.0).abs() < 1e-9);
    assert_eq!(s.recent_report_cards(&Filter::default(), 2).unwrap().len(), 2);
    let codex = Filter { source: Some("codex".into()), ..Filter::default() };
    assert_eq!(s.recent_report_cards(&codex, 5).unwrap().len(), 1);
    assert_eq!(Store::open_in_memory().unwrap().recent_report_cards(&Filter::default(), 5).unwrap(), vec![]);
}

/// `n` cards in order, each in a chat of its own: row `i` is graded by `make(i)`.
fn history_of(s: &Store, n: usize, make: impl Fn(usize) -> Seed) {
    for i in 0..n {
        let seed = make(i);
        let key = format!("{}-{i}", seed.key);
        let start = cards::ts_str(format!("2026-08-01T{:02}:{:02}:00Z", i / 60, i % 60).parse().unwrap());
        let first = format!("first-{key}");
        Seed {
            card: key.clone(),
            session: format!("session-{key}"),
            first,
            key,
            start: start.clone(),
            end: start,
            ..seed
        }
        .save(s);
    }
}

#[test]
fn the_improvement_headline_compares_your_first_and_latest_twenty() {
    let s = Store::open_in_memory().unwrap();
    // First 20: 7 A's and 13 C's, 12 missing the finish line, 2 missing the target.
    // Last 20: 14 A's and 6 C's, 4 missing the finish line, 5 missing the target.
    history_of(&s, 40, |i| {
        let k = i % 20;
        let (good, no_finish, no_target) = if i < 20 { (k < 7, k < 12, k >= 18) } else { (k < 14, k < 4, k >= 15) };
        let missing = (habit("finish") * u8::from(no_finish)) | (habit("target") * u8::from(no_target));
        Seed { grade: if good { "A" } else { "C" }, missing, ..Seed::new("m") }
    });
    let trend = |f: &Filter| s.progress(f, d("2026-10-14")).unwrap().insights.report_cards;
    let got = trend(&Filter::default()).unwrap();
    assert_eq!((got.first_ab, got.last_ab), (35, 70));
    assert_eq!(got.habit, Some(HabitDrop { habit: "finish".into(), first: 12, last: 4 }));

    // All of history under the model filter, not the range the screen shows.
    let range = Filter { since: Some(d("2026-11-01")), ..Filter::default() };
    assert_eq!(trend(&range), Some(got.clone()));
    assert_eq!(s.progress(&range, d("2026-10-14")).unwrap().weeks.len(), 0, "while the weeks follow the range");
    let sonnet = Filter { family: Some("Claude Sonnet".into()), ..Filter::default() };
    assert_eq!(trend(&sonnet), None);
}

#[test]
fn the_improvement_headline_waits_for_forty_and_only_compares_one_kind() {
    let s = Store::open_in_memory().unwrap();
    history_of(&s, 39, |_| Seed::new("m"));
    let trend = || s.progress(&Filter::default(), d("2026-10-14")).unwrap().insights.report_cards;
    assert_eq!(trend(), None, "39 is not enough");
    history_of(&s, 1, |_| Seed::new("late"));
    let t = trend().unwrap();
    assert_eq!((t.first_ab, t.last_ab, t.habit.clone()), (100, 100, None), "nothing got better: no habit");
}

#[test]
fn the_best_week_needs_five_graded_tasks_and_ignores_the_model_filter() {
    let s = Store::open_in_memory().unwrap();
    let week = |monday: &'static str, grades: &[&'static str]| {
        for (i, g) in grades.iter().copied().enumerate() {
            row(&format!("{monday}-{i}"), monday, g).save(&s);
        }
    };
    week("2026-10-05", &["A", "A", "A", "A", "C"]); // 80%
    week("2026-10-12", &["A", "A", "A", "A"]); // 100%, but only four
    week("2026-10-19", &["B", "B", "B", "B", "F"]); // 80% again: the later one wins a tie
    let best = |f: &Filter| s.progress(f, d("2026-10-28")).unwrap().bests.report_card;
    assert_eq!(best(&Filter::default()), Some((80, d("2026-10-19"))));
    let other = Filter { family: Some("Claude Sonnet".into()), ..Filter::default() };
    assert_eq!(best(&other), Some((80, d("2026-10-19"))), "bests are across all AI");
    assert_eq!(
        Store::open_in_memory().unwrap().progress(&Filter::default(), d("2026-10-28")).unwrap().bests.report_card,
        None
    );
}

// ---- the straight A's milestone ----------------------------------------------------------------

fn straight_week(s: &Store, monday: &'static str, n: usize, last: &'static str) {
    for i in 0..n {
        row(&format!("{monday}-{i}"), monday, if i + 1 == n { last } else { "A" }).save(s);
    }
}

#[test]
fn a_first_straight_a_week_is_announced_once() {
    let s = Store::open_in_memory().unwrap();
    let today = d("2026-10-28"); // a Wednesday: the week of the 26th is still open
    assert_eq!(s.advance_card_milestones(today, true).unwrap(), vec![]);
    assert_eq!(s.get_setting("milestone.straight_as").unwrap(), None, "no baseline until there is a card");

    straight_week(&s, "2026-10-05", 5, "B"); // not straight A's
    straight_week(&s, "2026-10-12", 4, "A"); // too few
    straight_week(&s, "2026-10-26", 5, "A"); // not over yet
    assert_eq!(s.advance_card_milestones(today, true).unwrap(), vec![], "the first look only records a baseline");
    assert_eq!(s.get_setting("milestone.straight_as").unwrap().as_deref(), Some("0"));
    assert_eq!(s.straight_a_weeks(today).unwrap(), vec![]);

    straight_week(&s, "2026-10-19", 6, "A");
    assert_eq!(s.straight_a_weeks(today).unwrap(), vec![(d("2026-10-19"), 6)]);
    let popups = s.advance_card_milestones(today, true).unwrap();
    assert_eq!(popups.len(), 1);
    assert_eq!(popups[0].title, "Straight A's!");
    assert_eq!(popups[0].body, "Every report card last week was an A.");
    assert_eq!(s.advance_card_milestones(today, true).unwrap(), vec![], "once");
    // The week ends, and with it the one that was open: still no second popup.
    assert_eq!(s.advance_card_milestones(d("2026-11-03"), true).unwrap(), vec![], "only the first ever");
    assert_eq!(s.get_setting("milestone.straight_as").unwrap().as_deref(), Some("2"));
}

#[test]
fn a_bulk_read_moves_the_baseline_but_never_announces_an_old_week() {
    let s = Store::open_in_memory().unwrap();
    let today = d("2026-10-28");
    straight_week(&s, "2026-10-05", 5, "B"); // a first card, not straight A's: the baseline is 0
    assert_eq!(s.advance_card_milestones(today, true).unwrap(), vec![]);
    assert_eq!(s.get_setting("milestone.straight_as").unwrap().as_deref(), Some("0"));

    // A source just allowed (or a rebuild, or a big first pass) brings a straight-A week from before.
    straight_week(&s, "2026-10-12", 5, "A");
    assert_eq!(s.advance_card_milestones(today, false).unwrap(), vec![], "history arriving is not news");
    assert_eq!(s.get_setting("milestone.straight_as").unwrap().as_deref(), Some("1"), "but it is counted");
    // So when the next week comes in the ordinary way, that wasn't the first.
    straight_week(&s, "2026-10-19", 5, "A");
    assert_eq!(s.advance_card_milestones(today, true).unwrap(), vec![], "it already had its first");

    // The same weeks arriving in an ordinary pass are news.
    let fresh = Store::open_in_memory().unwrap();
    straight_week(&fresh, "2026-10-05", 5, "B");
    fresh.advance_card_milestones(today, true).unwrap();
    straight_week(&fresh, "2026-10-12", 5, "A");
    assert_eq!(fresh.advance_card_milestones(today, true).unwrap().len(), 1, "a normal pass announces it");
}

#[test]
fn straight_a_weeks_that_were_already_there_stay_quiet() {
    let s = Store::open_in_memory().unwrap();
    straight_week(&s, "2026-10-05", 5, "A"); // read in by an upgrade or a backfill
    assert_eq!(s.advance_card_milestones(d("2026-10-28"), true).unwrap(), vec![]);
    straight_week(&s, "2026-10-12", 5, "A");
    assert_eq!(s.advance_card_milestones(d("2026-10-28"), true).unwrap(), vec![], "that was already their first");
    assert_eq!(straight_as(0, &[], d("2026-10-28")), None);
    let older = straight_as(0, &[(d("2026-10-05"), 5)], d("2026-10-28")).unwrap();
    assert_eq!(
        older.body, "Every report card the week of Oct 5 was an A.",
        "not last week: say which, no zero padding"
    );
    let two_digits = straight_as(0, &[(d("2026-10-12"), 5)], d("2026-11-25")).unwrap();
    assert_eq!(two_digits.body, "Every report card the week of Oct 12 was an A.", "and not an ISO date");
    // Cards going away later never re-arms it.
    s.delete_report_cards().unwrap();
    assert_eq!(s.advance_card_milestones(d("2026-10-28"), true).unwrap(), vec![]);
    straight_week(&s, "2026-10-19", 5, "A");
    assert_eq!(s.advance_card_milestones(d("2026-10-28"), true).unwrap(), vec![]);
}

#[test]
fn a_week_a_newer_card_changed_is_judged_as_it_now_stands() {
    let s = Store::open_in_memory().unwrap();
    straight_week(&s, "2026-10-12", 5, "A");
    // The fifth task is re-graded by a later card: no longer all A's.
    assert_eq!(s.straight_a_weeks(d("2026-10-28")).unwrap().len(), 1);
    Seed {
        session: "session-2026-10-12-4".into(),
        card: "later".into(),
        day: "2026-10-12",
        grade: "D",
        avoidable: 3,
        graded: cards::ts_str("2027-01-01T00:00:00Z".parse().unwrap()),
        ..Seed::new("regrade")
    }
    .save(&s);
    assert_eq!(s.straight_a_weeks(d("2026-10-28")).unwrap(), vec![], "the D replaced the A it overlaps");
}

// ---- the tables ---------------------------------------------------------------------------------

#[test]
fn the_card_table_is_not_a_score_table() {
    assert!(!SCORE_TABLES.contains(&"report_cards"));
}

// ---- pricing and time, round by round -----------------------------------------------------------------

/// Three prompts ten minutes apart: round one answered by `first`, rounds two and three by `later`.
fn two_models(first: &str, later: &str) -> String {
    cc_user("w1", &at(5, 0), "build the settings page with tabs")
        + &reply_on(first, "wr1", &at(5, 2), "Built it.", (10, 100, 1000, 50))
        + &cc_user("w2", &at(5, 10), "use tailwind for the styles")
        + &reply_on(later, "wr2", &at(5, 12), "Restyled.", (20, 0, 2000, 80))
        + &cc_user("w3", &at(5, 20), "also keep the old route working")
        + &reply_on(later, "wr3", &at(5, 22), "Done.", (5, 0, 500, 20))
}

fn changed_models() -> Value {
    json!({"at":id("w1"),"to":id("w3"),"waste":[[id("w2"),100],[id("w3"),50]]})
}

#[test]
fn each_rounds_tokens_are_kept_under_the_model_that_answered_it() {
    let mut lab = Lab::new();
    let body = two_models("claude-opus-5-5", "claude-sonnet-5-5");
    lab.write("chat1", &(body + &card("c1", &t(40), &lab.seal("chat1"), &[changed_models()])));
    lab.sync();
    let r = &stored(&lab.store)[0];
    // The second prompt redid all of round one (Opus). The third redid half of rounds one and two:
    // half of Opus's, and half of Sonnet's.
    let opus_tokens = Usage { input: 10 + 5, cache_write: 100 + 50, cache_read: 1000 + 500, output: 50 + 25 };
    let sonnet_tokens = Usage { input: 10, cache_write: 0, cache_read: 1000, output: 40 };
    assert_eq!(
        r.tokens,
        [("claude-opus-5-5".to_string(), opus_tokens), ("claude-sonnet-5-5".to_string(), sonnet_tokens)].into()
    );
    assert_eq!(r.minutes, 20, "(10 minutes × 100% + 20 × 50%)");
    assert_eq!(
        (r.family.as_str(), r.version.as_str()),
        ("Claude Opus", "5.5"),
        "the task is filed under its first round's model"
    );

    // The cost is each model's tokens at its own price.
    let want = opus_tokens.cost_on("claude-opus-5-5").unwrap() + sonnet_tokens.cost_on("claude-sonnet-5-5").unwrap();
    let recent = lab.store.recent_report_cards(&Filter::default(), 5).unwrap();
    assert!((recent[0].extra_cost.unwrap() - want).abs() < 1e-12, "{:?} vs {want}", recent[0].extra_cost);
    assert_eq!(recent[0].extra_tokens, opus_tokens.shown() + sonnet_tokens.shown());
    let totals = lab.store.report_card_totals(&Filter::default()).unwrap();
    assert_eq!(totals.priced, 1);
    assert!((totals.extra_cost.unwrap() - want).abs() < 1e-12);
}

#[test]
fn a_task_with_any_unpriced_round_is_left_out_of_the_cost_but_not_the_tokens() {
    let mut lab = Lab::new();
    let body = two_models("claude-opus-5-5", "gpt-5.1");
    lab.write("chat1", &(body + &card("c1", &t(40), &lab.seal("chat1"), &[changed_models()])));
    // A second chat that stayed on one priced model.
    lab.write("chat2", &(work("u", 5) + &card("c2", &t(40), &lab.seal("chat2"), &[task("u")])));
    lab.sync();
    let totals = lab.store.report_card_totals(&Filter::default()).unwrap();
    assert_eq!((totals.graded, totals.priced), (2, 1), "only the task that stayed on Opus is priced");
    let only_opus = Usage { input: 5, cache_write: 50, cache_read: 500, output: 25 };
    assert!((totals.extra_cost.unwrap() - only_opus.cost_on("claude-opus-5-5").unwrap()).abs() < 1e-12);
    let recent = lab.store.recent_report_cards(&Filter::default(), 5).unwrap();
    let mixed = recent.iter().find(|c| c.extra_cost.is_none()).expect("the mixed task has no cost");
    assert!(mixed.extra_tokens > 0 && totals.extra_tokens > mixed.extra_tokens);
}

#[test]
fn minutes_are_summed_in_seconds_and_rounded_once_at_the_end() {
    let mut lab = Lab::new();
    let sec = |s: u32| format!("2026-10-05T12:{:02}:{:02}Z", s / 60, s % 60);
    // Four prompts 40 seconds apart. The two follow-ups redo 40 s and then 80 s of work.
    let body = cc_user("s1", &sec(0), "build the settings page with tabs")
        + &reply("sr1", &sec(10), "Built it.", (1, 1, 1, 1))
        + &cc_user("s2", &sec(40), "use tailwind for the styles")
        + &reply("sr2", &sec(50), "Restyled.", (1, 1, 1, 1))
        + &cc_user("s3", &sec(80), "also keep the old route working")
        + &reply("sr3", &sec(90), "Done.", (1, 1, 1, 1))
        + &cc_user("s4", &sec(120), "and add a link to the nav bar")
        + &reply("sr4", &sec(130), "Done.", (1, 1, 1, 1));
    let both = json!({"at":id("s1"),"to":id("s4"),"waste":[[id("s2"),100],[id("s3"),100]]});
    lab.write("chat1", &(body + &card("c1", &t(40), &lab.seal("chat1"), &[both])));
    lab.sync();
    assert_eq!(stored(&lab.store)[0].minutes, 2, "40 s + 80 s = 120 s; a minute per round would have said 0");
}

#[test]
fn recent_cards_say_the_local_day_they_were_written_and_each_source_its_last() {
    let s = Store::open_in_memory().unwrap();
    assert_eq!(s.last_report_card_days().unwrap(), vec![]);
    let graded = |key, day, source, stamp| Seed { day, source, graded: stamp, ..row(key, day, "A") };
    graded("a", "2026-10-05", "claude-code", stamp(5, 10)).save(&s);
    graded("b", "2026-10-05", "claude-code", stamp(7, 30)).save(&s); // the task began on the 5th, the card came on the 7th
    graded("c", "2026-10-06", "codex", stamp(6, 15)).save(&s);
    let recent = s.recent_report_cards(&Filter::default(), 5).unwrap();
    let days: Vec<_> = recent.iter().map(|r| (r.day.as_str(), r.graded_day.to_string())).collect();
    assert_eq!(
        days,
        [
            ("2026-10-05", "2026-10-07".to_string()),
            ("2026-10-06", "2026-10-06".into()),
            ("2026-10-05", "2026-10-05".into())
        ]
    );
    assert_eq!(
        s.last_report_card_days().unwrap(),
        [("claude-code".to_string(), d("2026-10-07")), ("codex".to_string(), d("2026-10-06"))]
    );
    let json = serde_json::to_value(&recent[0]).unwrap();
    assert_eq!(json["graded_day"], "2026-10-07");
}

#[test]
fn recent_cards_read_only_as_many_as_asked() {
    let s = Store::open_in_memory().unwrap();
    for (i, key) in ["a", "b", "c", "d"].iter().enumerate() {
        Seed { graded: stamp(5, 10 + i as u32), ..row(key, "2026-10-05", "A") }.save(&s);
    }
    let two = s.recent_report_cards(&Filter::default(), 2).unwrap();
    assert_eq!(two.len(), 2);
    assert!(two[0].graded_day >= two[1].graded_day);
    assert_eq!(s.recent_report_cards(&Filter::default(), 0).unwrap(), vec![]);
    assert_eq!(s.recent_report_cards(&Filter::default(), 99).unwrap().len(), 4);
}

#[test]
fn a_files_checkpoint_carries_the_last_card_it_read() {
    let mut lab = Lab::new();
    lab.write("chat1", &(work("u", 5) + &card("c1", &t(40), &lab.seal("chat1"), &[task("u")])));
    let path = lab.path("chat1");
    assert!(lab.store.file_checkpoint(&path.to_string_lossy()).unwrap().is_none());
    lab.sync();
    let cp = lab.store.file_checkpoint(&path.to_string_lossy()).unwrap().unwrap();
    assert!(cp.cards_to.is_some(), "so the next pass knows this card is stored");
    assert!(cp.at > 0 && cp.meta.size > 0);
    lab.write("chat2", &work("v", 6));
    lab.sync();
    let none = lab.store.file_checkpoint(&lab.path("chat2").to_string_lossy()).unwrap().unwrap();
    assert_eq!(none.cards_to, None, "a chat with no card has none read");
}

#[test]
fn cowork_chats_are_not_counted_as_able_to_carry_a_card() {
    let s = Store::open_in_memory().unwrap();
    row("a", "2026-10-06", "A").save(&s);
    for source in ["claude-code", "claude-cowork", "codex", "chatgpt-export"] {
        task_row(&s, "2026-10-06", source, "Claude Opus");
    }
    let p = s.progress(&Filter::default(), d("2026-10-14")).unwrap();
    assert_eq!(p.weeks[0].report_cards.gradable_tasks, 2, "Claude Code and Codex only");
}

// ---- tips ----------------------------------------------------------------------------------------

const TIP: &str = "Tip from No, Actually...: say which page when you first ask. [target, redid 50%]";

/// What Claude Code saves when a session-start hook gives a chat the tip rules.
fn rules(uuid: &str, ts: &str) -> String {
    line(&json!({"type":"attachment","uuid":uuid,"timestamp":ts,"attachment":{"type":"hook_additional_context",
        "content":[engine::TIP_RULES],"hookName":"SessionStart","toolUseID":"SessionStart","hookEvent":"SessionStart"}}))
}

/// A task on October `day` whose follow-up got a tip, from `model`.
fn tipped_on(model: &str, p: &str, day: u32) -> String {
    let a = |m| at(day, m);
    cc_user(&format!("{p}1"), &a(0), "build the settings page with tabs")
        + &reply_on(model, &format!("{p}r1"), &a(2), "Built it.", (10, 100, 1000, 50))
        + &cc_user(&format!("{p}2"), &a(10), "use tailwind for the styles")
        + &reply_on(model, &format!("{p}r2"), &a(12), &format!("Restyled.\n\n{TIP}"), (20, 0, 2000, 80))
}

fn tipped(p: &str, day: u32) -> String {
    tipped_on("claude-opus-5-5", p, day)
}

fn tip_week_in(s: &Store, f: &Filter) -> TipWeek {
    s.progress(f, d("2026-10-14")).unwrap().weeks.first().map(|w| w.tips.clone()).unwrap_or_default()
}

fn tip_week(s: &Store) -> TipWeek {
    tip_week_in(s, &Filter::default())
}

fn rules_rebuild(lab: &mut Lab) {
    lab.store.set_setting("rules_version", "0").unwrap();
    lab.sync();
}

#[test]
fn a_tip_is_kept_as_numbers_and_counted_against_its_chats_tasks() {
    let mut lab = Lab::new();
    lab.write("s1", &(rules("h", &t(0)) + &tipped("p", 5)));
    lab.sync();
    let row: (String, String, u8, u8, String) = lab
        .store
        .conn
        .query_row("SELECT session, day, habit, redid, family FROM tips", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .unwrap();
    let family = engine::normalize_model("claude-opus-5-5").unwrap().family;
    assert_eq!(row, (hash(&["claude-code", "s1"]), "2026-10-05".to_string(), habit("target"), 50, family));
    assert_eq!(tip_week(&lab.store), TipWeek { tips: 1, tasks: 1 });
}

#[test]
fn reading_a_tip_again_keeps_one() {
    let mut lab = Lab::new();
    lab.write("s1", &(rules("h", &t(0)) + &tipped("p", 5)));
    lab.sync();
    lab.append("s1", &(cc_user("p3", &t(20), "thanks") + &reply("pr3", &t(21), "Welcome.", (1, 0, 100, 5))));
    lab.sync();
    lab.store.conn.execute("UPDATE files SET checkpoint = 0", []).unwrap();
    lab.append("s1", &(cc_user("p4", &t(30), "one more thing") + &reply("pr4", &t(31), "Sure.", (1, 0, 100, 5))));
    lab.sync();
    assert_eq!(count(&lab.store, "tips"), 1);
    assert_eq!(count(&lab.store, "tip_chats"), 1);
}

#[test]
fn a_forked_copy_of_a_chat_doesnt_count_twice() {
    let mut lab = Lab::new();
    let chat = rules("h", &t(0)) + &tipped("p", 5);
    lab.write("s1", &chat);
    lab.write("s2", &chat);
    lab.sync();
    assert_eq!(tip_week(&lab.store), TipWeek { tips: 1, tasks: 1 });
}

#[test]
fn a_reply_with_no_id_is_still_one_tip_each() {
    let mut lab = Lab::new();
    let no_id = |ts: &str| {
        line(&json!({"type":"assistant","timestamp":ts,"message":{"model":"claude-opus-5-5",
            "content":[{"type":"text","text":format!("Done.\n\n{TIP}")}]}}))
    };
    lab.write(
        "s1",
        &(cc_user("p1", &t(0), "fix the page")
            + &no_id(&t(1))
            + &cc_user("p2", &t(2), "and the other one")
            + &no_id(&t(3))),
    );
    lab.sync();
    assert_eq!(count(&lab.store, "tips"), 2, "they can't share a key");
}

#[test]
fn a_chat_without_the_rules_or_a_tip_isnt_counted() {
    let mut lab = Lab::new();
    lab.write("s1", &work("p", 5));
    lab.sync();
    assert_eq!(count(&lab.store, "tip_chats"), 0);
    assert_eq!(tip_week(&lab.store), TipWeek::default());
    assert!(count(&lab.store, "tasks") > 0);
}

#[test]
fn a_tip_brings_its_chat_into_the_count_without_the_rules_being_seen() {
    let mut lab = Lab::new();
    lab.write("s1", &tipped("p", 5));
    lab.sync();
    assert_eq!(tip_week(&lab.store), TipWeek { tips: 1, tasks: 1 });
}

#[test]
fn tasks_before_the_rules_arrived_dont_count() {
    let mut lab = Lab::new();
    lab.write("s1", &(work("a", 5) + &rules("h", &at(6, 0)) + &tipped("p", 6)));
    lab.sync();
    let all = count(&lab.store, "tasks");
    let week = tip_week(&lab.store);
    assert_eq!(week.tips, 1);
    assert!(week.tasks < all, "{} of {all}: the 5th is left out", week.tasks);
}

#[test]
fn from_now_on_skips_tips_from_before_the_cutoff() {
    let mut lab = Lab::new();
    lab.write("s1", &(rules("h", &t(0)) + &tipped("p", 5)));
    let src = lab.sources().remove(0);
    lab.store.start_from_now(&*src).unwrap();
    lab.store.reset_scores().unwrap(); // reads the file from the start, as a rebuild does
    lab.sync();
    assert_eq!((count(&lab.store, "tips"), count(&lab.store, "tip_chats")), (0, 0));
    let later = "2099-01-05T12:00:00Z";
    lab.append(
        "s1",
        &(cc_user("n1", later, "fix the page") + &reply("nr1", later, &format!("Done.\n\n{TIP}"), (1, 0, 1, 1))),
    );
    lab.sync();
    assert_eq!(
        (count(&lab.store, "tips"), count(&lab.store, "tip_chats")),
        (1, 1),
        "a chat seen only from here on still counts"
    );
}

#[test]
fn deleting_the_history_clears_tips_and_they_stay_gone() {
    let mut lab = Lab::new();
    lab.write("s1", &(rules("h", &t(0)) + &tipped("p", 5)));
    lab.sync();
    lab.store.delete_report_cards().unwrap();
    assert_eq!((count(&lab.store, "tips"), count(&lab.store, "tip_chats")), (0, 0));
    lab.store.reset_scores().unwrap();
    lab.sync();
    assert_eq!((count(&lab.store, "tips"), count(&lab.store, "tip_chats")), (0, 0), "a rebuild brings nothing back");
    assert!(count(&lab.store, "tasks") > 0, "the score does come back");
    let later = "2099-01-05T12:00:00Z";
    lab.append(
        "s1",
        &(cc_user("n1", later, "fix the page") + &reply("nr1", later, &format!("Done.\n\n{TIP}"), (1, 0, 1, 1))),
    );
    lab.sync();
    assert_eq!(count(&lab.store, "tips"), 1, "a new tip counts");
}

#[test]
fn a_rebuild_keeps_tips_and_tasks_in_step() {
    let mut lab = Lab::new();
    lab.write("s1", &(rules("h", &t(0)) + &tipped("p", 5)));
    lab.write("s2", &(rules("i", &t(0)) + &tipped("q", 5)));
    lab.sync();
    assert_eq!(tip_week(&lab.store), TipWeek { tips: 2, tasks: 2 });
    rules_rebuild(&mut lab);
    assert_eq!(tip_week(&lab.store), TipWeek { tips: 2, tasks: 2 }, "the same again");
    std::fs::remove_file(lab.path("s2")).unwrap();
    rules_rebuild(&mut lab);
    assert_eq!(tip_week(&lab.store), TipWeek { tips: 1, tasks: 1 }, "a transcript that's gone takes both with it");
}

#[test]
fn forget_deletes_a_sources_tips() {
    let mut lab = Lab::new();
    lab.write("s1", &(rules("h", &t(0)) + &tipped("p", 5)));
    lab.sync();
    lab.store.forget("claude-code").unwrap();
    assert_eq!((count(&lab.store, "tips"), count(&lab.store, "tip_chats")), (0, 0));
}

#[test]
fn tips_follow_the_model_filter() {
    let mut lab = Lab::new();
    lab.write("s1", &(rules("h", &t(0)) + &tipped_on("claude-opus-5-5", "p", 5)));
    lab.write("s2", &(rules("i", &t(0)) + &tipped_on("claude-sonnet-5-5", "q", 5)));
    lab.sync();
    let family = engine::normalize_model("claude-sonnet-5-5").unwrap().family;
    let f = Filter { family: Some(family), ..Filter::default() };
    assert_eq!(tip_week_in(&lab.store, &f), TipWeek { tips: 1, tasks: 1 });
    assert_eq!(tip_week(&lab.store), TipWeek { tips: 2, tasks: 2 });
}

/// Puts a tip about `name` in the table, `minute` minutes into the day.
fn seed_tip(s: &Store, n: u32, name: &str) {
    s.conn
        .execute(
            "INSERT INTO tips (key, source, session, day, ts, provider, family, version, habit, redid)
             VALUES (?1,'claude-code','s','2026-10-05',?2,'p','f','v',?3,50)",
            params![format!("k{n:03}"), format!("2026-10-05T{:02}:{:02}:00Z", n / 60, n % 60), habit(name)],
        )
        .unwrap();
}

fn top(s: &Store) -> Option<TopTip> {
    s.progress(&Filter::default(), d("2026-10-14")).unwrap().insights.top_tip
}

#[test]
fn the_top_tip_waits_for_enough_tips() {
    let s = Store::open_in_memory().unwrap();
    for n in 0..4 {
        seed_tip(&s, n, "finish");
    }
    assert_eq!(top(&s), None);
    seed_tip(&s, 4, "finish");
    assert_eq!(top(&s), Some(TopTip { habit: "finish".into(), tips: 5, of: 5 }));
}

#[test]
fn the_top_tip_is_about_your_latest_twenty() {
    let s = Store::open_in_memory().unwrap();
    for n in 0..30 {
        seed_tip(&s, n, "target");
    }
    for n in 30..42 {
        seed_tip(&s, n, "evidence");
    }
    assert_eq!(top(&s), Some(TopTip { habit: "evidence".into(), tips: 12, of: 20 }));
}

#[test]
fn a_tied_top_tip_is_the_habit_listed_first() {
    let s = Store::open_in_memory().unwrap();
    for (n, name) in ["context", "target", "context", "target", "finish"].into_iter().enumerate() {
        seed_tip(&s, n as u32, name);
    }
    assert_eq!(top(&s), Some(TopTip { habit: "target".into(), tips: 2, of: 5 }));
}

#[test]
fn tips_are_score_tables() {
    assert!(SCORE_TABLES.contains(&"tips") && SCORE_TABLES.contains(&"tip_chats"));
}

/// A chat, `first` to `last`, and whether it had the rules.
fn seed_chat(s: &Store, id: &str, source: &str, first: &str, last: &str, told: bool) {
    s.conn
        .execute(
            "INSERT INTO sessions (id, source, first_day, last_day, messages, corrections) VALUES (?1,?2,?3,?4,1,0)",
            params![id, source, first, last],
        )
        .unwrap();
    if told {
        s.conn
            .execute("INSERT INTO tip_chats (session, source, day) VALUES (?1,?2,?3)", params![id, source, first])
            .unwrap();
    }
}

#[test]
fn active_counts_chats_with_the_rules_used_this_week() {
    let s = Store::open_in_memory().unwrap();
    seed_chat(&s, "old-and-still-going", "claude-code", "2026-09-01", "2026-10-12", true);
    seed_chat(&s, "this-week", "claude-code", "2026-10-13", "2026-10-14", true);
    seed_chat(&s, "last-week", "claude-code", "2026-10-05", "2026-10-11", true);
    seed_chat(&s, "no-rules", "claude-code", "2026-10-13", "2026-10-13", false);
    let a = s.tip_activity(d("2026-10-14"), None).unwrap();
    assert_eq!(a, TipActivity { active: 2, tips: 0, blocked: false });
}

#[test]
fn tips_look_blocked_only_when_enough_new_chats_have_no_rules() {
    let s = Store::open_in_memory().unwrap();
    let blocked = |s: &Store| s.tip_activity(d("2026-10-14"), Some(d("2026-10-10"))).unwrap().blocked;
    seed_chat(&s, "same-day", "claude-code", "2026-10-10", "2026-10-14", false);
    seed_chat(&s, "codex", "codex", "2026-10-11", "2026-10-11", false);
    seed_chat(&s, "a", "claude-code", "2026-10-11", "2026-10-11", false);
    seed_chat(&s, "b", "claude-code", "2026-10-12", "2026-10-12", false);
    assert!(!blocked(&s), "two could be scripts, and the others don't count");
    seed_chat(&s, "c", "claude-code", "2026-10-13", "2026-10-13", false);
    assert!(blocked(&s));
    assert!(!s.tip_activity(d("2026-10-14"), None).unwrap().blocked, "tips are off");
    seed_chat(&s, "told", "claude-code", "2026-10-14", "2026-10-14", true);
    assert!(!blocked(&s), "one chat with the rules is proof they arrive");
}

#[test]
fn tips_that_stop_arriving_look_blocked_after_a_few_more_chats() {
    let s = Store::open_in_memory().unwrap();
    let blocked = |s: &Store| s.tip_activity(d("2026-10-20"), Some(d("2026-10-10"))).unwrap().blocked;
    seed_chat(&s, "told", "claude-code", "2026-10-12", "2026-10-12", true);
    for n in 0..4 {
        seed_chat(&s, &format!("n{n}"), "claude-code", "2026-10-13", "2026-10-13", false);
    }
    assert!(!blocked(&s), "four could be scripts");
    seed_chat(&s, "n4", "claude-code", "2026-10-14", "2026-10-14", false);
    assert!(blocked(&s));
}

#[test]
fn stopping_a_source_leaves_a_marker_and_allowing_it_removes_it() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("noactually.db");
    let mut s = Store::open(&db).unwrap();
    let stopped = || crate::source_stopped(&db, "claude-code");
    s.allow("claude-code", None).unwrap();
    assert!(!stopped());
    s.set_paused("claude-code", true).unwrap();
    assert!(stopped(), "paused");
    s.set_paused("claude-code", false).unwrap();
    assert!(!stopped(), "resumed");
    s.revoke("claude-code").unwrap();
    assert!(stopped(), "revoked");
    s.set_paused("claude-code", false).unwrap();
    assert!(stopped(), "resuming doesn't undo a revoke");
    s.allow("claude-code", None).unwrap();
    assert!(!stopped(), "allowed again");
    s.forget("claude-code").unwrap();
    assert!(stopped(), "forgotten");
    assert!(!crate::source_stopped(&db, "codex"), "each source has its own");
}

#[test]
fn a_card_that_cant_be_counted_is_kept_as_a_number_until_the_history_is_deleted() {
    let s = Store::open_in_memory().unwrap();
    assert_eq!(s.report_cards_left_out("claude-code").unwrap(), 0);
    s.conn.execute("INSERT INTO settings (key, value) VALUES ('report_cards.left_out.claude-code', '2')", []).unwrap();
    assert_eq!(s.report_cards_left_out("claude-code").unwrap(), 2);
    s.delete_report_cards().unwrap();
    assert_eq!(s.report_cards_left_out("claude-code").unwrap(), 0);
}

#[test]
fn a_tip_where_the_ai_had_to_ask_is_kept_as_zero() {
    let mut lab = Lab::new();
    let asked = TIP.replace("redid 50%", "had to ask");
    lab.write("s1", &(rules("h", &t(0)) + &tipped("p", 5).replace(TIP, &asked)));
    lab.sync();
    let redid: u8 = lab.store.conn.query_row("SELECT redid FROM tips", [], |r| r.get(0)).unwrap();
    assert_eq!(redid, engine::HAD_TO_ASK);
    assert_eq!(tip_week(&lab.store).tips, 1);
}

#[test]
fn deleting_the_history_counts_tips_and_isnt_read_as_blocked_chats() {
    let s = Store::open_in_memory().unwrap();
    let today = chrono::Local::now().date_naive();
    let yesterday = today.pred_opt().unwrap();
    for id in ["a", "b", "c"] {
        seed_chat(&s, id, "claude-code", &yesterday.to_string(), &yesterday.to_string(), true);
    }
    seed_tip(&s, 0, "finish");
    assert_eq!(s.delete_report_cards().unwrap(), 1, "tips count");
    let on = yesterday.pred_opt().unwrap();
    assert!(!s.tip_activity(today, Some(on)).unwrap().blocked, "their rules were deleted, not missing");
}
