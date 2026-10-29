//! The Learn tab's lessons (`app/src/learn/lessons.json`) must agree with the engine: every
//! example grades the way the lesson says, every quiz has exactly one right answer, and the
//! copy stays free of things that go stale (product names, versions, links, magic phrases).

use engine::{check, normalize_model, Signal};
use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};

const LESSONS: &str = include_str!("../../../app/src/learn/lessons.json");

const STEERS: [&str; 3] = ["added", "clarified", "redo"];

const BANNED: [&str; 18] = [
    "claude",
    "gpt",
    "chatgpt",
    "codex",
    "gemini",
    "copilot",
    "cursor",
    "anthropic",
    "openai",
    "google",
    "llama",
    "step by step",
    "you are an",
    "token",
    "hallucinat",
    "http",
    "www.",
    ".md",
];

fn lessons_json() -> Value {
    serde_json::from_str(LESSONS).expect("lessons.json parses")
}

fn signal_ids() -> Vec<&'static str> {
    Signal::ALL.iter().map(|s| s.as_str()).collect()
}

/// The ids of the signals the engine finds in `text`.
fn found(text: &str) -> BTreeSet<String> {
    let c = check(text);
    Signal::ALL.into_iter().filter(|s| c.signals.has(*s)).map(|s| s.as_str().to_string()).collect()
}

fn ids(v: &Value) -> BTreeSet<String> {
    v.as_array().expect("signals is an array").iter().map(|s| s.as_str().expect("signal id").to_string()).collect()
}

fn str_of<'a>(v: &'a Value, key: &str, at: &str) -> &'a str {
    v[key].as_str().unwrap_or_else(|| panic!("{at}: `{key}` is not a string"))
}

/// Every lesson, with the id of its track.
fn all_lessons(root: &Value) -> Vec<(&str, &Value)> {
    root["tracks"]
        .as_array()
        .expect("tracks")
        .iter()
        .flat_map(|t| {
            let track = t["id"].as_str().expect("track id");
            t["lessons"].as_array().expect("lessons").iter().map(move |l| (track, l))
        })
        .collect()
}

#[test]
fn examples_grade_the_way_the_lessons_say() {
    let root = lessons_json();
    let mut wrong = Vec::new();
    for (track, lesson) in all_lessons(&root) {
        let id = str_of(lesson, "id", track);
        let mut grades = Vec::new();
        for side in ["before", "after"] {
            let text = str_of(&lesson[side], "text", id);
            let want = ids(&lesson[side]["signals"]);
            let got = found(text);
            if got != want {
                wrong.push(format!("{id}.{side}: {text:?}\n   want {want:?}\n   got  {got:?}"));
            }
            let c = check(text);
            if c.not_a_prompt {
                wrong.push(format!("{id}.{side} reads as not a prompt: {text:?}"));
            }
            grades.push(c.grade);
        }
        if grades[1] <= grades[0] {
            wrong.push(format!("{id}: after ({:?}) must grade above before ({:?})", grades[1], grades[0]));
        }
        if let Some(signal) = lesson["teaches"]["signal"].as_str() {
            if !found(str_of(&lesson["after"], "text", id)).contains(signal) {
                wrong.push(format!("{id}.after is missing the signal it teaches ({signal})"));
            }
            if found(str_of(&lesson["before"], "text", id)).contains(signal) {
                wrong.push(format!("{id}.before already has the signal it teaches ({signal})"));
            }
        }
    }
    assert!(wrong.is_empty(), "{} lesson examples off:\n{}", wrong.len(), wrong.join("\n"));
}

#[test]
fn every_quiz_has_exactly_one_right_answer() {
    let root = lessons_json();
    let signals = signal_ids();
    let mut wrong = Vec::new();
    for (track, lesson) in all_lessons(&root) {
        let id = str_of(lesson, "id", track);
        let quiz = lesson["quiz"].as_array().unwrap_or_else(|| panic!("{id}: quiz is not an array"));
        if !(1..=2).contains(&quiz.len()) {
            wrong.push(format!("{id}: wants 1 or 2 quiz questions, has {}", quiz.len()));
        }
        for (n, q) in quiz.iter().enumerate() {
            let at = format!("{id}.quiz[{n}]");
            let options: Vec<&str> = q["options"]
                .as_array()
                .unwrap_or_else(|| panic!("{at}: options is not an array"))
                .iter()
                .map(|o| o.as_str().unwrap_or_else(|| panic!("{at}: an option is not a string")))
                .collect();
            for key in ["question", "explain"] {
                str_of(q, key, &at);
            }
            match str_of(q, "kind", &at) {
                "sharper" => {
                    let answer = q["answer"].as_u64().map(|a| a as usize);
                    let Some(answer) = answer.filter(|a| *a < 2) else {
                        wrong.push(format!("{at}: answer must be 0 or 1"));
                        continue;
                    };
                    if options.len() != 2 {
                        wrong.push(format!("{at}: a sharper quiz has 2 options, has {}", options.len()));
                        continue;
                    }
                    let (right, other) = (found(options[answer]), found(options[1 - answer]));
                    if right.len() <= other.len() {
                        wrong.push(format!(
                            "{at}: the answer {:?} has {right:?}, not more than the other {:?} with {other:?}",
                            options[answer],
                            options[1 - answer]
                        ));
                    }
                    for o in &options {
                        if check(o).not_a_prompt {
                            wrong.push(format!("{at}: option reads as not a prompt: {o:?}"));
                        }
                    }
                }
                "missing" => {
                    let prompt = str_of(q, "prompt", &at);
                    let answer = str_of(q, "answer", &at);
                    let has = found(prompt);
                    if !(3..=4).contains(&options.len()) {
                        wrong.push(format!("{at}: a missing quiz lists 3 or 4 signals, has {}", options.len()));
                    }
                    if let Some(o) = options.iter().find(|o| !signals.contains(o)) {
                        wrong.push(format!("{at}: {o:?} is not a signal"));
                    }
                    if !options.contains(&answer) {
                        wrong.push(format!("{at}: answer {answer:?} is not one of the options"));
                    }
                    if has.contains(answer) {
                        wrong.push(format!("{at}: the prompt already has {answer:?}: {prompt:?} ({has:?})"));
                    }
                    for o in options.iter().filter(|o| **o != answer) {
                        if !has.contains(*o) {
                            wrong.push(format!("{at}: {o:?} is missing too, so two answers fit: {prompt:?} ({has:?})"));
                        }
                    }
                    if check(prompt).not_a_prompt {
                        wrong.push(format!("{at}: the prompt reads as not a prompt: {prompt:?}"));
                    }
                }
                "pick" => {
                    if track != "correction" {
                        wrong.push(format!("{at}: pick quizzes are only for the correction track"));
                    }
                    if !(2..=4).contains(&options.len()) {
                        wrong.push(format!("{at}: a pick quiz has 2 to 4 options, has {}", options.len()));
                    }
                    if q["answer"].as_u64().is_none_or(|a| (a as usize) >= options.len()) {
                        wrong.push(format!("{at}: answer is out of range"));
                    }
                }
                other => wrong.push(format!("{at}: unknown quiz kind {other:?}")),
            }
        }
    }
    assert!(wrong.is_empty(), "{} quiz questions off:\n{}", wrong.len(), wrong.join("\n"));
}

#[test]
fn the_file_is_well_formed() {
    let root = lessons_json();
    let signals = signal_ids();
    let reviewed = Regex::new(r"^\d{4}-(?:0[1-9]|1[0-2])$").unwrap();
    let mut wrong = Vec::new();

    let hints = root["signal_hints"].as_object().expect("signal_hints");
    for s in &signals {
        if hints.get(*s).and_then(Value::as_str).is_none_or(str::is_empty) {
            wrong.push(format!("signal_hints is missing {s}"));
        }
    }
    if let Some(k) = hints.keys().find(|k| !signals.contains(&k.as_str())) {
        wrong.push(format!("signal_hints has an unknown key {k:?}"));
    }

    let mut seen = HashSet::new();
    for t in root["tracks"].as_array().expect("tracks") {
        let track = str_of(t, "id", "track");
        for key in ["title", "blurb"] {
            str_of(t, key, track);
        }
        if !seen.insert(track.to_string()) {
            wrong.push(format!("duplicate id {track:?}"));
        }
    }
    for (track, lesson) in all_lessons(&root) {
        let id = str_of(lesson, "id", track);
        if !seen.insert(id.to_string()) {
            wrong.push(format!("duplicate id {id:?}"));
        }
        for key in ["title", "why", "template"] {
            str_of(lesson, key, id);
        }
        if !reviewed.is_match(str_of(lesson, "reviewed", id)) {
            wrong.push(format!("{id}: reviewed must be YYYY-MM"));
        }
        let tips = lesson["tips"].as_array().map_or(0, Vec::len);
        if !(2..=3).contains(&tips) {
            wrong.push(format!("{id}: wants 2 or 3 tips, has {tips}"));
        }
        for side in ["before", "after"] {
            for s in ids(&lesson[side]["signals"]) {
                if !signals.contains(&s.as_str()) {
                    wrong.push(format!("{id}.{side}: {s:?} is not a signal"));
                }
            }
        }
        let teaches = lesson["teaches"].as_object().expect("teaches");
        let ok = teaches.len() == 1
            && match (track, teaches.iter().next()) {
                ("habits", Some((k, v))) => k == "signal" && v.as_str().is_some_and(|s| signals.contains(&s)),
                ("steer", Some((k, v))) => k == "steer" && v.as_str().is_some_and(|s| STEERS.contains(&s)),
                ("correction", Some((k, v))) => k == "correction" && v.as_bool() == Some(true),
                _ => false,
            };
        if !ok {
            wrong.push(format!("{id}: teaches {teaches:?} doesn't fit the {track} track"));
        }
    }
    // Each habit is taught once.
    let taught: BTreeSet<&str> =
        all_lessons(&root).iter().filter_map(|(_, l)| l["teaches"]["signal"].as_str()).collect();
    if taught.len() != signals.len() {
        wrong.push(format!("the habits track teaches {taught:?}, not all five signals"));
    }
    assert!(wrong.is_empty(), "{} problems:\n{}", wrong.len(), wrong.join("\n"));
}

/// Every string in `v`, with where it sits.
fn strings<'a>(v: &'a Value, at: &str, out: &mut Vec<(String, &'a str)>) {
    match v {
        Value::String(s) => out.push((at.to_string(), s)),
        Value::Array(a) => a.iter().enumerate().for_each(|(i, x)| strings(x, &format!("{at}[{i}]"), out)),
        Value::Object(o) => {
            // Name a lesson by its id, not its index, so failures say where to look.
            let here = o.get("id").and_then(Value::as_str).map_or_else(|| at.to_string(), str::to_string);
            for (k, x) in o {
                strings(x, &format!("{here}.{k}"), out);
            }
        }
        _ => {}
    }
}

#[test]
fn copy_does_not_go_stale() {
    let root = lessons_json();
    let placeholder = Regex::new(r"<[^<>\n]*>").unwrap();
    let slash = Regex::new(r"(?:^|\s)/[a-z]").unwrap();
    let version = Regex::new(r"\bv?\d+\.\d+\b").unwrap();
    let mut all = Vec::new();
    strings(&root, "lessons", &mut all);
    let mut wrong = Vec::new();
    for (at, s) in all {
        let lower = s.to_lowercase();
        for b in BANNED {
            if lower.contains(b) {
                wrong.push(format!("{at}: says {b:?}: {s:?}"));
            }
        }
        let text = placeholder.replace_all(s, " ");
        if slash.is_match(&text) {
            wrong.push(format!("{at}: looks like a slash command: {s:?}"));
        }
        if let Some(m) = version.find(&text) {
            wrong.push(format!("{at}: looks like a version number ({}): {s:?}", m.as_str()));
        }
        let words: Vec<&str> = text
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
            .filter(|w| !w.is_empty())
            .collect();
        let pairs = words.windows(2).flat_map(|p| [format!("{}-{}", p[0], p[1]), format!("{} {}", p[0], p[1])]);
        for candidate in words.iter().map(|w| (*w).to_string()).chain(pairs) {
            if let Some(m) = normalize_model(&candidate).filter(|m| m.provider != "Other") {
                wrong.push(format!("{at}: {candidate:?} reads as a {} model: {s:?}", m.provider));
            }
        }
    }
    assert!(wrong.is_empty(), "{} stale-prone strings:\n{}", wrong.len(), wrong.join("\n"));
}
