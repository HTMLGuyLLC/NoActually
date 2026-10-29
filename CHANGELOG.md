# Changelog

All notable changes are listed here. Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **Could AI replace you?** is now an overall score from 0 to 100 over the last 28 days: 70% how often your tasks land, 30% judgment (how rarely the AI has to correct you, per task, once you've caught it 3 times). Tiers: Rubber stamp, Coworker 65+, Sharp-eyed 80+, Irreplaceable 90+. Catching fewer mistakes because you briefed well never costs points. Overall and brief tiers are announced once, the first time you reach each one.
- **Improve** tab: one weekly goal suggested from your numbers (a habit, a steering ceiling or a higher tier), judged once each week is over, with week-by-week marks, streaks that pause on quiet weeks, trophies, personal bests, last week's recap, charts (tasks landed with tier bands, what your briefs carry, grades, steering, catch rate) over 12 weeks, 6 months or all time, and "what works for you" from your own history. Finished goal weeks are frozen. Popups when a goal week is met and when a goal is done.
- **Learn** tab: 12 one-minute lessons in three tracks (the 5 habits, steer less, catch more), each with a before/after, a template and a quick quiz, plus a brief checker that grades a draft as you type with the same rules as your score and saves nothing. The next lesson is picked from your briefs. A test grades every lesson example with the real rules and fails if the copy names a product, model or version.
- `noactually check "<brief>"` grades a draft brief from the command line.
- Settings: **Keep running in the menu bar**. Off makes it an ordinary app with no menu bar or tray icon that quits when its window closes.

### Changed

- The model picker shows the numbers the page is about: landed % on Briefs, catches on Catches and Hall of fame, both on the overview.
- Scorecard sections run Overview, Briefs, Catches, Hall of fame. The Catches section asks "Do you catch its mistakes?" and shows your judgment score.
- The top tabs show icons only on narrow windows.

## [0.3.0] - 2026-10-28

### Added

- How you brief it: a second scorecard, "Do you make it guess?". Each chat's first message (or a "now…" / "next:" later on) is a task. Its brief is graded on what it carries (points at it, shows it, says what done is, sets limits, says why), and later messages that redirect it count as steering: the AI got it wrong, you added more, you clarified, or you started over. The tier (Vibes only 0%, Ticket writer 70%, Tech lead 80%, Architect 90%) is the share of tasks that landed without steering that was on you. It counts tasks with a follow-up over the last 28 days, and needs at least 10 of them. Includes a tip, a share card, a quick-glance line, a tier-up popup, and the same numbers in `noactually stats`. Only counts are stored, never text.

### Changed

- The scorecard is split into sections: Overview (both verdicts side by side, plus the headline stats), Catches, Briefs and Hall of fame. The tab row scrolls sideways instead of wrapping on narrow windows.
- The top tabs have icons, the logo takes you back to the scorecard, and cards drop the ruled-line background.
- Rules version 4: existing history is re-read once to fill in briefs. Pinned catches are kept; other catches are re-counted from history files that still exist.
- A rule's "but not when…" exception now skips just that match, so a later match in the same message can still count.

### Fixed

- Live tracking now keeps each message's context. A read that ended on an AI reply used to start the next read after it, so the next message lost the reply it answered. That made "No, wait until tomorrow" in answer to a question look like pushback. Full rebuilds were never affected.
- Claude Code IDE context (the open file or selection) is no longer read as part of what you typed.

## [0.2.1] - 2026-10-28

### Fixed

- The Instagram share card is now a tall 4:5 image (1080×1350) that fills the Instagram feed, instead of the wide link-preview card shrunk into a thin strip. Other networks keep the wide card.

## [0.2.0] - 2026-10-28

### Added

- In-app updates. The app checks GitHub releases for a newer version a minute after launch and every 6 hours, and offers "Restart to update" in the scorecard and the menu bar menu. It never restarts on its own, and only installs updates signed with the project's key. The check sends no identifiers; turn it off in Settings → Check for updates automatically.
- `meta` command: the UI reads tiers, categories and sources from Rust, and scorecard data includes the verdict, tier ids, and category and source names, so the app and CLI can't disagree.

### Changed

- `NOACTUALLY_DB` picks the database file. `WA_DB` still works but is deprecated.
- `noactually import` also allows the source, like the app does.
- The CLI rejects unknown options and explains missing values.
- Keyboard focus survives redraws and returns to the opener after the share dialog. The model picker is a proper combobox and closes when focus leaves. Tabs announce the current page, and the weekly chart has a text summary.
- Better contrast for category chips and the "Pinned" tag. All motion respects "reduce motion".

### Fixed

- Replies like "No, that's fine, ship it" or "Actually, that works" no longer count as catches. "No, it worked before you changed it" still does.
- Catch categories are more accurate: an AI apologizing with "my error" no longer files a catch as "still broken", and "that's not what I meant" shows as a misread. Scores rebuild once with the updated rules.
- Pinned catches are kept when scores rebuild after a rules update, even if their original chat file is gone.
- If the saved text-retention setting can't be read, new catches keep no text instead of falling back to the defaults, and nothing already stored is deleted until the setting is fixed.
- Deleted text is more reliably cleared from the database's log file, and importing an export no longer waits on a running sync.
- The "No." verdict no longer replays its animation every time a background check finishes. It plays once at launch, and again only when the verdict changes.
- A ChatGPT, Claude.ai or Codex `.zst` file can no longer exhaust memory when decompressed (ruzstd 0.9.1, RUSTSEC decompression bomb).
- Messages skipped by a "since" date or "From now on" are counted if you later allow more history.
- A database error on one file no longer stops the whole sync.
- Two quick edits to a history file in the same second are always noticed.
- The app and CLI never read sources or open the database from a relative path when no home or app data folder is available.
- A failed Allow, Pin, Forget, Pause, Import or Install no longer leaves its button stuck; the error shows next to it.
- The update banner escapes the version string, the filter label can no longer read "undefined", and retention days are limited to 1–3650.

### Security

- The production content security policy no longer allows inline styles.
- CI runs with a read-only GitHub token, and the release workflow uses least-privilege permissions and a pinned `tauri-action`.

### Internal

- Rust: typed errors, shared readers, workspace lints (clippy pedantic, no `unsafe`), `cargo-deny`, MSRV 1.90 (app included), and 106 tests (up from 49) with no sleeps. Every scoring rule has a positive and a near-miss example, and a test keeps the browser demo data in step with what Rust sends.
- UI: split into small tested modules with one delegated event handler and an auto-escaping `html` template; Biome and Vitest with coverage (145 tests).
- CI: separate rust (Linux, macOS, Windows), frontend, scripts, cargo-deny, MSRV, wasm32, coverage and version jobs behind one required `ci-ok` check. Every action is pinned by commit, and ShellCheck and actionlint run on every change.
- Release: `scripts/bump-version.sh`, one `scripts/version.sh` for reading and checking the version, manual Release runs that only build, a `justfile` (`just ci` runs every check locally), and a macOS release script that refuses a dirty tree or a mismatched tag.
- Removed the Python prototype (kept at tag `prototype-v0`) and the sample scorecard.

## [0.1.0] - 2026-10-27

First release.

### Added

- Live tracking for Claude Code, Claude Cowork and Codex (CLI and desktop)
- Import of ChatGPT and Claude.ai data exports
- Deterministic rules for catches (Confirmed, Admitted, Claimed) and Humbled moments, with no AI in the scoring path
- Per-model breakdown (provider, family, version) with a searchable model picker
- Retention levels for catch text (Full, Snippet, Category only) with TTLs that step down over time
- Menu bar quick glance and a full scorecard window
- Milestone notifications (catch counts, tier ups, streaks), with no backlog spam on first import
- Share dialog for X, LinkedIn, Facebook, Threads, Bluesky, Reddit, WhatsApp and Instagram, showing exactly what will be posted and flagging networks where you paste the text yourself. Also saves a share card image or copies the text. Only numbers and the project link are shared.
- Share any Hall of Fame catch: the post quotes your redacted snippet, and you can edit it before sharing. It comes with its own catch card image. Pinned catches stand out.
- Universal macOS build (Apple silicon and Intel), signed with Developer ID and notarized by Apple
- Rules versioning: when scoring rules change, scores are rebuilt from history, keeping your source choices and settings
- Launch at login, quietly in the menu bar
- Redaction of secrets, emails, links, paths and long numbers in any stored text
- Bundled fonts and a strict content security policy: the app makes no network requests
- Per-source consent: nothing is read until you allow it, and Forget deletes a source's data
- `noactually` command-line tool (`sources`, `allow`, `sync`, `stats`)

[Unreleased]: https://github.com/HTMLGuyLLC/NoActually/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/HTMLGuyLLC/NoActually/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/HTMLGuyLLC/NoActually/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/HTMLGuyLLC/NoActually/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/HTMLGuyLLC/NoActually/releases/tag/v0.1.0
