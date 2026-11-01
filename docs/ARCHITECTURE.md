# Architecture

Goals, in order: **private by default, reliable, fast, deterministic.**

```
crates/engine    noactually-engine:  pure rules: classify a turn, grade a prompt, score and tier, normalize model ids, redact, build snippets. No I/O. Compiles to WASM.
crates/sources   noactually-sources: one adapter per AI tool: detect, backfill, tail. Turns files into `Turn`s.
crates/store     noactually-store:   SQLite: consent + settings, file checkpoints, daily aggregates, correction events, tasks (prompts), goals and goal weeks, trophies, lessons done, retention sweeps, rules versioning.
crates/cli       noactually-cli:     the `noactually` binary. Same code path as the app.
app/             noactually-app:     Tauri 2: tray quick-glance, full window, notifications, update checks. Thin shell over the crates.
```

The crate directories keep short import names (`engine`, `sources`, `store`); the Cargo package names are the `noactually-*` ones above.

### One source of truth

Rust owns every table the UI needs. The app asks for them once through the `meta` Tauri command (tiers and their floors, categories and titles, sources and display names), and `Stats` carries the computed `verdict` and a tier id. The TypeScript side formats and draws; it doesn't re-derive tiers, verdicts or labels. The CLI uses the same values.

## Data flow

```
source files ──adapter──▶ SourceTurn { source, session, ts, key, turn: Turn { human, prev_ai, next_ai, chat_model } }
                              │
                       engine::classify  (fixed rules, no network, no LLM)
                              │
               ┌──────────────┼────────────────┐
        daily aggregates   session tally    correction event (text kept per retention level)
```

A **turn** is one human message plus the AI reply before it and the AI's messages after it, up to the next human message. Those later messages count only when they say the person was right. The adapter only emits a turn once the next human message arrives (or the file goes quiet), so live tailing and backfill produce identical results.

## Consent

Nothing is read until the user allows it.

- First launch lists every **detected** source with the exact path it would read and what it reads ("your messages and the AI's replies, to count corrections"). Each is off until the user taps **Allow**.
- Per source: allow/revoke, backfill window (everything, since a date, or none), pause.
- **Revoke** stops reading and keeps scores. **Forget** also deletes that source's events, aggregates and checkpoints.
- Nothing leaves the machine. Share cards render locally. The app's only network use is the update check (see [Updates](#updates)) and opening a share URL the user taps. (With report cards on, Claude Code itself downloads the plugin from GitHub; see [Report cards](#report-cards).) A future global board is a separate opt-in that sends numbers only.
- macOS may show its own "access data from other apps" prompt for some sources. Declining it only means that source can't be read.

## Updates

`app/src-tauri/src/updater.rs` uses `tauri-plugin-updater`. One thread fetches `https://github.com/HTMLGuyLLC/NoActually/releases/latest/download/latest.json` a minute after launch and every 6 hours, while **Check for app updates automatically** (Settings → General) is on (setting `updates.auto`, default on). Development builds never check on their own.

- **Update channel** (Settings → General, setting `updates.channel`) picks the address. Stable reads the address above, which GitHub keeps on the newest release that isn't a prerelease. Beta reads `releases/download/beta-channel/latest.json`, on a release that holds only that file. `scripts/publish-release.sh` copies each release's `latest.json` there, so beta also gets full releases. The updater never offers an older version, so going back to Stable keeps the installed version until a newer full release comes out.
- The request is a plain GET with the plugin's user agent. No identifiers, stats or chat data are sent.
- `latest.json` (written by `scripts/finish-release.sh`) holds the version, release date, and per platform a download URL on the same release plus a minisign signature.
- A newer version shows a "Restart to update" banner in the scorecard window and a tray menu item. Nothing is downloaded until the user clicks one; then the plugin downloads the file, checks it against the public key in `tauri.conf.json` (`plugins.updater.pubkey`), installs it and restarts. It never restarts on its own.
- macOS installs the `.app.tar.gz` in place; Windows runs the NSIS (or MSI) installer in passive mode; Linux replaces the AppImage.

## Command line

The `noactually` binary shares the engine and database with the app:

| Command | What it does |
|---|---|
| `sources` | List sources, where they read from, and whether they're allowed |
| `allow <source> [--since YYYY-MM-DD \| --from-now]` | Allow a source, optionally limiting the backfill |
| `revoke <source>` | Stop reading, keep scores |
| `forget <source>` | Stop reading and delete everything from that source |
| `import <source> <path>` | Add an export file or folder (`chatgpt-export`, `claude-ai-export`) |
| `sync` | Read anything new from allowed sources |
| `stats [--source S] [--provider P] [--family F] [--version V] [--since YYYY-MM-DD] [--json]` | Print the scorecard, optionally from a start date instead of the last 28 days |
| `stats --report-cards [--source S] [--provider P] [--family F] [--version V] [--since YYYY-MM-DD] [--json]` | Report cards instead of the scorecard, with the same filters (all history unless `--since`): graded tasks and their grades, % A or B, redone work per graded task, extra cost (priced tasks only, with the date the prices were last checked), extra minutes, the habit left out most, and the last 5 cards. `--json` prints one object with a fixed set of keys; a number that can't be worked out is `null` |
| `check "<prompt>"` | Grade a draft prompt with the same rules as your history. Nothing is saved |
| `retention <full\|snippet\|category> [--full-days N] [--snippet-days N]` | Set how correction text is kept |

`digest --session <id>` (or `--transcript <path>`) and `hook <session-start\|stop\|codex-prompt\|codex-stop>` are hidden helper commands, left out of `--help`: the `grade` plugin and the Codex hooks run them to give the model a chat summary and to say a word after a reply. They print nothing and succeed on any error, and the desktop app runs them from its own executable.

**Database location.** One function, `store::default_db_path()`, picks the file for both the CLI and the app: `app.noactually/noactually.db` in your OS app-data folder. Set `NOACTUALLY_DB=/path/to/file.db` to use another database (development, demos, tests). `WA_DB` is still read as a deprecated fallback.

## What is stored

| Data | Stored | Text? | Lifetime |
|---|---|---|---|
| Ordinary messages (not corrections) | Counts only, per day × source × model | Never | Forever (just numbers) |
| Sessions | Hashed id, source, day range, message and correction counts | Never | Forever |
| Tasks (prompts) | Day, source, hashed chat id, model, a 5-bit mask of the signals the prompt carried, message and follow-up counts | Never | Forever |
| Your goal | What it asks for (a signal, a kind of follow-up or a tier), its target percent and start day. Stored as a setting | Never | Until you drop it |
| Goal weeks, trophies | A goal's finished weeks (count, percent, met/missed/paused), frozen; finished goals. Not score tables: rebuilds and Forget leave them | Never | Forever |
| Lessons done | The ids of lessons marked done | Never | Forever |
| Settings and milestones | Retention, update and menu bar choices, and the tiers already announced | Never | Forever |
| Correction / humbled events | Time, source, model, label, category, matched rule ids | Per retention level | Per TTL |
| Report cards | Day, start and end time, source, hashed chat id, model, letter grade, follow-up counts, which habits were missing (bitmask), tokens, minutes. Not a score table: rebuilds keep it | Never | Until the source is forgotten or the user clears report card history |
| Tips, chats that got the rules | `tips`: day, time, source, hashed chat id, model, habit (bitmask), `redid` (25 to 100, or 0 for "had to ask"). `tip_chats`: hashed chat id, source, day. Score tables, so a rebuild keeps them in step with tasks | Never | Until the source is forgotten or the history is deleted |
| File checkpoints | Path, byte offset, size, modified time | Never | Until the source is forgotten |
| Message fingerprints | Short hashes of message ids (dedupe) | Never | Until the source is forgotten |

### Report cards

Optional. The score never reads them. Plan: [`docs/plans/prompt-feedback.md`](plans/prompt-feedback.md).

1. The user types `/grade:me` (Codex: `$grade-me`). The user's own AI writes the card and a small `noactually` record block into the chat.
2. The transcript holds both. The adapter looks for the fence in AI messages only. The whole reply that holds a block that reads as a card is hidden from the engine (a quoted example that doesn't parse is scored as before), so a card can't trip a correction rule, and the `/grade:me` prompt and any "Stop hook feedback" message never count as the person's messages.
3. When a block appears, the whole chat is re-read once. That checks the block's seal (a per-chat HMAC keyed by `report-cards.key`), checks every id against the chat's prompts, and measures rounds, tokens and minutes from the transcript.
4. The numbers go into the `report_cards` table. Block text is never kept.
5. A rules rebuild keeps the table (transcripts expire, so these rows are the history). Writes are idempotent. Forget deletes the source's rows, and a Settings button clears all of them.

**Switching it on and off** (`app/src-tauri/src/report_cards*.rs`, Settings → Report cards). Turning on needs the tool's own source allowed first (Cowork doesn't stand in for Claude Code); the command checks it as well as the window. Only the main window may change anything.

- *Claude Code:* the app runs Claude Code's own `claude plugin` commands against the marketplace pinned to the app's tag (`HTMLGuyLLC/NoActually#v<version>`). Afterwards, and at every launch of a release build for someone who has turned report cards on (a `noactually` marketplace in Claude Code's `known_marketplaces.json`, a file the app only reads, or an unfinished move; anyone else is not asked anything), it finds the installed plugin (the folder named by `installPath` in `claude plugin list --json`, and no other: Claude Code's cache layout is its own, and a guess could check a different copy than the one that runs) and holds it to an allowlist. The folder may contain only `.claude-plugin/` (`plugin.json`, `icon.png`), `bin/na`, `hooks/hooks.json`, `skills/me/SKILL.md` and Claude Code's own `.orphaned_at` and `.links_materialized` marker files and `.in_use` folder (not looked inside), and each file must match the copy built into the app byte for byte (line endings aside; the icon exactly). Anything else, a link included, uninstalls the plugin. A plugin whose list entry names no folder can't be checked; it is kept, and the log says so. The launch check moves the pin to a new release only for a marketplace added from exactly that repo at a `v` tag, never for another `noactually` marketplace (the README's unpinned one, a local folder). Removing a marketplace uninstalls its plugin, so a move that fails puts the old tag back and installs the plugin again (switching it off again if it was off), and writes `report-cards-repin` beside the database. That file keeps the next launch retrying even when the plugin is gone for now, keeps Settings saying the update failed, and is cleared when a move finishes or the person turns report cards on or off.
- *Windows:* Turn on for Claude Code is refused unless Git for Windows' `bash.exe` is found (`CLAUDE_CODE_GIT_BASH_PATH`, then the `bash.exe` beside the `git.exe` on `PATH`, then the usual Program Files and per-user folders), because Claude Code runs the plugin's shell shim with it. Turn on for Codex is refused: its skill and hooks aren't set up for Windows yet. The row says why instead of showing Turn on.
- *Codex:* a skill in `~/.agents/skills/grade-me/` (first line after the front matter says it is ours; a `grade-me` skill without that line is refused and never removed) and two entries in `~/.codex/hooks.json`. Every change to an existing hooks file first saves a `hooks.json.bak-YYYYMMDD-HHMMSS` copy. Turning off removes only our folder, never `~/.agents` or `~/.agents/skills`. At every launch of a release build, if the skill carries our mark, the skill and shim are rewritten when they differ from what this version would write, and the `command` of our hook entries when it differs (nothing else about them, and a hook the person removed is not put back); a hooks file that changes is copied first, and a skill without our mark is never touched.
- *Files beside the database:* `report-cards-tips` (give tips as you work, Claude Code; its date says when it went on), `report-cards-tips-off` (tips were turned off, so chats that come back are told to stop) and `report-cards-quiet` (no one-line reminders from the hooks) and `source-stopped-<id>` (a paused, revoked or forgotten source: the hooks and digest stay out of its chats), and `report-cards-problem` (a plugin problem Settings keeps showing across restarts). Before a plugin move, the app runs `git ls-remote` on the new tag and keeps the current plugin if it can't be reached. Extra files in an installed plugin disable it at launch, and any other difference uninstalls it. Their presence is the setting; they are created without touching anything already there. `report-cards-claude` holds the absolute path where `claude` was last found by Turn on or Look again (owner-only). All other writes go through a randomly named temporary file opened with `create_new`, then a rename.
- *A custom database location:* the plugin's shim looks for `cli-path` in the default app data folder only, and the app writes `cli-path` (and the files above) beside the database it is using. With `NOACTUALLY_DB` pointing somewhere else the shim finds no helper and the hooks quietly do nothing, so report cards only work with the default database location.
- *Finding the tools:* `claude` is looked for first at the path saved in `report-cards-claude`, if it still exists and can be run, on every launch. Otherwise `claude` and `codex` are looked for once per run (the usual folders, which include version managers' folders such as nvm, Volta, fnm, asdf and Bun, and on macOS the Claude desktop app's own copy, newest version first; then `PATH`) and the path is remembered. A login shell is asked only when the person presses Turn on, Turn off or Look again, never on a plain status check or at launch.
- *Logs:* output from `claude` goes to the log cut to about 1 KB, with `user:password@` taken out of any link.

The command line has `stats --report-cards` for these numbers, and the helper commands the plugin runs (`digest`, `hook`); see [Command line](#command-line).

### Retention levels for correction text

| Level | What's kept | Default TTL, then… |
|---|---|---|
| **Full** | Your whole message and the start of the AI's reply (secrets still redacted) | 30 days → Snippet |
| **Snippet** (default) | The matched sentence from each side, ≤140 chars, redacted (emails, URLs, keys, numbers, home paths) | 365 days → Category |
| **Category only** | No text at all | Forever |

Levels only ever step down (Full → Snippet → Category), on a sweep after every sync. The user picks the starting level and both TTLs. **Pinning** a correction to the Hall of Fame keeps its text until unpinned.

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

**Attribution.** A correction is charged to the model that wrote the reply you corrected (the *previous* AI reply). Humbled events go to the model that corrected you (the *next* reply).

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
- The watcher is a trigger, not the source of truth. A 5-minute sync picks up anything the OS missed, and launch catches up on anything written while the app was closed.
- The first sync only records a milestone baseline, so backfilling history never fires notifications.
- Claude Code deletes old transcripts (about 30 days by default). Aggregates live in our database, so history survives that, rules rebuilds included: a rebuild sets scores aside in `kept_*` tables, reads what it can, and puts back each chat it couldn't read. Scores counted by newer rules than the running build are left alone.

## Testing

- **Rules and adapters** (`crates/*`): unit and fixture tests. Rule changes need a test for the new case and a near-miss that must not match. Adapter tests use small, made-up files in the tool's real format; no real chats are ever committed.
- **Store**: tests run against temporary SQLite files.
- **Frontend** (`app/`): `vitest` for the logic, `tsc --noEmit` for types, Biome for lint and formatting.
- **Scripts**: `node --test scripts/*.test.mjs` for the release helpers, ShellCheck for the shell scripts.
- Coverage numbers come from `cargo llvm-cov` (Rust) and `vitest --coverage` (frontend) and appear in CI run summaries.

## Continuous integration

`.github/workflows/ci.yml` runs on every pull request and push to `main`:

| Job | Checks |
|---|---|
| `rust` | Ubuntu, macOS and Windows: build the frontend, clippy with `-D warnings`, tests for the whole workspace including the app crate. `cargo fmt --check` runs once, on Ubuntu. |
| `frontend` | Type check, Biome, vitest with coverage, build, `pnpm audit --prod` |
| `scripts` | Script tests, ShellCheck, actionlint |
| `deny` | `cargo deny check` (licenses, advisories, sources) using `deny.toml` |
| `msrv` | `cargo check` of the whole workspace, app included, on the minimum supported Rust version |
| `wasm` | `noactually-engine` builds for `wasm32-unknown-unknown` |
| `coverage` | `cargo llvm-cov` summary in the job summary |
| `version` | Cargo workspace, `app/package.json` and `tauri.conf.json` versions agree |
| `ci-ok` | The only required check: fails if any job above failed or was cancelled |

Run the same checks locally with `just ci` (see [CONTRIBUTING.md](../CONTRIBUTING.md)). `.github/workflows/release.yml` builds draft releases from `v*` tags; the macOS build is signed and notarized locally (see Releasing in CONTRIBUTING.md).

## Speed

- Backfill runs files in parallel (rayon), pre-filters lines by substring before JSON parsing, and skips tool-result payloads without decoding them.
- Live updates read only appended bytes.
