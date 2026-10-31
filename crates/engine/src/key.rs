//! Stable ids for messages and chats. The store uses them to skip copies, and report cards use a
//! short form of the same id to point at a prompt, so both always name a message the same way.

use crate::head_chars;
use sha2::{Digest, Sha256};
use std::fmt::Write;

/// Hex characters in a short id, the form report cards and the digest use.
pub const SHORT_ID: usize = 8;

/// Hashes `parts` (each ended by a NUL, so `["ab", "c"]` and `["a", "bc"]` differ) to 24 hex chars.
pub fn key_hash(parts: &[&str]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p.as_bytes());
        h.update([0]);
    }
    hex(&h.finalize()[..12])
}

/// The id of one human message. The tool's own id when it has one, else the chat, the time and the
/// start of the text. Changing this counts history twice, so it needs a `RULES_VERSION` bump.
pub fn message_key(source: &str, session: &str, key: Option<&str>, ts: &str, human: &str) -> String {
    match key {
        Some(k) => key_hash(&[source, k]),
        None => key_hash(&[source, session, ts, head_chars(human, 200)]),
    }
}

/// The first [`SHORT_ID`] chars of a [`message_key`]: what a report card writes to name a prompt.
pub fn short_id(key: &str) -> &str {
    &key[..SHORT_ID.min(key.len())]
}

/// HMAC-SHA256 (RFC 2104) of `msg` under `secret`, as lowercase hex.
pub fn hmac_sha256(secret: &[u8], msg: &[u8]) -> String {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if secret.len() > BLOCK {
        k[..32].copy_from_slice(&Sha256::digest(secret));
    } else {
        k[..secret.len()].copy_from_slice(secret);
    }
    let pad = |byte: u8| k.map(|b| b ^ byte);
    let inner = Sha256::new().chain_update(pad(0x36)).chain_update(msg).finalize();
    hex(&Sha256::new().chain_update(pad(0x5c)).chain_update(inner).finalize())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_are_separated() {
        assert_ne!(key_hash(&["ab", "c"]), key_hash(&["a", "bc"]));
        assert_eq!(key_hash(&["a"]).len(), 24);
    }

    #[test]
    fn message_key_falls_back_to_time_and_text() {
        let with = message_key("claude-code", "s", Some("u1"), "t", "hi");
        assert_eq!(with, key_hash(&["claude-code", "u1"]));
        let without = message_key("codex", "s", None, "t", "hi");
        assert_eq!(without, key_hash(&["codex", "s", "t", "hi"]));
        assert_eq!(short_id(&without), &without[..8]);
    }

    #[test]
    fn hmac_matches_rfc_4231() {
        // Test case 2: key "Jefe", data "what do ya want for nothing?".
        assert_eq!(
            hmac_sha256(b"Jefe", b"what do ya want for nothing?"),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        // Test case 6: a key longer than the block is hashed first.
        assert_eq!(
            hmac_sha256(&[0xaa; 131], b"Test Using Larger Than Block-Size Key - Hash Key First"),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }
}
