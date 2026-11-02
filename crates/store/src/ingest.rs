//! Counting what a file holds: one transaction per file, so a crash can never count a turn without
//! moving the file's checkpoint, or the reverse.

use crate::cards::{self, set_left_out};
use crate::rebuild::level_after_rebuild;
use crate::{card_secret, hash, tips, Level, Result, SourceState, Store, NO_MODEL};
use chrono::Local;
use engine::{classify, normalize_model, read_prompt, snippet, Label, Prompt};
use rusqlite::{params, Connection, OptionalExtension};
use sources::{ChatView, Kind, Source, SourceTurn};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A file's size and modified time, as last read. A change in either means it changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct FileMeta {
    pub size: u64,
    /// Nanoseconds since the Unix epoch, so two writes in the same second still differ. Older
    /// stores hold seconds here; those never match, so the first sync after upgrading re-checks
    /// each file once (append-only files resume at their checkpoint; whole files are re-read
    /// and the `seen` keys block double counts).
    pub mtime: i64,
}

impl FileMeta {
    /// The file's current size and mtime, plus the raw modified time.
    pub fn read(path: &Path) -> std::io::Result<(FileMeta, SystemTime)> {
        let meta = std::fs::metadata(path)?;
        let modified = meta.modified().unwrap_or(UNIX_EPOCH);
        let mtime = modified.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos() as i64);
        Ok((FileMeta { size: meta.len(), mtime }, modified))
    }
}

/// Every history file a source covers: its live roots, plus the files and folders the user
/// added (exports).
pub(crate) fn source_files(src: &dyn Source, extra: &[String]) -> Vec<PathBuf> {
    let mut roots = if src.kind() == Kind::Live { src.roots() } else { Vec::new() };
    roots.extend(extra.iter().map(PathBuf::from));
    roots.into_iter().flat_map(|root| if root.is_file() { vec![root] } else { src.files(&root) }).collect()
}

/// One read of a file: what it looked like, and what an earlier read already did with it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pass {
    pub meta: FileMeta,
    /// The pass that wrote this checkpoint already counted a trailing unanswered message, so an
    /// unchanged file needs no more reads even though its checkpoint is short of its end.
    pub finalized: bool,
    /// The last report card position in this file that was read and checked, or `None` if none was
    /// (also after a rewrite, which starts over). A card at or before it is already stored, so the
    /// whole chat isn't read again for it.
    pub cards_to: Option<u64>,
}

impl Pass {
    pub fn new(meta: FileMeta, finalized: bool) -> Self {
        Self { meta, finalized, cards_to: None }
    }
}

/// Where a file was read up to, and what it looked like then.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Checkpoint {
    /// Where the next pass resumes.
    pub at: u64,
    /// The file's size and mtime when it was read.
    pub meta: FileMeta,
    /// That pass was final (see [`Pass::finalized`]).
    pub finalized: bool,
    /// The last report card position read (see [`Pass::cards_to`]).
    pub cards_to: Option<u64>,
}

pub(crate) fn upsert_file(conn: &Connection, path: &str, source: &str, checkpoint: u64, pass: Pass) -> Result<()> {
    conn.execute(
        "INSERT INTO files (path, source, checkpoint, size, mtime, finalized, cards_to) VALUES (?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(path) DO UPDATE SET checkpoint = excluded.checkpoint, size = excluded.size,
           mtime = excluded.mtime, finalized = excluded.finalized, cards_to = excluded.cards_to",
        params![
            path,
            source,
            checkpoint as i64,
            pass.meta.size as i64,
            pass.meta.mtime,
            pass.finalized,
            pass.cards_to.map(|p| p as i64)
        ],
    )?;
    Ok(())
}

impl Store {
    /// Remembers that the file as it stands now can't be read, so it isn't tried on every pass.
    pub(crate) fn remember_unreadable(&self, path: &str, source: &str, meta: FileMeta) {
        let pass = Pass { meta, finalized: true, cards_to: None };
        if let Err(e) = upsert_file(&self.conn, path, source, 0, pass) {
            log::warn!("couldn't note an unreadable file: {e}");
        }
    }

    /// Where `path` was last read: see [`Checkpoint`].
    pub(crate) fn file_checkpoint(&self, path: &str) -> Result<Option<Checkpoint>> {
        Ok(self
            .conn
            .query_row("SELECT checkpoint, size, mtime, finalized, cards_to FROM files WHERE path = ?1", [path], |r| {
                Ok(Checkpoint {
                    at: r.get::<_, i64>(0)? as u64,
                    meta: FileMeta { size: r.get::<_, i64>(1)? as u64, mtime: r.get(2)? },
                    finalized: r.get(3)?,
                    cards_to: r.get::<_, Option<i64>>(4)?.map(|p| p as u64),
                })
            })
            .optional()?)
    }

    /// Writes one file's turns and its new checkpoint in a single transaction, so a crash
    /// can never count a turn without moving the checkpoint (or the reverse). Turns the
    /// source's consent skips are left out entirely, so allowing more history later still
    /// counts them. Returns (messages, corrections) counted.
    ///
    /// `chat` is the whole file read again; it's only needed (and only given) when the scan found
    /// report cards past `pass.cards_to`, which are checked against it and saved in the same
    /// transaction.
    pub(crate) fn ingest(
        &mut self,
        state: &SourceState,
        path: &str,
        scan: &sources::FileScan,
        chat: Option<&ChatView>,
        mut pass: Pass,
    ) -> Result<(usize, usize)> {
        let source = state.id.as_str();
        let level = self.retention()?.level;
        let graded = self.read_cards(state, scan, chat, pass.cards_to)?;
        let has_tips = !scan.tips.is_empty() || !scan.rules.is_empty();
        let cleared = if has_tips { self.get_setting(cards::CLEARED_KEY)? } else { None };
        let checkpoint = checkpoint_after(&mut pass, scan, &graded);
        let rebuilding = self.rebuild_pending()?;
        let tx = self.conn.transaction()?;
        let (mut messages, mut corrections) = (0, 0);
        for st in scan.turns.iter().filter(|st| state.wants(st.ts)) {
            if let Some(corrected) = count_turn(&tx, source, st, level, rebuilding)? {
                messages += 1;
                corrections += usize::from(corrected);
            }
        }
        cards::save(&tx, &graded.rows)?;
        tips::save(&tx, state, scan, cleared.as_deref())?;
        // Only a full check knows the file's total. Cards that couldn't be checked yet are tried
        // again, so they aren't counted as left out.
        if graded.checked {
            set_left_out(&tx, source, path, graded.left_out())?;
        }
        upsert_file(&tx, path, source, checkpoint, pass)?;
        tx.commit()?;
        graded.log(source, path);
        Ok((messages, corrections))
    }

    /// The report cards in `scan` that check out against `chat`. The secret that seals them is read
    /// from its file each time (this is rare): the helper may have made it after the app started,
    /// and a missing, unreadable or empty one means no card can be trusted, never a failed sync.
    fn read_cards(
        &self,
        state: &SourceState,
        scan: &sources::FileScan,
        chat: Option<&ChatView>,
        cards_to: Option<u64>,
    ) -> Result<cards::Cards> {
        let mut out = cards::Cards::default();
        if !cards::unread(&scan.cards, cards_to) {
            return Ok(out);
        }
        let secret = match self.conn.path().filter(|p| !p.is_empty()) {
            Some(db) => card_secret(Path::new(db), false).unwrap_or_else(|e| {
                log::warn!("couldn't read the report card key: {e}");
                None
            }),
            None => None, // an in-memory database has no folder to keep one in
        };
        match (chat, secret) {
            (Some(chat), Some(secret)) => {
                let cleared = self.get_setting(cards::CLEARED_KEY)?;
                out = cards::read(state, chat, &secret, cleared.as_deref());
            }
            (None, _) => {
                out.skip("chat unreadable", scan.cards.len());
                out.unchecked = true;
            }
            (_, None) => {
                out.skip("no secret", scan.cards.len());
                out.unchecked = true;
            }
        }
        Ok(out)
    }
}

/// Counts one message, unless it's a copy already counted. Returns whether it was a correction, or
/// `None` for a copy. `level` is how much text a new correction keeps.
fn count_turn(
    tx: &rusqlite::Transaction<'_>,
    source: &str,
    st: &SourceTurn,
    level: Level,
    rebuilding: bool,
) -> Result<Option<bool>> {
    let session = hash(&[source, &st.session]);
    if rebuilding {
        tx.execute("INSERT OR IGNORE INTO rebuild_read (id, source) VALUES (?1, ?2)", params![session, source])?;
    }
    let key = engine::message_key(source, &st.session, st.key.as_deref(), &st.ts.to_rfc3339(), &st.turn.human);
    let new = "INSERT OR IGNORE INTO seen (key, source, session) VALUES (?1, ?2, ?3)";
    if tx.execute(new, params![key, source, session])? == 0 {
        return Ok(None); // a copy we've already counted (resumed or forked session, re-import)
    }
    let v = classify(&st.turn);
    let model = v.blamed_model(&st.turn).and_then(normalize_model);
    let (provider, family, version) = model
        .as_ref()
        .map_or((NO_MODEL, NO_MODEL, ""), |m| (m.provider.as_str(), m.family.as_str(), m.version.as_str()));
    let day = st.ts.with_timezone(&Local).date_naive().to_string();
    let bump = |kind: &str, category: &str| {
        tx.execute(
            "INSERT INTO daily (day, source, provider, family, version, kind, category, n) VALUES (?1,?2,?3,?4,?5,?6,?7,1)
             ON CONFLICT DO UPDATE SET n = n + 1",
            params![day, source, provider, family, version, kind, category],
        )
    };
    bump("message", "")?;
    let is_correction = v.label.is_some_and(Label::is_correction);
    tx.execute(
        "INSERT INTO sessions (id, source, first_day, last_day, messages, corrections) VALUES (?1,?2,?3,?3,1,?4)
         ON CONFLICT(id) DO UPDATE SET messages = messages + 1, corrections = corrections + excluded.corrections,
           first_day = min(first_day, excluded.first_day), last_day = max(last_day, excluded.last_day)",
        params![session, source, day, i64::from(is_correction)],
    )?;
    count_prompt(tx, &st.turn, &v, &session, [day.as_str(), source, provider, family, version])?;
    let Some(label) = v.label else { return Ok(Some(is_correction)) };
    let category = v.category.map_or("", engine::Category::as_str);
    bump(label.as_str(), category)?;

    let level = if rebuilding { level_after_rebuild(tx, &key, level)? } else { level };
    let text = EventText::new(&st.turn, &v, level);
    // A pinned correction kept through a rules rebuild is already here; keep it as pinned.
    tx.execute(
        "INSERT INTO events (ts, day, source, session, provider, family, version, label, category,
            level, human_full, ai_full, human_snip, ai_snip, seen_key)
         SELECT ?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15
         WHERE NOT EXISTS (SELECT 1 FROM events WHERE seen_key = ?15)",
        params![
            st.ts.to_rfc3339(),
            day,
            source,
            session,
            provider,
            family,
            version,
            label.as_str(),
            category,
            level.as_str(),
            text.human_full,
            text.ai_full,
            text.human_snip,
            text.ai_snip,
            key,
        ],
    )?;
    Ok(Some(is_correction))
}

/// Where the next pass resumes, given how this one's report cards went. Remembers checked cards in
/// `pass`.
fn checkpoint_after(pass: &mut Pass, scan: &sources::FileScan, graded: &cards::Cards) -> u64 {
    if !graded.unchecked {
        pass.cards_to = pass.cards_to.max(scan.cards.iter().map(|c| c.pos).max());
        return scan.checkpoint;
    }
    // Cards that couldn't be checked (no secret yet) are not remembered as read, and the file
    // resumes at the first of them, not past it: a following whole turn would otherwise move
    // the checkpoint beyond the card and no later pass would see it. Turns read twice are `seen`.
    cards::first_unread(&scan.cards, pass.cards_to).map_or(scan.checkpoint, |first| scan.checkpoint.min(first))
}

/// The text kept for one event at a retention level. Always redacted. The next reply holds the
/// AI's admission (for a correction) or its pushback (humbled).
struct EventText {
    human_full: Option<String>,
    ai_full: Option<String>,
    human_snip: Option<String>,
    ai_snip: Option<String>,
}

/// Prompts: a new task gets a row; steering and other replies count toward the chat's latest.
/// `row` is the day, source, provider, family and version a new task is filed under.
fn count_prompt(
    tx: &rusqlite::Transaction<'_>,
    turn: &engine::Turn,
    v: &engine::Verdict,
    session: &str,
    row: [&str; 5],
) -> Result<()> {
    let latest = "id = (SELECT MAX(id) FROM tasks WHERE session = ?1)";
    let has_task: bool =
        tx.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE session = ?1)", [session], |r| r.get(0))?;
    match read_prompt(turn, v, has_task) {
        Prompt::Task { signals, .. } => {
            let [day, source, provider, family, version] = row;
            tx.execute(
                "INSERT INTO tasks (day, source, session, provider, family, version, signals) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![day, source, session, provider, family, version, signals.0],
            )?
        }
        Prompt::Steer { cause, .. } => tx.execute(
            &format!("UPDATE tasks SET messages = messages + 1, {c} = {c} + 1 WHERE {latest}", c = cause.as_str()),
            [session],
        )?,
        Prompt::Other => tx.execute(&format!("UPDATE tasks SET messages = messages + 1 WHERE {latest}"), [session])?,
    };
    Ok(())
}

impl EventText {
    fn new(turn: &engine::Turn, v: &engine::Verdict, level: Level) -> Self {
        let mut text = EventText { human_full: None, ai_full: None, human_snip: None, ai_snip: None };
        if level == Level::Category {
            return text;
        }
        // The span points into the message that held the admission.
        let ai = match v.ai_later {
            Some(i) => turn.later_ai.get(i).map(String::as_str),
            None => turn.next_ai.as_ref().map(|m| m.text.as_str()),
        };
        let ai_norm = engine::normalize_quotes(ai.unwrap_or(""));
        text.human_snip = Some(snippet(&engine::normalize_quotes(turn.human.trim()), v.human_span));
        text.ai_snip = (!ai_norm.is_empty()).then(|| snippet(&ai_norm, v.ai_span));
        if level == Level::Full {
            text.human_full = Some(engine::redact(turn.human.trim()));
            text.ai_full = ai.map(engine::redact);
        }
        text
    }
}
