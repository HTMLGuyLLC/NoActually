# Architecture

Goals, in order: **private by default, reliable, fast, deterministic.**

```
crates/engine    Pure rules: classify a turn, normalize model ids, redact, build snippets. No I/O. Compiles to WASM.
crates/sources   One adapter per AI tool: detect, backfill, tail. Turns files into `Turn`s.
crates/store     SQLite: consent + settings, file checkpoints, daily aggregates, catch events, retention sweeps.
crates/cli       `noactually` binary: scan, stats, purge. Same code path as the app.
app/             Tauri 2: tray quick-glance, full window, notifications, update checks. Thin shell over the crates.
```

## Data flow

```
source files ──adapter──▶ Turn { human, prev_ai, next_ai, model, ts, session }
                              │
                       engine::classify  (fixed rules, no network, no LLM)
                              │
               ┌──────────────┼────────────────┐
        daily aggregates   session tally    catch event (text kept per retention level)
```

A **turn** is one human message plus the AI reply before it and the AI reply after it. The adapter only emits a turn once the next AI reply exists (or the next human message arrives), so live tailing and backfill produce identical results.

## Consent

Nothing is read until the user allows it.

- First launch lists every **detected** source with the exact path it would read and what it reads ("your messages and the AI's replies, to count catches"). Each is off until the user taps **Allow**.
- Per source: allow/revoke, backfill window (everything, since a date, or none), pause.
- **Revoke** stops reading. **Forget** also deletes that source's events, aggregates and checkpoints.
- Nothing leaves the machine. Share cards render locally. The only network use is the update check (see [Updates](#updates)) and opening a share URL the user taps. A future global board is a separate opt-in that sends numbers only.
- macOS may show its own "access data from other apps" prompt for some sources. Declining it only means that source can't be read.

## Updates

`app/src-tauri/src/updater.rs` uses `tauri-plugin-updater`. One thread fetches `https://github.com/HTMLGuyLLC/NoActually/releases/latest/download/latest.json` a minute after launch and every 6 hours, while **Check for updates automatically** is on (setting `updates.auto`, default on). Development builds never check on their own.

- The request is a plain GET with the plugin's user agent. No identifiers, stats or chat data are sent.
- `latest.json` (written by `scripts/finish-release.sh`) holds the version, release date, and per platform a download URL on the same release plus a minisign signature.
- A newer version shows a "Restart to update" banner in the scorecard window and a tray menu item. Nothing is downloaded until the user clicks one; then the plugin downloads the file, checks it against the public key in `tauri.conf.json` (`plugins.updater.pubkey`), installs it and restarts. It never restarts on its own.
- macOS installs the `.app.tar.gz` in place; Windows runs the NSIS (or MSI) installer in passive mode; Linux replaces the AppImage.

## What is stored

| Data | Stored | Text? | Lifetime |
|---|---|---|---|
| Ordinary messages (not catches) | Counts only, per day × source × model | Never | Forever (just numbers) |
| Sessions | Hashed id, source, day range, message and catch counts | Never | Forever |
| Catch / humbled events | Time, source, model, label, category, matched rule ids | Per retention level | Per TTL |
| File checkpoints | Path, byte offset, size, modified time | Never | Until the source is forgotten |
| Message fingerprints | Short hashes of message ids (dedupe) | Never | Until the source is forgotten |

### Retention levels for catch text

| Level | What's kept | Default TTL, then… |
|---|---|---|
| **Full** | Your whole message and the start of the AI's reply (secrets still redacted) | 30 days → Snippet |
| **Snippet** (default) | The matched sentence from each side, ≤140 chars, redacted (emails, URLs, keys, numbers, home paths) | 365 days → Category |
| **Category only** | No text at all | Forever |

Levels only ever step down (Full → Snippet → Category), on a sweep after every sync. The user picks the starting level and both TTLs. **Pinning** a catch to the Hall of Fame keeps its text until unpinned.

Snippets are cut deterministically from the matched sentence. Summaries would need a model, so they aren't in the scoring path. An optional local-model summary (Ollama / LM Studio) can come later as an explicit opt-in.

## Models

Each AI reply's raw model id is normalized to three levels:

| Raw | Provider | Family | Version |
|---|---|---|---|
| `claude-opus-5-5` | Anthropic | Claude Opus | 5.5 |
| `claude-haiku-4-5-20251001` | Anthropic | Claude Haiku | 4.5 |
| `gpt-5.1` | OpenAI | GPT | 5.1 |
| `gemini-2.5-pro` | Google | Gemini Pro | 2.5 |
| `llama3.1:8b` | Meta (local) | Llama | 3.1 |

Aggregates store all three, so every view can filter by all models, a provider, a family, or one exact version.

**Attribution.** A catch is charged to the model that wrote the reply you corrected (the *previous* AI reply). Humbled events go to the model that corrected you (the *next* reply).

## Sources

| Source | Status | How |
|---|---|---|
| Claude Code | ✅ live | Tail `~/.claude/projects/**/*.jsonl` by byte offset. Skips subagents, tool results, injected reminders. |
| Claude Cowork | ✅ live | Same format under `~/Library/Application Support/Claude/local-agent-mode-sessions` |
| Codex (CLI + desktop) | ✅ live | `~/.codex/sessions/**/rollout-*.jsonl`, plus older `.jsonl.zst`. Skips guardian/subagent threads and injected context. |
| ChatGPT | ✅ export | Import `conversations.json`, walk the visible branch, model from `model_slug` |
| Claude.ai | ✅ export | Import `conversations.json` (no model recorded, so it shows as "Claude") |
| ChatGPT desktop app | ✗ | Local cache is encrypted. Live support needs the browser extension. |
| Gemini CLI, Ollama app, Open WebUI, LM Studio, VS Code Copilot, Cursor, Aider, opencode | planned | Formats documented, but no real files on hand to verify. Each needs a fixture before it ships. |

Adding a source means implementing the `Source` trait in `crates/sources`: `roots()`, `files(root)`, `scan(path, from, finalize)`, `append_only(path)`, plus a fixture test.

## Reliability

- Checkpoints only advance past turns that are complete. A half-written line or an unanswered message is re-read on the next pass. A file quiet for 15 minutes is finalized, so a last unanswered message still counts.
- Every write for a file happens in one SQLite transaction with its checkpoint update.
- Every message has a stable key (the tool's message id, or a hash of session + time + text). Its hash is stored in `seen`, so resumed sessions, forks, re-imports and rewritten files never count twice.
- If a file shrinks or is rewritten, it's re-read from zero. Keys block the duplicates.
- The watcher is a trigger, not the source of truth. A 5-minute sync catches anything the OS missed, and launch catches up on anything written while the app was closed.
- The first sync only records a milestone baseline, so backfilling history never fires notifications.
- Claude Code deletes old transcripts (about 30 days by default). Aggregates live in our database, so history survives that.

## Speed

- Backfill runs files in parallel (rayon), pre-filters lines by substring before JSON parsing, and skips tool-result payloads without decoding them.
- Live updates read only appended bytes.
