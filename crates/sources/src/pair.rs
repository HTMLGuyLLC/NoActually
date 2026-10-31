//! Shared pairing: a stream of human / AI events becomes turns, plus a safe resume point.

use crate::{CardSeen, FileScan, SourceTurn, TipSeen};
use chrono::{DateTime, Utc};
use engine::{find_cards, has_card, head_chars, parse_card, parse_tip, strip_tip, tail_chars, AiMsg, Turn};

const KEEP_PREV: usize = 600; // chars from the end of the reply you answered
const KEEP_NEXT: usize = 2000; // chars from the start of the reply after you

/// One message in a chat, in order. `pos` is where it starts in the file (or its index, for
/// whole-file exports), so the pairing can say where to resume.
#[derive(Debug, Clone)]
pub enum Event {
    /// Something the person typed.
    Human {
        /// The message.
        text: String,
        /// When it was sent.
        ts: DateTime<Utc>,
        /// The tool's id for it, for dedupe (see [`SourceTurn::key`]).
        key: Option<String>,
        /// Where it starts.
        pos: u64,
    },
    /// An AI reply.
    Ai {
        /// The reply.
        text: String,
        /// The raw model id, if the tool recorded one.
        model: Option<String>,
        /// When it was written, if the tool recorded that.
        ts: Option<DateTime<Utc>>,
        /// The tool's id for it (see [`CardSeen::key`]).
        key: Option<String>,
        /// Where it starts.
        pos: u64,
    },
}

/// True when `text` holds a record block that reads as one (`engine::parse_card`). A reply that
/// only mentions the fence in prose, or quotes a block that isn't a record (a made-up example, a
/// block of another version), is no report card and is scored like any other, as before cards existed.
pub(crate) fn holds_card(text: &str) -> bool {
    has_card(text) && find_cards(text).into_iter().any(|body| parse_card(body).is_ok())
}

/// The tips in `events`. A line under a report card is part of the card, and a reply with no time
/// can't be put on a day.
fn read_tips(session: &str, events: &[Event]) -> Vec<TipSeen> {
    let tip_in = |e: &Event| {
        let Event::Ai { text, model, ts: Some(ts), key, pos } = e else { return None };
        let tip = parse_tip(text).filter(|_| !holds_card(text))?;
        Some(TipSeen { session: session.to_string(), key: key.clone(), ts: *ts, pos: *pos, model: model.clone(), tip })
    };
    events.iter().filter_map(tip_in).collect()
}

/// True when `text` is a tip and nothing else, written on its own after the real reply.
fn only_a_tip(text: &str) -> bool {
    parse_tip(text).is_some() && strip_tip(text).is_empty()
}

impl Event {
    fn pos(&self) -> u64 {
        match self {
            Event::Human { pos, .. } | Event::Ai { pos, .. } => *pos,
        }
    }
}

/// Pairs each human event with the AI text around it.
///
/// A human message is complete once anything follows it. If the last one isn't, the checkpoint
/// rewinds to the AI reply before it, so the next pass re-reads that reply as context and emits
/// the turn exactly once. A pass that ends on a reply also resumes at that reply, so the next
/// message still knows what it answered. `end` is the byte offset after the last whole line read.
pub fn pair(source: &'static str, session: &str, mut events: Vec<Event>, end: u64, finalize: bool) -> FileScan {
    let tips = read_tips(session, &events);
    // A tip on its own is no reply: what came before it is still what the person answered.
    events.retain(|e| !matches!(e, Event::Ai { text, .. } if only_a_tip(text)));
    let mut scan = FileScan { checkpoint: end, tips, ..FileScan::default() };
    // The chat's model near each message: the last real one before it, else the first after it.
    let real = |e: &Event| match e {
        Event::Ai { model: Some(m), .. } if engine::is_real_model(m) => Some(m.clone()),
        _ => None,
    };
    let first_after: Vec<Option<String>> = {
        let mut next = None;
        let mut v: Vec<Option<String>> = events
            .iter()
            .rev()
            .map(|e| {
                next = real(e).or(next.take());
                next.clone()
            })
            .collect();
        v.reverse();
        v
    };
    let mut last_real: Option<String> = None;
    let mut last_ai: Option<(usize, &Event)> = None;
    // Re-reading a reply emits nothing (only human messages become turns).
    if let Some(Event::Ai { pos, .. }) = events.last() {
        scan.checkpoint = *pos;
    }
    for (i, ev) in events.iter().enumerate() {
        match ev {
            Event::Ai { text, ts, key, pos, .. } => {
                // Report cards are read out here, and the reply is hidden from scoring below.
                // A re-read reply (after a checkpoint rewind) yields the same blocks again;
                // the store dedupes them.
                if has_card(text) {
                    scan.cards.extend(find_cards(text).into_iter().map(|body| CardSeen {
                        source,
                        session: session.to_string(),
                        key: key.clone(),
                        ts: *ts,
                        pos: *pos,
                        body: body.to_string(),
                    }));
                }
                last_ai = Some((i, ev));
                last_real = real(ev).or(last_real.take());
            }
            Event::Human { text, ts, key, pos } => {
                let next = events[i + 1..].first();
                if next.is_none() && !finalize {
                    scan.checkpoint = last_ai.map_or(*pos, |(_, e)| e.pos());
                    break;
                }
                // Keep only what the rules read: the end of the previous reply, the start of the next.
                let ai = |e: Option<&Event>, keep: fn(&str, usize) -> &str, n: usize| match e {
                    Some(Event::Ai { text, model, .. }) => {
                        let text = if holds_card(text) { "" } else { keep(strip_tip(text), n) };
                        Some(AiMsg { text: text.to_string(), model: model.clone() })
                    }
                    _ => None,
                };
                scan.turns.push(SourceTurn {
                    source,
                    session: session.to_string(),
                    ts: *ts,
                    key: key.clone(),
                    turn: Turn {
                        human: text.clone(),
                        prev_ai: ai(last_ai.map(|(_, e)| e), tail_chars, KEEP_PREV),
                        next_ai: ai(next, head_chars, KEEP_NEXT),
                        chat_model: last_real.clone().or_else(|| first_after[i].clone()),
                    },
                });
            }
        }
    }
    scan
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        /// One time for every reply here, so a re-read reply compares equal.
        static TS: DateTime<Utc> = Utc::now();
    }

    fn h(text: &str, pos: u64) -> Event {
        Event::Human { text: text.into(), ts: Utc::now(), key: Some(text.into()), pos }
    }
    fn a(text: &str, pos: u64) -> Event {
        Event::Ai { text: text.into(), model: Some("m".into()), ts: Some(TS.with(|t| *t)), key: None, pos }
    }

    const TIP: &str = "Tip from No, Actually...: say which page when you first ask. [target, redid 50%]";

    #[test]
    fn a_tip_where_the_ai_had_to_ask_is_read_out_too() {
        let reply = "Fixed it.\n\nTip from No, Actually...: say which page when you first ask. [target, had to ask]";
        let scan = pair("t", "s", vec![h("one", 0), a(reply, 10), h("two", 20), a("r2", 30)], 40, false);
        assert_eq!(scan.tips[0].tip.redid, engine::HAD_TO_ASK);
        assert_eq!(scan.turns[0].turn.next_ai.as_ref().map(|m| m.text.as_str()), Some("Fixed it."));
    }

    #[test]
    fn a_tip_is_read_out_and_never_reaches_the_rules() {
        let reply = format!("Fixed it, sorry about that.\n\n{TIP}");
        let with_tip = pair("t", "s", vec![h("one", 0), a(&reply, 10), h("two", 20), a("r2", 30)], 40, false);
        let without = pair(
            "t",
            "s",
            vec![h("one", 0), a("Fixed it, sorry about that.", 10), h("two", 20), a("r2", 30)],
            40,
            false,
        );
        assert_eq!(with_tip.tips.len(), 1);
        assert_eq!(with_tip.tips[0].tip.redid, 50);
        assert_eq!((with_tip.tips[0].pos, with_tip.tips[0].session.as_str()), (10, "s"));
        for (a, b) in with_tip.turns.iter().zip(&without.turns) {
            assert_eq!(a.turn.prev_ai.as_ref().map(|m| &m.text), b.turn.prev_ai.as_ref().map(|m| &m.text));
            assert_eq!(a.turn.next_ai.as_ref().map(|m| &m.text), b.turn.next_ai.as_ref().map(|m| &m.text));
        }
    }

    #[test]
    fn a_reply_that_is_only_a_tip_is_no_reply() {
        let scan = pair("t", "s", vec![h("one", 0), a("r1", 10), a(TIP, 20), h("two", 30), a("r2", 40)], 50, false);
        assert_eq!(scan.tips.len(), 1);
        assert_eq!(scan.turns[0].turn.next_ai.as_ref().unwrap().text, "r1");
        assert_eq!(scan.turns[1].turn.prev_ai.as_ref().unwrap().text, "r1", "the real reply, not an empty one");
    }

    #[test]
    fn a_tip_with_no_time_or_on_a_card_is_left_out() {
        let at = Some(Utc::now());
        let timed = |text: &str, ts| Event::Ai { text: text.into(), model: None, ts, key: None, pos: 10 };
        let block = r#"{"v":1,"k":"9c41e0aa","tasks":[{"at":"7f3a9c21","to":"e44a1b90"}]}"#;
        let card = format!("Report card: A\n\n```noactually\n{block}\n```\n\n{TIP}");
        assert_eq!(pair("t", "s", vec![timed(TIP, None)], 20, false).tips.len(), 0);
        assert_eq!(pair("t", "s", vec![timed(&card, at)], 20, false).tips.len(), 0);
        assert_eq!(pair("t", "s", vec![timed(TIP, at)], 20, false).tips.len(), 1);
    }

    #[test]
    fn a_re_read_reply_reports_its_tip_again() {
        let reply = format!("Done.\n\n{TIP}");
        let first = pair("t", "s", vec![h("one", 0), a(&reply, 10)], 20, false);
        assert_eq!(first.checkpoint, 10, "the reply is read again next time");
        let next = pair("t", "s", vec![a(&reply, 10), h("two", 20), a("r2", 30)], 40, false);
        assert_eq!(next.tips, first.tips, "the store keeps one");
        assert_eq!(next.turns[0].turn.prev_ai.as_ref().unwrap().text, "Done.");
    }

    #[test]
    fn pending_human_rewinds_to_previous_reply() {
        let scan = pair("t", "s", vec![h("one", 0), a("r1", 10), h("two", 20)], 30, false);
        assert_eq!(scan.turns.len(), 1);
        assert_eq!(scan.checkpoint, 10);
        // The next pass starts at the reply and now sees the answer: "two" is emitted once.
        let next = pair("t", "s", vec![a("r1", 10), h("two", 20), a("r2", 30)], 40, false);
        assert_eq!(next.turns.len(), 1);
        assert_eq!(next.turns[0].turn.human, "two");
        assert_eq!(next.turns[0].turn.prev_ai.as_ref().unwrap().text, "r1");
        assert_eq!(next.turns[0].turn.next_ai.as_ref().unwrap().text, "r2");
        assert_eq!(next.checkpoint, 30, "and resumes at r2 next time");
    }

    #[test]
    fn a_pass_ending_on_a_reply_resumes_at_it() {
        let scan = pair("t", "s", vec![h("one", 0), a("r1", 10)], 20, false);
        assert_eq!(scan.checkpoint, 10, "the reply is re-read next time");
        // So the next message, written after this pass, still has its context.
        let next = pair("t", "s", vec![a("r1", 10), h("still broken", 20), a("r2", 30)], 40, false);
        assert_eq!(next.turns.len(), 1);
        assert_eq!(next.turns[0].turn.prev_ai.as_ref().unwrap().text, "r1");
    }

    #[test]
    fn finalize_emits_unanswered_message() {
        let scan = pair("t", "s", vec![a("r1", 0), h("bye", 10)], 20, true);
        assert_eq!(scan.turns.len(), 1);
        assert!(scan.turns[0].turn.next_ai.is_none());
        assert_eq!(scan.checkpoint, 20);
    }

    #[test]
    fn unanswered_first_message_takes_the_chats_model() {
        let ev = vec![
            h("first, never answered", 0),
            Event::Ai { text: "placeholder".into(), model: Some("<synthetic>".into()), ts: None, key: None, pos: 5 },
            h("second", 10),
            a("real reply", 20),
        ];
        let scan = pair("t", "s", ev, 30, true);
        assert_eq!(scan.turns[0].turn.chat_model.as_deref(), Some("m"));
        assert_eq!(scan.turns[1].turn.chat_model.as_deref(), Some("m"));
    }

    #[test]
    fn a_reply_that_only_mentions_the_fence_is_scored_as_usual() {
        let mention = "Put it in a ```noactually block when you are done.";
        let ev = vec![a(mention, 0), h("no, wrong", 10), a(mention, 20)];
        let scan = pair("t", "s", ev, 30, true);
        assert_eq!(scan.cards.len(), 0);
        assert_eq!(scan.turns[0].turn.prev_ai.as_ref().unwrap().text, mention);
        assert_eq!(scan.turns[0].turn.next_ai.as_ref().unwrap().text, mention);
    }

    const VALID: &str = r#"{"v":1,"k":"9c41e0aa","tasks":[{"at":"7f3a9c21","to":"e44a1b90"}]}"#;

    #[test]
    fn only_a_reply_with_a_block_that_reads_as_a_card_is_hidden_from_scoring() {
        let block = |body: &str| format!("Here you go.\n\n```noactually\n{body}\n```\n");
        let quoted = [
            block("not json"),
            block(r#"{"v":2,"k":"9c41e0aa","tasks":[]}"#), // another version
            block(r#"{"v":1,"k":"nope","tasks":[{"at":"7f3a9c21","to":"e44a1b90"}]}"#), // a bad seal shape
            block(r#"{"v":1,"k":"9c41e0aa","tasks":[]}"#), // no tasks
        ];
        for text in &quoted {
            assert!(!holds_card(text), "{text}");
            let scan = pair("t", "s", vec![a(text, 0), h("no, wrong", 10), a("fine", 20)], 30, true);
            assert_eq!(scan.turns[0].turn.prev_ai.as_ref().unwrap().text, text.as_str(), "scored as usual: {text}");
            assert_eq!(scan.cards.len(), 1, "the block is still reported, to be left out by whoever stores cards");
        }
        let valid = block(VALID);
        assert!(holds_card(&valid));
        let scan = pair("t", "s", vec![a(&valid, 0), h("no, wrong", 10), a(&valid, 20)], 30, true);
        assert_eq!(scan.turns[0].turn.prev_ai.as_ref().unwrap().text, "", "a card is hidden from the rules");
        assert_eq!(scan.turns[0].turn.next_ai.as_ref().unwrap().text, "");
        assert_eq!(scan.cards.len(), 2);
        // One block that reads is enough, whatever else the reply quotes.
        let mixed = format!("{}{}", block("not json"), block(VALID));
        assert!(holds_card(&mixed));
    }

    #[test]
    fn back_to_back_humans_are_both_complete() {
        let scan = pair("t", "s", vec![h("one", 0), h("two", 5), a("r", 9)], 12, false);
        assert_eq!(scan.turns.len(), 2);
        assert!(scan.turns[0].turn.next_ai.is_none());
    }
}
