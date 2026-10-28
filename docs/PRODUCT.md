# No, Actually...: what it is

A local-first app that reads your AI chat history on your own computer, counts the times you caught the AI being wrong, and turns that into a weekly score you can share.

> **Could Claude replace you?** No. You caught it 10 times this week.

## Who it's for

Anyone who uses AI chat: ChatGPT, Claude, Codex, Claude Code. Developers come first, because their tools keep readable transcripts on disk. The copy and share cards are written for everyone.

## Principles

- **Deterministic.** Every label comes from fixed text rules. The same history always gives the same score, and anyone can read the rules in [`crates/engine/src/rules.rs`](../crates/engine/src/rules.rs). No AI judges your chats.
- **Local-first.** Raw chats never leave the machine. Only numbers are ever shared, and only when you click Share.
- **Asks first.** Nothing is read until you allow a source.
- **Open source, MIT.** Trust comes from being able to read exactly what the app looks at.
- **Fun, and honest.** Few corrections doesn't mean you're great. The bottom tier is "Rubber stamp", because maybe you're just not reading.

## How scoring works

Each of your messages is checked against the AI reply before it and the AI reply after it.

| Label | Meaning | Counts as |
|---|---|---|
| **Confirmed** | You push back and the AI admits the mistake | Catch |
| **Admitted** | The AI's next reply opens with an admission ("you're right", "good catch") | Catch |
| **Claimed** | Your message opens with correction language ("no, that…", "still not working"), and you aren't answering a question the AI just asked | Catch |
| **Humbled** | The AI's reply opens by correcting you ("Actually…") | Tracked separately |
| **Context** | You add information the AI couldn't have known ("also", "fyi") | Never a catch |

- A catch is charged to the model that wrote the reply you corrected, so you can compare all AI, a provider, a family, or one exact version.
- Your **tier** comes from your catches in the last 7 days, from Rubber stamp at the bottom to Irreplaceable at the top. The cutoffs live in one place in the Rust code, and the app asks for them instead of keeping its own copy.
- The **verdict** ("No." or "Hmm.") is also decided in Rust, so the app, the menu bar and the command line always agree.
- Known weak spot: a polite "you're right" where nothing was actually wrong can count as an Admitted catch. Wrong counts can be reported with the **Wrong catch** issue form.

How the pieces fit together is in [ARCHITECTURE.md](ARCHITECTURE.md). What is stored and for how long is in [PRIVACY.md](../PRIVACY.md).

## Roadmap

Only things that are not built yet. Nothing here is promised.

- **More sources.** Gemini CLI, Ollama, Open WebUI, LM Studio, VS Code Copilot, Cursor, Aider. Each needs real sample files to write a fixture test from, so none ships on format notes alone.
- **Browser extension** for live ChatGPT, Claude.ai and Gemini, reusing the engine compiled to WASM and feeding the desktop app. (The ChatGPT desktop app's local cache is encrypted, so it can't be read directly.)
- **Weekly recap card.**
- **Opt-in global board** that takes numbers only, keyed to an anonymous handle. A separate, explicit opt-in, never on by default.
- **Code signing for Windows.**
- **Optional local-model summaries** of catches (Ollama or LM Studio), never in the scoring path.

## History

The first version was a Python prototype over Claude Code history. It was replaced by the Rust engine, and the original lives at the git tag `prototype-v0`.
