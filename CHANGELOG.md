# Changelog

All notable changes are listed here. Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed

- A rules update no longer loses history. Chats no longer on your computer, and sources you stopped, keep their counts, and a popup says when your scores were updated.
- The scorecard now shows the same tier for the same habits in any time window. Corrections needed for full credit scale with the window: 3 in 28 days, 1 in 7 days.
- Share dialog: you edit each quote in its own box, and every network and image uses your edits. Images show the same words as the box.
- Report card hooks now stay out of any source you never allowed, not only ones you stopped.
- With automatic update checks off, changing the update channel no longer checks for updates.
- A chat's first message can't count as a correction, since there's nothing yet to correct. It counts as a task.
- `noactually sync` exits non-zero when a file is skipped. `sources`, `sync` and `check` take `--json`. New `pause` and `resume` commands.
- The app honors `CODEX_HOME` and `CLAUDE_CONFIG_DIR`.

### Fixed

- A filter or period with no chats no longer looks like a fresh install. Your pickers stay, with a button to widen the view.
- Redaction now catches many more secrets: prefixed names like `DB_PASSWORD`, JSON keys, Bearer tokens, private key blocks, more API key formats, database logins, and more path styles.
- Imported files are no longer described as being watched.
- Failed checks, toggles and updates now say why, and the error page keeps the app's header and links to the log folder.
- The database and its folder are now private to your account on Mac and Linux.
- A count popup can't repeat after Forget or a rebuild. The "left out" report card count no longer grows when chats are read again.

- "Good catch, let me check" no longer counts as the AI admitting it was wrong. That reply only says the question was smart. The same goes for "You're right to ask" and for "let me check if I missed it". Real admissions still count, including when they come later in the same reply, and "you were right" and "you're correct" now count too. When the AI checks first and only then says you were right, that counts too: "Good catch, let me check", some work, then "You're right". Later messages only count when they say you were right, so a mistake the AI found itself doesn't. A message is now scored once your next message arrives, or after 15 quiet minutes, because the AI may still be working. History is re-read once to apply this (rules version 10).

## [0.5.2] - 2026-10-31

### Added

- **Update channel** in Settings → General: Stable gets full releases, and Beta gets new versions first.

## [0.5.1] - 2026-10-31

### Fixed

- Turning on report cards for Claude Code no longer fails with "The downloaded plugin didn't match this app version": the check now allows the folder Claude Code keeps inside its plugins.

## [0.5.0] - 2026-10-30

### Added

- **Report cards** (optional): finish a task in Claude Code, type `/grade:me` (Codex: `$grade-me`), and your AI gives you a letter grade, A to F, on how you asked: why, what you could have said up front, a better first message to copy, and the extra back-and-forth it cost in minutes (plus dollars at pay-as-you-go prices, when the model has a price). It writes for your level: plain words if you're new, short and technical if you're not. Only follow-ups you could have known up front count against you, and a follow-up that only adds new work is a tip, not a lower grade.
- **Settings → Report cards**: turn report cards on for Claude Code or Codex with one click (the app adds a small plugin, pinned to the app's version and checked against it), see when the last report card arrived, turn off the free one-line reminders, or delete your report card and tip history.
- **Improve** shows how your report cards trend: grades by week, follow-ups that redid work per task, your latest report cards, a line in last week's recap, and "since you started" in What works for you. A popup marks your first straight-A week. A new **Context** lesson ("Say what you already know").
- **Tips as you work** (optional, Claude Code, off by default): when a follow-up could have been in your first message, your AI ends that reply with a one-line tip. Improve shows how many tasks needed one and the tip you get most. Settings shows the exact instructions your AI gets. The app keeps the habit, never the words.
- `noactually stats --report-cards [--json]` prints the same numbers.
- Report cards are kept as numbers only (grade, follow-up counts, which habits were missing, tokens, minutes), never text, and never change your score. The app checks each card came from the chat it's in, so a pasted or copied card is ignored.

### Changed

- On macOS, the window buttons now sit inside the app instead of on a separate title bar.
- The header is one line: the logo and sync status on the left, the menu on the right. The green dot is the check-now button, and clicking "Checked…" shows what the numbers cover.
- Settings opens on **General**.
- The scorecard window now opens at 1085 by 1195 when your screen has room, and as much of that as fits when it doesn't.

## [0.4.0] - 2026-10-29

### Added

- **Could AI replace you?** is now an overall score from 0 to 100 over the last 28 days: 70% how often your prompts are clear the first time, 30% judgment (how rarely the AI has to correct you, per task, once you've corrected it 3 times). Tiers: Rubber stamp, Coworker 65+, Sharp-eyed 80+, Irreplaceable 90+. Correcting fewer mistakes because you prompted well never costs points. Overall and prompt tiers are announced once, the first time you reach each one.
- **Improve** tab: one weekly goal suggested from your numbers (a habit, a ceiling on one kind of follow-up or a higher tier), judged once each week is over, with week-by-week marks, streaks that pause on quiet weeks, trophies, personal bests, last week's recap, charts (prompts clear the first time with tier bands, what your prompts include, grades, follow-ups, correction rate) over 12 weeks, 6 months or all time, and "what works for you" from your own history. Finished goal weeks are frozen. Popups when a goal week is met and when a goal is done.
- **Learn** tab: 12 one-minute lessons in three tracks (the 5 habits, follow up less, correct more), each with a before/after, a template and a quick quiz, plus a prompt checker that grades a draft as you type with the same rules as your score and saves nothing. The next lesson is picked from your prompts. A test grades every lesson example with the real rules and fails if the copy names a product, model or version.
- **Do you correct it?** now has a one-word answer and tiers from judgment: Autopilot (Rarely.), Spot-checker 50+ (Sometimes.), Reviewer 75+ (Often.), Hawk-eyed 90+ (Religiously.), with **Share your corrections** on the Corrections page. The overview shares your score; Prompting and Corrections each share their own answer.
- `noactually check "<prompt>"` grades a draft prompt from the command line.
- The scorecard window opens as tall as your screen allows (940 wide, centered) the first time, then where you left it. If that no longer fits on a connected screen, it goes back to the best fit.
- Settings: **Keep running in the menu bar**. Off makes it an ordinary app with no menu bar or tray icon that quits when its window closes.

### Changed

- Plainer words everywhere you read the app: **prompts** and **corrections** (not briefs and catches), **clear the first time** (not landed: "82% of your prompts were clear the first time"), **follow-ups** (not steering), what a prompt includes (Target, Evidence, Finish line, Guardrails, Purpose) and its grade (Vague, Decent, Specific). The code, database and settings use the same words: the database migrates in place, and history is re-read once (rules version 6).
- Download names now include the architecture: `No-Actually-<version>-Mac-universal.dmg`, `-Windows-Setup-x64.exe`, `-Windows-IT-Installer-x64.msi`, `-x86_64.AppImage`, `-Linux-Ubuntu-Debian-amd64.deb` and `-Linux-Fedora-x86_64.rpm`.
- A time picker on the scorecard (7 days, 28 days, 90 days, all time) sets how far back the whole scorecard looks: the score and tiers, corrections, who corrected whom, prompts, the chart, models, categories and the hall of fame. Popups, goals, the quick glance and the tray stay on the last 28 days. `noactually stats --since YYYY-MM-DD` does the same.
- The Prompting page leads with its answer and one line of why; the Specific/Decent/Vague bar moved down to What your prompts include.
- Prompt rules: "you'll know it's fixed when…" and "done once…" give a finish line; "no error in the logs" is no longer proof; "we want them to come back" gives a purpose. History is re-read once to apply them. A route on its own ("/hi", "/api/users") is a target (rules version 7). "Look at the logs" or "check the output" is evidence, and "you should fix it" is no longer a finish line (rules version 8).
- **Check for updates** (for new chats) sits at the top of Sources; the updater's button, in Settings → General, is now **Check for app updates**.
- The model picker sits beside the time range (on the scorecard and Improve), so it's clear it filters the whole page; the questions name the model in plain text ("Could AI replace you?"). The "Showing … only" chip is gone: pick All AI to go back.
- Tier ladders show your number in your tier's row, instead of a note below. Prompting labels its rows by the answer (Constantly, Often, Sometimes, Rarely) and Corrections does too (Rarely, Sometimes, Often, Religiously); the tier's name and a line on what it means sit under the answer ("Hawk-eyed. You catch AI slipping nearly every time.").
- Click (or hover) a bar in Tasks per week or Corrections per week to see that week's breakdown: what the tasks needed, or which kinds of corrections, and how often it corrected you. Every tooltip is the app's own now, charts included.
- The Hall of fame flips between an award's entries with ‹ › arrows instead of expanding, and each entry is quieter: your quote in ink, no category chip (the award names it).
- Share buttons carry a share icon and a short label (Share your score, Share) everywhere, and each overview card can share its own answer next to its See button.
- Plainer section names: **What your prompts include** and **Why you followed up**. The AI's mistakes are blue in every chart.
- Cards drop labels that repeated the question or the time picker. Scrolling lists leave room for the scrollbar.
- Prompting has a **Prompts per week** chart: each week's prompts split into the ones that were clear the first time and the ones that needed more from you, the same tasks your prompt tier counts.
- Corrections per week no longer colors the busiest week as if it were the good one; it's labeled "Most" instead.
- The header keeps Scorecard, Improve and Learn labeled; Settings is an icon button with sub-tabs: Sources, General and Privacy. **Choose sources** opens the Sources tab.
- The quick glance asks the scorecard's three questions, each answered in a word with a short why, plus your goal with this week's number. A gear opens Settings and **Open app** opens the scorecard; the share and copy buttons are gone (share from the app).
- Live tracking checks at most once a minute while an AI tool is writing (it used to check a few seconds after every write). Under the menu, a refresh button checks now, and hovering the status shows the dates and messages the numbers cover.
- The Hall of fame is an awards wall: one award per kind of correction (and Touché for the times it corrected you), each with its count and best snippet first, ranked the same way every time: pinned, then admitted, then with both sides kept, then the punchiest line, then the newest. Unearned awards wait, dimmed.
- Improve leads with your goal: a bigger card with one number to watch (this week's percent, or your score against the tier). Last week's recap sits beside it.
- The model picker shows the numbers the page is about: clear-the-first-time % on Prompting, corrections on Corrections and Hall of fame, both on the overview.
- Scorecard sections run Overview, Corrections, Prompting, Hall of fame. The Corrections section asks "Do you correct it?" and shows your judgment score.
- The top tabs show icons only on narrow windows.

### Fixed

- The Linux AppImage no longer exits at launch when the system has no tray library. The window shows, and closing it quits the app.
- On Linux desktops that don't show tray icons (GNOME without the AppIndicator extension), the app no longer hides into an icon you can't see. The window stays open, and closing it quits.
- Finished chats are no longer re-read on every check, which kept the app busy for no reason. Existing chats are read once more after the update, then skipped.
- Allowing a source again with more history (say, "All history" after "From now on") now reads the older chats instead of skipping them.
- The window remembers where you left it when you quit with Cmd+Q, log out or restart to update, not only when you close it.
- Settings says so when your system has no menu bar tray, instead of offering a setting that can't work.
- The overview no longer shows two different correction counts for the same period, and the model picker's counts follow the time picker.
- Clicking inside the share dialog's edges no longer closes it. A "Click again to…" button resets after 5 seconds.
- Charts, the week strip and tooltips work from the keyboard, every page has a heading, small red and green text is easier to read in light mode, and stacked bars no longer rely on color alone.
- A very large retention value from the CLI no longer breaks every sync.
- Wording: "It never corrected you", "1 day", "1 was its mistake", "across all time", "Check for new chats", and clearer goal titles ("Add more after sending in 15% of tasks or fewer each week").
- Bundled font and library licenses are listed in `THIRD_PARTY_NOTICES.md`, and Settings → Privacy notes the app is for fun and not affiliated with any AI company.

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

[Unreleased]: https://github.com/HTMLGuyLLC/NoActually/compare/v0.5.2...HEAD
[0.5.2]: https://github.com/HTMLGuyLLC/NoActually/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/HTMLGuyLLC/NoActually/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/HTMLGuyLLC/NoActually/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/HTMLGuyLLC/NoActually/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/HTMLGuyLLC/NoActually/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/HTMLGuyLLC/NoActually/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/HTMLGuyLLC/NoActually/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/HTMLGuyLLC/NoActually/releases/tag/v0.1.0
