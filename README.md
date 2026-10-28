<p align="center">
  <img src="app/src-tauri/icons/128x128@2x.png" width="96" height="96" alt="No, Actually app icon">
</p>

<h1 align="center">No, Actually</h1>

<p align="center">
  <b>Could AI replace you? No, actually.</b><br>
  A private, local-first scorecard that counts the times you caught your AI being wrong.
</p>

<p align="center">
  <a href="https://github.com/HTMLGuyLLC/NoActually/actions/workflows/ci.yml"><img src="https://github.com/HTMLGuyLLC/NoActually/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-e0322b" alt="MIT license"></a>
  <img src="https://img.shields.io/badge/platforms-macOS%20%7C%20Windows%20%7C%20Linux-3b5bdb" alt="macOS, Windows, Linux">
  <img src="https://img.shields.io/badge/built%20with-Tauri%202%20%2B%20Rust-141a2e" alt="Built with Tauri 2 and Rust">
  <img src="https://img.shields.io/badge/telemetry-none-2f9e44" alt="No telemetry">
</p>

<p align="center">
  <img src="docs/screenshot-scorecard.png" alt="The No, Actually scorecard: a verdict, your catch count versus the AI's, a tier ladder and weekly chart" width="820">
</p>

---

You correct your AI all day: the function that doesn't exist, the instruction it ignored, the bug that's "fixed" but still broken. **No, Actually** reads your chat history on your own computer, finds those moments with fixed rules, and turns them into a score you can be proud of.

## Features

- **A verdict, every week.** "Could Claude replace you? No. You caught it 17 times." Tiers run from Rubber stamp up to Irreplaceable.
- **Every model, side by side.** See catches for all AI, one provider, one family, or one exact version (say, Claude Opus 5.5), with a searchable picker.
- **Lives in your menu bar.** A quick glance on click, a full scorecard window, and a popup when you hit your 10th catch, a new tier, or a streak.
- **A Hall of Fame.** Your best catches, kept as redacted snippets. Pin the ones you want to keep forever.
- **Made to share.** One dialog for X, LinkedIn, Facebook, Threads, Bluesky, Reddit, WhatsApp and Instagram, plus share card images. You always see exactly what will be posted.
- **History and live tracking.** It backfills what's already on disk (about 1,700 files in 6 seconds), then follows new chats as they happen.

<p align="center">
  <img src="docs/screenshot-glance.png" alt="The menu bar quick glance" width="300">
</p>

## Private by design

| | |
|---|---|
| **Asks first** | Nothing is read until you click **Allow** on a source. Pause, stop, or **Forget** (delete everything from it) any time. |
| **Stays on your machine** | No accounts, no servers, no telemetry. The only network use is opening a share link you click. Fonts are bundled. |
| **Stores numbers, not chats** | Ordinary messages are only counted. For catches you choose **full text**, **snippet**, or **category only**, and text steps down on a timer you set. |
| **Redacts** | Keys, emails, links, paths and long numbers are removed from anything kept. |

The details are in [PRIVACY.md](PRIVACY.md).

## How a catch is counted

No AI judges your chats. The same history always gives the same score, and every rule is readable in [`crates/engine/src/rules.rs`](crates/engine/src/rules.rs).

| Label | Rule |
|---|---|
| **Confirmed** | You push back **and** the AI admits the mistake |
| **Admitted** | The AI's next reply opens with "you're right", "good catch", "my mistake"… |
| **Claimed** | Your message opens with "no, that…", "still not working", "you missed"… (not when you're answering its question) |
| **Humbled** | The AI corrects *you* ("Actually…", "Not quite") |

Adding context ("also", "fyi", "I forgot to mention") never counts against the AI. Each catch is charged to the model that wrote the reply you corrected.

## Supported tools

| Tool | How |
|---|---|
| Claude Code | Live |
| Claude Cowork | Live |
| Codex (CLI and desktop) | Live |
| ChatGPT | Import your data export |
| Claude.ai | Import your data export |

Coming: Gemini CLI, Ollama, Open WebUI, LM Studio, VS Code Copilot, Cursor and Aider, plus a browser extension for live ChatGPT, Claude.ai and Gemini. Want one sooner? [Request it](https://github.com/HTMLGuyLLC/NoActually/issues/new/choose), or add it yourself: an adapter is about 100 lines plus a test (see [CONTRIBUTING.md](CONTRIBUTING.md#add-a-source-adapter)).

## Install

Signed downloads will be on the [Releases](https://github.com/HTMLGuyLLC/NoActually/releases) page. Until then, build it yourself (needs Rust stable, Node 20+ and pnpm):

```bash
git clone https://github.com/HTMLGuyLLC/NoActually.git
cd NoActually/app && pnpm install && pnpm tauri build
```

The app lands in `target/release/bundle/`. To run it in development instead, use `pnpm tauri dev`.

### Command line

The same engine and database also work from a terminal:

```bash
cargo run --release -p noactually-cli -- sources              # what it can read, and what you've allowed
cargo run --release -p noactually-cli -- allow claude-code
cargo run --release -p noactually-cli -- sync
cargo run --release -p noactually-cli -- stats --family "Claude Opus"
```

## How it's built

```
crates/engine    the rules, model names and redaction: pure Rust, no I/O, WASM-ready
crates/sources   one adapter per AI tool (Claude Code, Codex, exports…)
crates/store     SQLite: consent, checkpoints, daily counts, retention, rules versioning
crates/cli       the `noactually` command
app/             Tauri 2 desktop app: menu bar, scorecard window, notifications
```

Highlights:
- **Crash-safe checkpoints.** A turn is only counted once its reply exists, and every file's writes commit in a single transaction.
- **Hashed message keys.** Resumed sessions, forks and re-imports are never counted twice.
- **Rules versioning.** When the rules change, scores are rebuilt from history.

The full design is in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Contributing

Issues and PRs are welcome. Good places to start:
- Support for another AI tool
- A rule that miscounted one of your chats (use the **Wrong catch** form, and redact the text first)
- Polish on the app

Read [CONTRIBUTING.md](CONTRIBUTING.md), then run the same checks as CI:

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
pnpm --dir app check && pnpm --dir app test
```

You can try the UI in a browser on sample data with `cd app && pnpm dev`, then open http://localhost:1420.

## License

[MIT](LICENSE) © HTMLGuy LLC
