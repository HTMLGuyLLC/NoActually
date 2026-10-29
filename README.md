<p align="center">
  <img src="app/src-tauri/icons/128x128@2x.png" width="96" height="96" alt="No, Actually... app icon">
</p>

<h1 align="center">No, Actually...</h1>

<p align="center">
  <b>Could AI replace you? No, actually.</b><br>
  A private scorecard for how you work with AI: every time you caught it being wrong, and how well you brief it.
</p>

<p align="center">
  <a href="https://github.com/HTMLGuyLLC/NoActually/actions/workflows/ci.yml"><img src="https://github.com/HTMLGuyLLC/NoActually/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-e0322b" alt="MIT license"></a>
  <img src="https://img.shields.io/badge/platforms-macOS%20%7C%20Windows%20%7C%20Linux-3b5bdb" alt="macOS, Windows, Linux">
  <img src="https://img.shields.io/badge/built%20with-Tauri%202%20%2B%20Rust-141a2e" alt="Built with Tauri 2 and Rust">
  <img src="https://img.shields.io/badge/telemetry-none-2f9e44" alt="No telemetry">
</p>

<p align="center">
  <a href="https://github.com/HTMLGuyLLC/NoActually/releases/latest"><img src="https://img.shields.io/badge/%E2%AC%87%EF%B8%8E_Download-Mac_%C2%B7_Windows_%C2%B7_Linux-e0322b?style=for-the-badge" alt="Download for Mac, Windows or Linux"></a><br>
  <sub>Free and open source · Your chats never leave your computer · <a href="#get-started-in-a-minute">Get started</a></sub>
</p>

<p align="center">
  <img src="docs/screenshot-scorecard.png" alt="The No, Actually... scorecard overview: the catch verdict and the brief verdict side by side, each with its tier, above the headline stats" width="820">
</p>

---

You correct your AI all day: the function that doesn't exist, the instruction it ignored, the bug that's "fixed" but still broken. And sometimes it goes sideways because the ask was vague.

**No, Actually...** reads the chat history already on your computer and answers two questions every week:

| | The question | What it counts | Tiers |
|---|---|---|---|
| 🧠 | **Could AI replace you?** | Times you caught it being wrong | Rubber stamp → Coworker → Sharp-eyed → **Irreplaceable** |
| 🎯 | **Do you make it guess?** | How often your tasks landed without you adding, clarifying or starting over | Vibes only → Ticket writer → Tech lead → **Architect** |

No AI does the judging. Fixed, readable rules do, so the same history always gives the same score. Nothing is uploaded, ever.

## Get started in a minute

1. **[Download it](https://github.com/HTMLGuyLLC/NoActually/releases/latest)** for Mac, Windows or Linux ([which file?](#download-and-install)).
2. Open it, click **Choose sources**, and **Allow** the AI tools you want counted. Nothing is read until you do.
3. That's it. It backfills your history, then keeps score as you work, from your menu bar (Mac) or system tray (Windows and Linux).

<p align="center">
  <img src="docs/screenshot-glance.png" alt="The menu bar quick glance: this week's verdict, your catches versus the AI's, your streak and your brief tier" width="300">
</p>

## What you get

- **A weekly verdict.** "Could Claude replace you? **No.** You caught it 17 times in the last 7 days."
- **A brief score.** "Do you make it guess? **Sometimes.** 85% of your tasks landed." Plus what your briefs carry, where the steering went, and one tip backed by your own history.
- **Every model, side by side.** All AI, one provider, one family, or one exact version (say, Claude Opus 5.5).
- **A Hall of Fame** of your best catches, as redacted snippets. Pin the ones you want to keep forever.
- **Tier-up popups** for your 10th catch, a new tier, or a streak.
- **Made to brag.** Share cards for X, LinkedIn, Bluesky, Threads, Reddit, Facebook, WhatsApp and Instagram. You always see exactly what will be posted before anything goes anywhere.

## Private and secure by design

Your AI chats can hold code, keys, client names and half-formed ideas. This app is built so none of that leaves your computer.

| | |
|---|---|
| 🔒 **Local only** | No accounts, no servers, no telemetry, no analytics. Fonts are bundled. The only network calls are an update check against GitHub (sends nothing about you, and can be turned off) and a share link you click yourself. |
| ✋ **Asks first** | Every source is off until you **Allow** it. Pause it, stop it, or **Forget** it (deletes everything it read) any time. |
| 🔢 **Numbers, not chats** | Ordinary messages are only counted, never stored. For catches you pick **category only** (the default), **snippet** or **full text**, and kept text steps down on a timer you set. Briefs store counts only, never words. |
| ✂️ **Redacted** | Keys, emails, links, file paths and long numbers are stripped from anything kept. |
| ✅ **Verifiable** | MIT open source. The Mac app is signed and notarized by Apple, updates are signed, and every release ships `SHA256SUMS.txt`. |

The full details are in [PRIVACY.md](PRIVACY.md).

## Questions people ask

<details>
<summary><b>Does it send my chats anywhere?</b></summary>

No. Everything is read and scored on your computer, and the results live in a local SQLite file. There is no server to send them to.
</details>

<details>
<summary><b>Does it use AI to judge my chats?</b></summary>

No. Using an AI to grade AI would mean uploading your chats and getting a different answer each run. Scoring is plain pattern rules you can read in [`rules.rs`](crates/engine/src/rules.rs) and [`brief.rs`](crates/engine/src/brief.rs).
</details>

<details>
<summary><b>Which AI tools does it work with?</b></summary>

Claude Code, Claude Cowork and Codex live, plus ChatGPT and Claude.ai through their data exports. More are coming; see [Supported tools](#supported-tools).
</details>

<details>
<summary><b>Will it slow my computer down?</b></summary>

The engine is native Rust, and the window uses your system's built-in web view instead of shipping its own browser. The first backfill reads files in parallel. After that it only reads what's new since its last checkpoint.
</details>

<details>
<summary><b>Is a low brief score bad?</b></summary>

It's a nudge, not a grade on you. "Landed" means nothing *on you* followed the task: you didn't add to it, clarify it or start it over. The AI getting it wrong never counts against you; that's a catch for you instead. One-shot tasks don't count at all, since they can't need steering.
</details>

<details>
<summary><b>It counted something wrong. Now what?</b></summary>

Rules miss sometimes. Open a [**Wrong catch**](https://github.com/HTMLGuyLLC/NoActually/issues/new/choose) issue with the message redacted. Fixes land with a test for your example, so they stay fixed. When rules change, your score is rebuilt from history automatically.
</details>

<details>
<summary><b>Can my employer see my score?</b></summary>

Only if you share it. There's no account, no dashboard and no sync. Your score lives on your machine.
</details>

<details>
<summary><b>How do I delete everything?</b></summary>

Click **Forget** on a source to delete everything read from it. Uninstalling the app and deleting its data folder removes the rest.
</details>

<details>
<summary><b>Is it really free?</b></summary>

Yes. MIT licensed, no paid tier, no ads.
</details>

---

## How the scoring works

### How a catch is counted

| Label | Rule |
|---|---|
| **Confirmed** | You push back **and** the AI admits the mistake |
| **Admitted** | The AI's next reply opens with "you're right", "good catch", "my mistake"… |
| **Claimed** | Your message opens with "no, that…", "still not working", "you missed"… (not when you're answering its question) |
| **Humbled** | The AI corrects *you* ("Actually…", "Not quite") |

Adding context ("also", "fyi", "I forgot to mention") never counts against the AI. Each catch is charged to the model that wrote the reply you corrected. The weekly tier comes from catches in the last 7 days: Rubber stamp 0–2, Coworker 3–6, Sharp-eyed 7–14, Irreplaceable 15+.

### How a brief is graded

- **A task** starts with a chat's first message, or a later one that opens with "now…", "next:" or "new task". Hellos, "continue" and questions ("what does this do?") aren't tasks.
- **A brief's grade** counts what it carries, never its length:
  - **points at it**: a file, function, link or `code`
  - **shows it**: an error, output, an example, anything pasted
  - **says what done is**: the expected result, tests, a format
  - **sets limits**: don't, only, keep, without
  - **says why**: the goal or context

  Three or more is **sharp**, two is **solid**, fewer is **thin**. Pasted blocks count as showing it, but their words aren't graded as yours.
- **Steering** is a later message that redirects the task. Either the AI got it wrong (a catch), or it's on you: **added more** ("also…", "I forgot to mention"), **clarified** ("I meant…", "to be clear") or **started over** ("scrap that", "revert it").
- **A task landed** when nothing on you followed it. The tier is the share of tasks with a follow-up that landed over the last 28 days (at least 10 of them): Vibes only, Ticket writer 70%+, Tech lead 80%+, Architect 90%+.

The tip names the thing your briefs carry least. It only quotes "briefs that do landed X% vs Y%" when your own history shows a real gap.

### Where the thresholds come from

There's no public benchmark for weekly per-person scores, so every threshold was checked three ways:

- **The author's history:** 8,600 messages.
- **Public chats:** a random 3,400 English chats from [WildChat-1M](https://huggingface.co/datasets/allenai/WildChat-1M), run through these exact rules.
- **Published figures:** Claude Code users interrupt in 5–9% of turns; in multi-turn developer chats, 14–17% of prompts refine the request.

What that showed:

- The catch rate agrees across sources: 1 in 47 messages for the author, 1 in 41 in WildChat.
- Public chats are short and almost never steer, so the brief floors come from one heavy user, whose typical month landed 86% (Tech lead). Expect them to move as more people's numbers exist.
- Longer briefs didn't steer less. Bigger tasks get both longer briefs and more back-and-forth, which is why the tier scores outcomes, not length.

## Supported tools

| Tool | How |
|---|---|
| Claude Code | Live |
| Claude Cowork | Live |
| Codex (CLI and desktop) | Live |
| ChatGPT | Import your data export |
| Claude.ai | Import your data export |

Coming: Gemini CLI, Ollama, Open WebUI, LM Studio, VS Code Copilot, Cursor and Aider, plus a browser extension for live ChatGPT, Claude.ai and Gemini. Want one sooner? [Request it](https://github.com/HTMLGuyLLC/NoActually/issues/new/choose), or add it yourself: an adapter is about 100 lines plus a test (see [CONTRIBUTING.md](CONTRIBUTING.md#add-a-source-adapter)).

## Download and install

Open the **[latest release](https://github.com/HTMLGuyLLC/NoActually/releases/latest)** and scroll to **Assets** (click it to expand the list if needed). Download the one file for your computer:

| Your computer | Download | Install |
|---|---|---|
| **Mac** (macOS 12+, Apple silicon or Intel) | `No-Actually-<version>-Mac.dmg` | Open it, drag **No, Actually...** into **Applications**, and open it from there. It's signed and notarized, so there are no warnings. |
| **Windows** | `No-Actually-<version>-Windows-Setup.exe` | Run it. If you see "Windows protected your PC", click **More info → Run anyway** (the Windows build isn't code-signed yet). |
| **Linux** | `No-Actually-<version>-Linux.AppImage` | Right-click → Properties → allow it to run as a program (or `chmod +x`), then open it. |

The other files are for special cases:
- `-Windows-IT-Installer.msi` is for IT teams deploying to many PCs.
- `-Linux-Ubuntu-Debian.deb` and `-Linux-Fedora.rpm` install through your package manager.
- `SHA256SUMS.txt` lets you verify a download.

The app updates itself (signed updates) unless you turn that off in Settings.

## For developers

### Build from source

Needs Rust stable, Node 20+ and pnpm:

```bash
git clone https://github.com/HTMLGuyLLC/NoActually.git
cd NoActually/app && pnpm install && pnpm tauri build -c '{"bundle":{"createUpdaterArtifacts":false}}'
```

The app lands in `target/release/bundle/`. (The `-c` part skips the signed update files, which need the project's private key.) For development, use `pnpm tauri dev`. To try the UI in a browser on sample data, run `pnpm dev` and open http://localhost:1420.

### Command line

The same engine and database work from a terminal:

```bash
cargo run --release -p noactually-cli -- sources              # what it can read, and what you've allowed
cargo run --release -p noactually-cli -- allow claude-code
cargo run --release -p noactually-cli -- sync
cargo run --release -p noactually-cli -- stats --family "Claude Opus"
```

`stats` prints both scores. Set `NOACTUALLY_DB=/path/to/test.db` to use a different database than your real one. Every command is in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#command-line).

### How it's built

```
crates/engine    catch rules, brief grading, model names, redaction: pure Rust, no I/O, WASM-ready
crates/sources   one adapter per AI tool (Claude Code, Codex, exports…)
crates/store     SQLite: consent, checkpoints, daily counts, tasks, retention, rules versioning
crates/cli       the `noactually` command
app/             Tauri 2 desktop app: menu bar, scorecard window, notifications
```

- **Crash-safe checkpoints.** A turn is only counted once its reply exists, and each file's writes commit in a single transaction.
- **Hashed message keys.** Resumed sessions, forks and re-imports are never counted twice.
- **Rules versioning.** When the rules change, scores are rebuilt from history. Pinned catches are always kept; other history counts again only if its files still exist.

The full design is in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

### Contributing

Issues and PRs are welcome. Good places to start:
- Support for another AI tool
- A rule that miscounted one of your chats (use the **Wrong catch** form, and redact the text first)
- Polish on the app

Read [CONTRIBUTING.md](CONTRIBUTING.md), then run the same checks as CI with [just](https://github.com/casey/just) (`brew install just` or `cargo install just`):

```bash
just ci
```

Without `just`, run these from a fresh clone. The frontend must be built first because the app crate reads `app/dist` at compile time:

```bash
pnpm --dir app install --frozen-lockfile
pnpm --dir app build
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm --dir app check
pnpm --dir app lint
pnpm --dir app test
node --test scripts/*.test.mjs
```

## History

The first version was a Python prototype. The Rust engine replaced it, and the original is kept at the git tag `prototype-v0`. The roadmap and what scoring means are in [docs/PRODUCT.md](docs/PRODUCT.md).

## License

[MIT](LICENSE) © HTMLGuy LLC
