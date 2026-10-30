//! Finding the message a person just sent without reading the whole transcript. The Stop hook
//! runs after every reply and almost always has nothing to say, so it looks at the end of the
//! file first and goes no further than it must.

use super::chat::{in_tail, Found, Look};
use serde_json::Value;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// The bytes of `path` from `offset` to its end.
fn read_from(path: &Path, offset: u64) -> Option<Vec<u8>> {
    let mut file = File::open(path).ok()?;
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// The last real message the person typed in a Claude Code transcript. With `prompt_id`, the
/// last one that carries it: the Stop hook is told which prompt it just answered, and a command
/// or a hook's own feedback is not one of those.
///
/// The adapter decides what counts as the person's message (it skips tool results, injected
/// reminders, commands, subagents), so the hook can't drift from what the app counts.
///
/// With a prompt id the window is read once. The records that carry it say where the prompt is:
/// none in the window means it is further back; some, with an earlier record before the first (so
/// the prompt's own start is inside), and the adapter's turns in the window say whether one of them
/// is a person's message or not. Only then is the window parsed, and only that once.
pub(crate) fn last_claude_message(found: &Found, prompt_id: Option<&str>) -> Option<String> {
    in_tail(&found.path, |offset| {
        let ids = match prompt_id {
            None => None,
            Some(want) => {
                let Some(bytes) = read_from(&found.path, offset) else { return Look::Never };
                match records_of(&bytes, want) {
                    // The prompt's records start the window: its own may be further back.
                    (_, false) if offset > 0 => return Look::Further,
                    (uuids, _) if uuids.is_empty() => return Look::Further,
                    (uuids, _) => Some(uuids),
                }
            }
        };
        let Ok(scan) = found.source.scan(&found.path, offset, true) else { return Look::Never };
        let turn = scan.turns.iter().rev().find(|t| match &ids {
            None => true,
            Some(uuids) => t.key.as_deref().is_some_and(|key| uuids.iter().any(|u| u == key)),
        });
        match (turn, prompt_id) {
            (Some(t), _) => Look::Found(t.turn.human.clone()),
            (None, Some(_)) => Look::Never,
            (None, None) => Look::Further,
        }
    })
}

/// The `uuid` of every record in `bytes` whose `promptId` is `want`, in file order, and whether
/// the first of them has a record before it in `bytes` (so the prompt's own start is in there).
fn records_of(bytes: &[u8], want: &str) -> (Vec<String>, bool) {
    let finder = memchr::memmem::Finder::new(want.as_bytes());
    let mut uuids = Vec::new();
    let mut after_another = false;
    for (n, line) in bytes.split(|b| *b == b'\n').filter(|l| !l.is_empty()).enumerate() {
        if finder.find(line).is_none() || memchr::memmem::find(line, b"\"promptId\"").is_none() {
            continue;
        }
        let Ok(v) = serde_json::from_slice::<Value>(line) else { continue };
        if v["promptId"] != want {
            continue;
        }
        if uuids.is_empty() {
            after_another = n > 0;
        }
        if let Some(uuid) = v["uuid"].as_str() {
            uuids.push(uuid.to_string());
        }
    }
    (uuids, after_another)
}

/// The last real message the person typed in a Codex rollout. The Codex adapter reads the end of the
/// file (and the rollout's first line, which says whose thread it is) and decides what counts as the
/// person's message, with its own filters: attachment lists, images, text Codex injects as the user,
/// and report card requests. So this can't drift from what the app counts.
pub(crate) fn last_codex_message(found: &Found) -> Option<String> {
    if !found.source.append_only(&found.path) {
        return None; // a finished, compressed session
    }
    in_tail(&found.path, |offset| {
        let Some(Ok(view)) = found.source.view_from(&found.path, offset, false) else { return Look::Never };
        view.prompts.last().map_or(Look::Further, |p| Look::Found(p.text.clone()))
    })
}
