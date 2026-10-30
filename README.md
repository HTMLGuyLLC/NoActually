<p align="center">
  <img src="app/src-tauri/icons/128x128@2x.png" width="96" height="96" alt="No, Actually... app icon">
</p>

<h1 align="center">No, Actually...</h1>

<p align="center">
  <b>A scoreboard for you vs. your AI.</b><br>
  Prompt it well, correct it when it's wrong, and find out if it could really replace you.<br>
  Tiers to climb, goals to hit, lessons to level up, and a card worth bragging about.
</p>

<p align="center">
  <a href="https://github.com/HTMLGuyLLC/NoActually/releases/latest"><img src="https://img.shields.io/badge/%E2%AC%87%EF%B8%8E_Download-Mac_%C2%B7_Windows_%C2%B7_Linux-e0322b?style=for-the-badge" alt="Download for Mac, Windows or Linux"></a>
</p>

<p align="center">
  🔒 Private: everything stays on your computer unless you share it<br>
  💸 Free: no AI calls, so nothing to pay for
</p>

<p align="center">
  <img src="docs/screenshot-scorecard.png" alt="The No, Actually... scorecard overview: the overall score and tier, with how you prompt it and who corrects whom below" width="820">
</p>

---

You correct your AI all day: the function that doesn't exist, the instruction it ignored, the bug that's "fixed" but still broken. And sometimes it goes sideways because the ask was vague.

**No, Actually...** reads the chat history already on your computer and answers one big question, built from two smaller ones:

| | The question | What it measures | Tiers |
|---|---|---|---|
| 🧠 | **Could AI replace you?** | Your overall score, 0 to 100, from both questions below | Rubber stamp → Coworker → Sharp-eyed → **Irreplaceable** |
| 🎯 | ↳ **Do you make it guess?** | How often your prompts are clear the first time: no adding, clarifying or starting over | Constantly → Often → Sometimes → **Rarely** |
| 🧐 | ↳ **Do you correct it?** | Who corrects whom: you correcting it, or it correcting you | Rarely → Sometimes → Often → **Religiously** |

Better prompts mean fewer mistakes to correct, and that never costs you: once you've corrected it a few times (proof you check its work), more corrections add nothing.

> **💸 Costs nothing to run.** No AI does the judging, so there's no API key, no tokens spent and no subscription. Fixed, readable rules score your chats right on your computer: the same history always gives the same score, and nothing is uploaded.

## Get started in a minute

1. **[Download it](https://github.com/HTMLGuyLLC/NoActually/releases/latest)** for Mac, Windows or Linux ([which file?](#download-and-install)).
2. Open it, click **Choose sources**, and **Allow** the AI tools you want counted. Nothing is read until you do.
3. That's it. It backfills your history, then keeps score as you work, from your menu bar (Mac) or system tray (Windows and Linux). Prefer an ordinary app that quits when you close it? Turn off **Keep running in the menu bar** in Settings → General.

<p align="center">
  <img src="docs/screenshot-glance.png" alt="The menu bar quick glance: the three questions, each answered in a word with why, and your goal" width="300">
</p>

## What you get

- **A verdict and a score.** "Could Claude replace you? **No.** Score 86, Sharp-eyed." Updated as you work, over the last 28 days.
- **A prompt score.** "Do you make it guess? **Sometimes.** 85% of your prompts were clear the first time." Plus what your prompts include, why you followed up, and one tip backed by your own history.
- **Improve.** One weekly goal picked from your own numbers ("Include guardrails in 25% of your prompts"), streaks, trophies, personal bests, last week's recap, and charts of how you're trending.
- **Learn.** 12 one-minute lessons in three tracks (the 5 habits, follow up less, correct more), quick quizzes, and a prompt checker that grades a draft as you type, with the same rules as your score.
- **Any time window.** 7 days, 28 days, 90 days or all time: the whole scorecard follows the picker, score included. Every part of the score is a rate, so tiers mean the same thing in any window.
- **Every model, side by side.** All AI, one provider, one family, or one exact version (say, Claude Opus 5.5). The picker shows the numbers that page is about.
- **A Hall of fame** of awards, one per kind of correction: The Fabulist (made it up), Selective Hearing, Reading Is Hard, The Blind Spot, “Fixed” It, Big Brain Energy, and Touché for the times it corrected you. Each shows its best snippet, picked the same way every time. Pin the ones you want to keep forever.
- **Popups** for your 1st, 10th, 25th… correction, a new tier, a streak, a goal week met, or a goal done.
- **Made to brag.** Share cards for X, LinkedIn, Bluesky, Threads, Reddit, Facebook, WhatsApp and Instagram. You always see exactly what will be posted before anything goes anywhere.

## Private and secure by design

Your AI chats can hold code, keys, client names and half-formed ideas. This app is built so none of that leaves your computer.

| | |
|---|---|
| 🔒 **Local only** | No accounts, no servers, no telemetry, no analytics. Fonts are bundled. The only network use is an update check against GitHub and a share link you click yourself. The update check carries no account, ID, stats or chat data (GitHub sees your IP address, as with any download), and you can turn it off. |
| ✋ **Asks first** | Every source is off until you **Allow** it. Pause it, stop it, or **Forget** it (deletes everything it read) any time. |
| 🔢 **Numbers, not chats** | Ordinary messages are only counted, never stored. For corrections you pick **full text**, **snippet** (the default) or **category only**, and kept text steps down on a timer you set. Prompts, goals and lessons store numbers only, never words, and the prompt checker saves nothing you type. |
| ✂️ **Redacted** | Keys, emails, links, home paths and long numbers are stripped from any text kept. It's pattern-based, so it can miss things: pick **category only** if that matters. |
| ✅ **Verifiable** | MIT open source. The Mac app is signed and notarized by Apple, updates are signed, and every release ships `SHA256SUMS.txt`. |

The full details are in [PRIVACY.md](PRIVACY.md).

## Questions people ask

<details>
<summary><b>Does it send my chats anywhere?</b></summary>

No. Everything is read and scored on your computer, and the results live in a local SQLite file. There is no server to send them to. The only time text leaves is if you share a Hall of Fame card, and you see the card before it goes anywhere.
</details>

<details>
<summary><b>Does it use AI to judge my chats?</b></summary>

No. Using an AI to grade AI would mean uploading your chats and getting a different answer each run. Scoring is plain pattern rules you can read in [`rules.rs`](crates/engine/src/rules.rs) and [`prompt.rs`](crates/engine/src/prompt.rs).
</details>

<details>
<summary><b>Which AI tools does it work with?</b></summary>

Claude Code, Claude Cowork and Codex live, plus ChatGPT and Claude.ai through their data exports. More are coming; see [Supported tools](#supported-tools).
</details>

<details>
<summary><b>Will the lessons go out of date?</b></summary>

They're written not to. They teach habits that work with any collaborator, human or AI (name a target, give evidence, set a finish line, add guardrails, share the purpose), never tool tricks or magic phrases. A test fails the build if a lesson names a product, a model or a version, and the real scoring rules grade every example, so a lesson can never disagree with your score.
</details>

<details>
<summary><b>Will it slow my computer down?</b></summary>

The engine is native Rust, and the window uses your system's built-in web view instead of shipping its own browser. The first backfill reads files in parallel. After that it only reads what's new since its last checkpoint.
</details>

<details>
<summary><b>If I prompt it better, it makes fewer mistakes. Doesn't that lower my score?</b></summary>

No. Corrections only have to show you check its work: 3 in 28 days is full credit, and more add nothing. After that, what counts is how rarely the AI has to correct *you*, per task. Most of the score (70%) is how often your prompts are clear the first time, so prompting better is the fastest way up.
</details>

<details>
<summary><b>Is a low prompt score bad?</b></summary>

It's a nudge, not a grade on you. A prompt is "clear the first time" when nothing *on you* followed it: you didn't add to it, clarify it or start it over. The AI getting it wrong never counts against you; that's a correction for you instead. One-shot prompts don't count at all, since they can't need a follow-up.
</details>

<details>
<summary><b>It counted something wrong. Now what?</b></summary>

Rules miss sometimes. Open a [**Wrong correction**](https://github.com/HTMLGuyLLC/NoActually/issues/new/choose) issue with the message redacted. Fixes usually come with a test for your example, so they stay fixed. When rules change, your score is rebuilt from history automatically.
</details>

<details>
<summary><b>Can my employer see my score?</b></summary>

Not through the app: there's no account, no dashboard and no sync. Your score lives in a file on your machine (not encrypted, so anyone with access to your computer could read it, like any other file).

It's also a for-fun self-check, not a measure of job performance, and it shouldn't be used to evaluate anyone.
</details>

<details>
<summary><b>How do I delete everything?</b></summary>

Click **Forget** on a source to delete everything read from it. To remove it all, quit the app and delete its data folder (the paths are in [PRIVACY.md](PRIVACY.md#how-to-delete-your-data)). Uninstalling alone leaves the data behind.
</details>

<details>
<summary><b>Is it really free? What does it cost to run?</b></summary>

Free, and it costs nothing to run. It never calls an AI, so there's no API key to add and no tokens on your bill, however much history you have. MIT licensed, no paid tier, no ads.
</details>

---

## How the scoring works

### Could AI replace you? (the overall score)

Over the window you pick (the last 28 days unless you choose another), once it has at least 10 tasks with a follow-up:

```
score = 70% × direction + 30% × judgment

direction = % of your prompts that were clear the first time (see "How a prompt is graded")
judgment  = 100, minus 5 for each 1% of your tasks where the AI had to correct you
            (counted against at least 30 tasks, so a quiet month isn't wiped out),
            scaled down until you've corrected it 3 times
```

| Tier | Score | Answer |
|---|---|---|
| Irreplaceable | 90+ | No. |
| Sharp-eyed | 80–89 | No. |
| Coworker | 65–79 | No. |
| Rubber stamp | under 65 | Hmm. |

Each question below the score gets a one-word answer too, and that word is its tier. Each tier also has a short name, shown in the quick glance and on share cards.

Do you correct it? goes by judgment:

| Answer | Judgment | Name |
|---|---|---|
| Religiously. | 90+ | Hawk-eyed |
| Often. | 75–89 | Reviewer |
| Sometimes. | 50–74 | Spot-checker |
| Rarely. | under 50 | Autopilot |

Do you make it guess? goes by the share of prompts that were clear the first time (see "How a prompt is graded"):

| Answer | Clear the first time | Name |
|---|---|---|
| Rarely. | 90%+ | Architect |
| Sometimes. | 80–89% | Tech lead |
| Often. | 70–79% | Ticket writer |
| Constantly. | under 70% | Vibes only |

A few things this gets right on purpose:
- **Prompting better never costs you.** Past 3 corrections, more corrections add nothing, so an AI that makes fewer mistakes because you prompted it well doesn't lower your score.
- **No corrections at all caps you.** Getting everything clear the first time but never correcting anything is a Coworker (70) at best: there's no sign you check its work.
- **Being corrected counts, per task.** If the AI keeps correcting *you*, judgment drops. It's measured per task, never against your corrections, so it doesn't depend on how often the AI is wrong.

### How a correction is counted

| Label | Rule |
|---|---|
| **Confirmed** | You push back **and** the AI admits the mistake |
| **Admitted** | The AI's next reply opens with "you're right", "good catch", "my mistake"… |
| **Claimed** | Your message opens with "no, that…", "still not working", "you missed"… (not when you're answering its question) |
| **Humbled** | The AI corrects *you* ("Actually…", "Not quite") |

Adding context ("also", "fyi", "I forgot to mention") never counts against the AI. Each correction is charged to the model that wrote the reply you corrected.

### How a prompt is graded

- **A task** starts with a chat's first message, or a later one that opens with "now…", "next:" or "new task". Hellos, "continue" and questions ("what does this do?") aren't tasks.
- **A prompt's grade** counts what it includes, never its length:
  - **Target**: a file, function, page or link
  - **Evidence**: an error, output, an example, anything pasted
  - **Finish line**: the expected result, tests, a format
  - **Guardrails**: don't, only, keep, without
  - **Purpose**: who it's for or what it's part of

  Three or more is **specific**, two is **decent**, fewer is **vague**. Pasted blocks count as evidence, but their words aren't graded as yours.
- **A follow-up** is a later message that redirects the task. Either the AI got it wrong (a correction), or it's on you: **added more** ("also…", "I forgot to mention"), **clarified** ("I meant…", "to be clear") or **started over** ("scrap that", "revert it").
- **A prompt is clear the first time** when nothing on you followed it. The answer (Rarely, Sometimes, Often, Constantly) comes from the share of prompts with a follow-up that were clear the first time over the last 28 days, once there are at least 10 of them.

The tip names the thing your prompts include least. It only quotes "prompts with guardrails were clear the first time X% vs Y%" when your own history shows a real gap.

### How goals work

- **One goal at a time**, across all AI: a habit ("include guardrails in 25% of your prompts"), a ceiling on one kind of follow-up ("add more after sending in 15% of tasks or fewer"), or a tier to reach.
- **Suggested from your numbers.** A habit goal asks for 15 points more than your last 8 weeks, capped at 80%, so it's a stretch, not a wall.
- **Weeks run Monday to Sunday**, local time, counting only tasks from the day you set the goal. A week needs 5 prompts to count; quieter weeks pause the goal instead of breaking your streak.
- **Met 4 weeks, it's done**, and it goes on your trophy shelf. Finished weeks are frozen, so a rules update or an import can't take back a week you earned.

### Where the thresholds come from

There's no public benchmark for weekly per-person scores, so every threshold was checked three ways:

- **The author's history:** 8,600 messages.
- **Public chats:** about 3,400 English chats from [WildChat-1M](https://huggingface.co/datasets/allenai/WildChat-1M), run through these exact rules.
- **Published figures:** Claude Code users interrupt in 5–9% of turns ([Anthropic](https://www.anthropic.com/research/measuring-agent-autonomy)); in multi-turn developer chats in the [DevGPT](https://arxiv.org/abs/2309.03914) dataset, 14–17% of prompts refine the request.

What that showed:

- The correction rate agrees across sources: 1 in 47 messages for the author, 1 in 41 in WildChat.
- Public chats are short and almost never need follow-ups, so the prompt floors come from one heavy user, whose typical month had 86% of prompts clear the first time (Tech lead). Expect them to move as more people's numbers exist.
- Longer prompts didn't mean fewer follow-ups. Bigger tasks get both longer prompts and more back-and-forth, which is why the tier scores outcomes, not length.
- The overall score puts the author's typical month at 86 (Sharp-eyed): 83% of 150 prompts clear the first time, 41 corrections, and the AI corrected them twice.

## Supported tools

| Tool | How |
|---|---|
| Claude Code | Live |
| Claude Cowork | Live |
| Codex (CLI and desktop) | Live |
| ChatGPT | Import your data export |
| Claude.ai | Import your data export |

Planned: Gemini CLI, Ollama, Open WebUI, LM Studio, VS Code Copilot, Cursor and Aider, plus a browser extension for live ChatGPT, Claude.ai and Gemini. Want one sooner? [Request it](https://github.com/HTMLGuyLLC/NoActually/issues/new/choose), or add it yourself: an adapter is about 100 lines plus a test (see [CONTRIBUTING.md](CONTRIBUTING.md#add-a-source-adapter)).

## Download and install

Open the **[latest release](https://github.com/HTMLGuyLLC/NoActually/releases/latest)** and scroll to **Assets** (click it to expand the list if needed). Download the one file for your computer:

| Your computer | Download | Install |
|---|---|---|
| **Mac** (macOS 12+, Apple silicon or Intel) | `No-Actually-<version>-Mac-universal.dmg` | Open it, drag **No, Actually...** into **Applications**, and open it from there. It's signed and notarized, so there are no warnings. |
| **Windows** | `No-Actually-<version>-Windows-Setup-x64.exe` | Run it. If you see "Windows protected your PC", click **More info → Run anyway** (the Windows build isn't code-signed yet). |
| **Linux** | `No-Actually-<version>-x86_64.AppImage` | Right-click → Properties → allow it to run as a program (or `chmod +x`), then open it. |

The other files are for special cases:
- `-Windows-IT-Installer-x64.msi` is for IT teams deploying to many PCs.
- `-Linux-Ubuntu-Debian-amd64.deb` and `-Linux-Fedora-x86_64.rpm` install through your package manager.
- `SHA256SUMS.txt` lets you verify a download.

The app checks for new versions and shows **Restart to update** when one is ready. Nothing is downloaded or installed until you click it, and an update only installs if its signature matches the project's key. Turn the check off in Settings → General.

## For developers

<p>
  <a href="https://github.com/HTMLGuyLLC/NoActually/actions/workflows/ci.yml"><img src="https://github.com/HTMLGuyLLC/NoActually/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-e0322b" alt="MIT license"></a>
  <img src="https://img.shields.io/badge/built%20with-Tauri%202%20%2B%20Rust-141a2e" alt="Built with Tauri 2 and Rust">
  <img src="https://img.shields.io/badge/telemetry-none-2f9e44" alt="No telemetry">
</p>

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
cargo run --release -p noactually-cli -- check "Fix the crash in checkout.ts. Done when the tests pass."
```

`stats` prints the overall score and both halves. `check "<prompt>"` grades a draft prompt, like the app's checker. Set `NOACTUALLY_DB=/path/to/test.db` to use a different database than your real one. Every command is in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#command-line).

### How it's built

```
crates/engine    correction rules, prompt grading, the overall score, model names, redaction: pure Rust, no I/O, WASM-ready
crates/sources   one adapter per AI tool (Claude Code, Codex, exports…)
crates/store     SQLite: consent, checkpoints, daily counts, tasks, goals, retention, rules versioning
crates/cli       the `noactually` command
app/             Tauri 2 desktop app: menu bar, scorecard, Improve and Learn, notifications
```

- **Crash-safe checkpoints.** A turn is only counted once its reply exists, and each file's writes commit in a single transaction.
- **Hashed message keys.** Resumed sessions, forks and re-imports are never counted twice.
- **Rules versioning.** When the rules change, scores are rebuilt from history. Pinned corrections are always kept; other history counts again only if its files still exist.

The full design is in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

### Contributing

Issues and PRs are welcome. Good places to start:
- Support for another AI tool
- A rule that miscounted one of your chats (use the **Wrong correction** form, and redact the text first)
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

## License

[MIT](LICENSE) © HTMLGuy LLC. Provided as is, without warranty. Bundled fonts (SIL OFL 1.1) and dependency licenses are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

Not affiliated with or endorsed by Anthropic, OpenAI or any other AI provider. Claude, ChatGPT, Codex and other product names are trademarks of their owners, used here only to say which tools it can read.
