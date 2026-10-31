# No, Actually...: what it is

A local-first app that reads your AI chat history on your own computer, counts the times you correct your AI, grades your prompts, and turns that into a 0–100 score you can share.

> **Could Claude replace you?** No. You corrected it 10 times this week.

## Who it's for

Anyone who uses AI chat: ChatGPT, Claude, Codex, Claude Code. Developers come first, because their tools keep readable transcripts on disk. The copy and share cards are written for everyone.

## Principles

- **Deterministic.** Every label comes from fixed text rules. The same history always gives the same score, and anyone can read the rules in [`crates/engine/src/rules.rs`](../crates/engine/src/rules.rs). No AI judges your chats for the score. Optional report cards come from your own AI, are labeled as such, and never change the score.
- **Local-first.** Raw chats never leave the machine. Nothing is shared unless you click Share. Share cards show numbers; Hall of fame cards also quote a short, redacted snippet of one correction, which you can edit before sharing.
- **Asks first.** Nothing is read until you allow a source.
- **Open source, MIT.** Trust comes from being able to read exactly what the app looks at.
- **Fun, and honest.** Few corrections doesn't mean you're great. The bottom tier is "Rubber stamp", because maybe you're just not reading.

## How scoring works

Each of your messages is checked against the AI reply before it and the AI reply after it.

| Label | Meaning | Counts as |
|---|---|---|
| **Confirmed** | You push back and the AI admits the mistake | Correction |
| **Admitted** | The AI's next reply opens with an admission ("you're right", "good catch"), or a later message before your next one says you were right | Correction |
| **Claimed** | Your message opens with correction language ("no, that…", "still not working"), and you aren't answering a question the AI just asked | Correction |
| **Humbled** | The AI's reply opens by correcting you ("Actually…") | Tracked separately |
| **Context** | You add information the AI couldn't have known ("also", "fyi") | Never a correction |

- A correction is charged to the model that wrote the reply you corrected, so you can compare all AI, a provider, a family, or one exact version.
- Your **score** is 0–100 over the last 28 days (or the range you pick): 70% how often your prompts are clear the first time, 30% judgment (how rarely the AI has to correct you, per task, once you've corrected it 3 times). Tiers run from Rubber stamp at the bottom, through Coworker (65+) and Sharp-eyed (80+), to Irreplaceable (90+). The cutoffs live in one place in the Rust code, and the app asks for them instead of keeping its own copy.
- A **prompt** is graded on what it includes: Target, Evidence, Finish line, Guardrails and Purpose. Three or more is Specific, two is Decent, fewer is Vague. Length never counts. A prompt is **clear the first time** when nothing on you (adding more, clarifying, starting over) followed it. The full rules are in the [README](../README.md#how-a-prompt-is-graded).
- The **verdict** ("No." or "Hmm.") is also decided in Rust, so the app, the menu bar and the command line always agree.
- Known weak spot: a polite "you're right" where nothing was actually wrong can count as an Admitted correction. Wrong counts can be reported with the **Wrong correction** issue form.

How the pieces fit together is in [ARCHITECTURE.md](ARCHITECTURE.md). What is stored and for how long is in [PRIVACY.md](../PRIVACY.md).

## Roadmap

Only things that are not built yet. Nothing here is promised.

- **More sources.** Gemini CLI, Ollama, Open WebUI, LM Studio, VS Code Copilot, Cursor, Aider. Each needs real sample files to write a fixture test from, so none ships on format notes alone.
- **Browser extension** for live ChatGPT, Claude.ai and Gemini, reusing the engine compiled to WASM and feeding the desktop app. (The ChatGPT desktop app's local cache is encrypted, so it can't be read directly.)
- **Weekly recap card.**
- **Opt-in global board** that takes numbers only, keyed to an anonymous handle. A separate, explicit opt-in, never on by default.
- **Code signing for Windows.**
- **Optional local-model summaries** of corrections (Ollama or LM Studio), never in the scoring path.

## History

The first version was a Python prototype over Claude Code history. It was replaced by the Rust engine, and the original lives at the git tag `prototype-v0`.
