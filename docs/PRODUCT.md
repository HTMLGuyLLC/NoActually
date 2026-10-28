# No, Actually: product description

## One line

A local-first app that watches your AI chats, counts the times you caught the AI being wrong, and turns it into a score you can be proud of and share.

> **Could Claude replace you?** No. You caught it 10 times this week.

## Who it's for

Anyone who uses AI chat: ChatGPT, Claude, Gemini, Claude Code, Codex. Developers are the first wave (they live in transcripts and GitHub READMEs), but the copy, badges and share cards are written for everyone.

## Principles

- **Deterministic.** Every label comes from fixed text rules. The same history always gives the same score, and anyone can read the rules in the repo. No LLM judge in the scoring path.
- **Local-first.** Raw chats never leave the machine. Only numbers are ever shared, and only when the user taps Share.
- **Open source, MIT.** Trust comes from people being able to read exactly what it looks at.
- **Fun, and honest.** Few corrections doesn't mean you're great. The bottom tier is "Rubber stamp," because maybe you're just not reading.

## What counts as a catch

Each human message is checked against the AI reply before it and the AI reply after it.

| Label | Rule | Counts |
|---|---|---|
| **Confirmed** | You push back **and** the AI's next reply admits the mistake | Catch |
| **Admitted** | The AI's next reply opens with an admission: "you're right", "good catch", "my mistake", "I was wrong", "I missed" | Catch |
| **Claimed** | Your message opens with correction language ("no, that…", "still not working", "you missed", "I asked you to"), and you aren't answering a question the AI just asked | Catch |
| **Humbled** | The AI's reply opens by correcting you ("Actually…", "Not quite") | Tracked separately |
| **Context** | You add info the AI couldn't have known ("also", "fyi", "btw", "I forgot to mention") | Never a catch |

Catch categories (for badges): made it up, ignored instructions, misread you, missed something, still broken, flat-out wrong.

The strongest signal is the AI admitting it. That signal works the same on every chat product, which is what makes this portable.

### Measured on real data (prototype, Shane's Claude Code history)

| Label | Count | Spot-check precision |
|---|---|---|
| Confirmed | 7 | 7 / 7 |
| Admitted | 103 | ~11 / 15 |
| Claimed | 16 | 13 / 14 |
| Humbled | 3 | 3 / 3 |

Spot-checks are small hand-reviewed samples. Before publishing accuracy claims, hand-label ~200 messages into `fixtures/` and report precision/recall in CI.

Known weak spot: polite "you're right" replies where nothing was wrong. Fix candidates: require a correction word within the admission sentence, or an optional (off by default) local-model tiebreak that never changes the deterministic score.

## Scoring

| Metric | Definition |
|---|---|
| Catches | Confirmed + Admitted + Claimed |
| You vs AI | Catches vs Humbled |
| Catch-in-chat % | Share of real chats (10+ messages) with at least one catch |
| 1 in N | Messages per catch |
| Streak | Consecutive days with a catch |
| Tier | Catches in the last 7 days: 15+ Irreplaceable · 7–14 Sharp-eyed · 3–6 Coworker · 0–2 Rubber stamp |

Lead with the count, back it with the percentage. "36% of my chats" is impressive. "1.9% of my messages" is not, so per-message rates stay secondary.

## Moments (popups)

Native notifications with a Share button:

- 1st, 10th, 50th, 100th, 500th catch
- Tier up (and a gentle tier-down on Monday)
- Streaks: 3, 7, 10, 30 days
- First "made it up" catch (Hallucination hunter)
- First time the AI humbled you
- Sunday: weekly Wrapped card

## Sharing and the viral loop

1. Share card: a PNG rendered locally (1200×630 for links, 1080×1920 for stories).
2. Share text plus a download link: "Could AI replace me? No. I caught it 10× this week. What's yours?"
3. README badge for developers (static shields.io first, dynamic later).
4. Competition happens on social: people post cards and compare.
5. Later: an opt-in global board ("Everyone vs AI": total catches this week, tier distribution, top streaks). It only takes numbers, keyed to an anonymous handle. An open-source client plus a hosted board is a normal split (see WakaTime, github-readme-stats).

## Data sources

| Source | How | Live? |
|---|---|---|
| Claude Code | Watch `~/.claude/projects/**/*.jsonl` | Yes |
| Codex CLI | Watch its local session logs | Yes |
| ChatGPT | Official data export (`conversations.json`) or browser extension | Export: no · Extension: yes |
| Claude.ai | Official data export or browser extension | Export: no · Extension: yes |
| Gemini | Browser extension | Yes |

The ChatGPT desktop app's local cache is encrypted, so it can't be read directly.

## Backbone options

| Option | Quick glance | Full window | Popups | Platforms | Pros | Cons | Effort | Cost |
|---|---|---|---|---|---|---|---|---|
| **A. Native Mac menu bar app** (SwiftUI `MenuBarExtra` + `Window`) | Menu bar | Yes | Native | macOS | Best feel and battery, tiny, strong resume piece | Mac-only, Swift-only engine, ChatGPT still needs an extension | Medium | $99/yr Apple dev (notarize) |
| **B. Tauri 2 tray app** (Rust shell + TS web UI) | Menu bar / system tray | Yes | Native | Mac, Windows, Linux | One codebase for all desktops, reuses the scorecard HTML, small binary, Rust + TS on the resume | Tray APIs a bit rougher than Swift, signing per OS | Medium | $99/yr Apple, Windows signing optional |
| **C. Electron + menubar** | Menu bar / tray | Yes | Native | All | Fastest for a JS dev | ~150 MB, heavy, less impressive | Low–medium | Same signing costs |
| **D. Claude Code plugin + CLI** (`npx …`) | Terminal | Browser tab | Terminal / OS notification | Anywhere Node runs | Hours to build, spreads among devs, no install friction | Devs only, no persistent glance | Low | Free |
| **E. Browser extension** | Toolbar popup | Extension page | In-page toast | Chrome, Firefox, Edge, Safari | Only way to get *live* ChatGPT, Claude.ai, Gemini for everyone | DOM scraping breaks, store reviews, can't see Claude Code | Medium + upkeep | $5 Chrome, Safari via Apple dev |
| **F. Web app / PWA** (upload exports) | — | Yes | — | Any browser | Zero install, great for a demo and share pages | Not live, manual exports | Low | Hosting ~free |

### Chosen: B (Tauri 2) with a Rust engine, E (extension) next

- **Engine** (`crates/engine`, Rust): the rules, model names and redaction. Pure functions with tests. It compiles to WASM, so the browser extension and a web demo can reuse the exact same rules.
- **Desktop app** (Tauri 2): menu bar quick glance, full scorecard window, file watcher, SQLite store, milestone notifications, share card, launch at login.
- **CLI** (`noactually`): the same store and engine from a terminal.
- **Browser extension** (next): live ChatGPT, Claude.ai and Gemini through the WASM engine, feeding the desktop app.

Rust instead of TypeScript because the watcher must keep running while the window is hidden, and macOS throttles hidden webviews.

## Name

**No, Actually.** It's the verdict itself: "Could AI replace you? No, actually."

The working name was "Well, Actually", dropped because wellactually.dev is a live AI dev-tool product with the same name.

Checked 2026-10-27 for "No, Actually":

| Where | Result |
|---|---|
| Apps / products | None found with this name |
| GitHub | No matching projects |
| npm | `noactually` and `no-actually` free |
| Domains | noactually.ai, .io, .dev free · .com, .app taken |

Not a legal trademark search. Run a USPTO search before a paid launch.

## Roadmap

- **v0 (done):** Python prototype over Claude Code history, sample scorecard.
- **v1 (built):** Rust engine with tests, CLI, Tauri app for Claude Code, Cowork, Codex and exports, milestones, share card.
- **v2:** Browser extension (ChatGPT, Claude.ai, Gemini), Codex CLI watcher, weekly Wrapped.
- **v3:** Opt-in global board, dynamic README badge, friend comparisons.
