# Privacy

*Last updated 2026-11-01. No, Actually... is made by HTMLGuy LLC. Questions: [GitHub Discussions](https://github.com/HTMLGuyLLC/NoActually/discussions).*

No, Actually... runs on your computer. The app doesn't send your chats anywhere.

## What it reads

Only sources you allow. On first launch the app lists each AI tool it detects, with the exact path it would read. Every source is off until you tap **Allow**. You can also set a backfill window (everything, since a date, or none), pause a source, or revoke it. While a source is paused, revoked, forgotten or was never allowed, the report card hooks stay out of its chats too. If you install the plugin yourself and never set up the app, there are no sources to allow, so the plugin works in the chats you use it in, grading by eye and saving nothing.

macOS may show its own "access data from other apps" prompt for some sources. Saying no there only means that source can't be read.

## What it stores

| Data | Stored | Text? | Lifetime |
|---|---|---|---|
| Ordinary messages (not corrections) | Counts only, per day, source and model | Never | Forever (just numbers) |
| Sessions | Hashed id, source, day range, message and correction counts | Never | Forever |
| Tasks | Day, source, hashed chat id, model, which of 5 signals the prompt carried, message and follow-up counts | Never | Forever |
| Your goal | What it asks for (a signal, a kind of follow-up or a tier), its target percent and start day | Never | Until you drop it |
| Goal weeks and trophies | Each finished week's count, percent and result; finished goals and the day | Never | Forever (kept even after Forget, since they're totals across sources) |
| Lessons done | The ids of lessons you marked done | Never | Forever |
| Correction / humbled events | Time, source, hashed chat id, model, label, category | Per retention level | Until it steps down (see Retention levels) |
| Report cards | Day, start and end time, source, hashed chat id, model, letter grade, follow-up counts, which habits were missing, tokens, minutes | Never | Until you Forget the source or delete report card history |
| Tips | Day, time, source, hashed chat id, model, which habit, how much was redone | Never | Until you Forget the source or delete report card and tip history |
| Chats that got the tip instructions | Hashed chat id, source, day | Never | Until you Forget the source or delete report card and tip history |
| Sources | Which sources you allowed, the paths of files you imported (which can include your username), and when you allowed them | Never | Until you forget the source |
| File checkpoints | Path, byte offset, size, modified time | Never | Until you forget the source |
| Message fingerprints | Short hashes of message ids and the hashed chat id, so copies are never counted twice | Never | Until you forget the source |
| A backup of the database | `noactually-backup.db`, a copy of all of the above, beside the database and readable only by you | Same as the database | Replaced about once a day. Removed whenever text is deleted or ages out, then written again without it |

## Retention levels

You choose how much correction text to keep:

| Level | What's kept | Kept for (default), then... |
|---|---|---|
| Full | Your whole message and the start of the AI's reply (redacted) | 30 days, then Snippet |
| Snippet (default) | The matched sentence from each side, 140 characters or less, redacted | 365 days, then Category |
| Category only | No text at all | Forever |

Levels only step down (Full, then Snippet, then Category). The step-down runs after every check (at launch, when files change, and every 5 minutes). You pick the starting level and both time limits. Pinning a correction to the Hall of fame keeps its text until you unpin it.

## Redaction

Stored text has emails, URLs, keys, numbers and home paths removed. This is done by fixed rules, so it can miss things. Use Category only if that matters to you.

## What never leaves your machine

- No telemetry, analytics or crash reporting
- The only network use by the app itself is checking for app updates (below), opening a share link when you click it, and, with report cards on, asking GitHub with `git` whether a new version of the plugin exists before moving to it. With report cards on, Claude Code also downloads the plugin from GitHub (see Report cards)
- Share cards are rendered locally
- Fonts are bundled, not fetched

The one exception is text you choose to share. When you share a Hall of fame correction, its redacted quotes go in the link to the site you pick, and you see that text first. Score cards carry numbers only.

## Report cards (optional)

Report cards are off until you turn them on in **Settings → Report cards**. What turning them on does:

- Adds a small plugin to your AI tool. Claude Code downloads it from GitHub, pinned to the app's version. After an app update, Claude Code downloads the matching plugin from GitHub. No, Actually... itself still sends nothing.
- With report cards on, a small check runs after each AI reply and reads the end of that chat on your computer. It uses no AI, sends nothing and saves nothing.
- When you ask for a report card, your AI reads a short summary of the chat that the app makes on your computer. The report card is written inside your AI tool, by the provider that already has the chat, and it uses your tokens. The app only reads the card's small block of numbers from your local chat history and keeps the numbers.
- If you turn on **Give me tips as I work** (Claude Code only, off by default), a small check gives your AI the tip instructions when a chat starts, and again after a long chat is compacted. That's about 480 tokens each time, re-read with the rest of the chat, and it came to about 2.5% more tokens overall in testing. Settings shows those instructions word for word. Tips are part of your AI's replies, so they go wherever a reply goes. The app stores the habit each tip was about and how much was redone, never the words.
- Small marker files beside the database hold these settings: `report-cards-tips`, `report-cards-tips-off`, `source-allowed-…` for a source you allowed, `source-stopped-…` for one you paused, revoked or forgot, and `report-cards-problem` for a plugin problem Settings needs to keep showing. They hold no chat data.
- To find Claude Code again after a restart, the app saves its location (a file path, in `report-cards-claude` beside the database). It is only used to run `claude plugin` commands to add, check and remove the plugin.
- A small secret file, `report-cards.key`, sits in the app data folder. It stops a block of numbers that was copied or pasted from somewhere else (another chat, a web page, a file) from being counted. It does not stop text injected into the same chat, which could still fake that chat's own report card.

Files written outside the app's own folder while report cards are on:

| Tool | Where | What |
|---|---|---|
| Claude Code | Claude Code's plugin folder | The plugin, installed and removed by Claude Code itself |
| Codex | `~/.agents/skills/grade-me/` | A skill and a small script |
| Codex | `~/.codex/hooks.json` | Two entries, added to the file you have. A dated copy of the file is saved beside it each time the app changes it (`hooks.json.bak-…`) |

**Turn report cards off before you uninstall the app.** Turning them off removes the plugin (Claude Code) or those files and entries (Codex). The history stays until you delete it. Uninstalling the app on its own leaves them in place. Chats already open keep any tips they were told to give, so tell your AI "no more tips" in those. Codex's dated `hooks.json.bak-…` copies stay, because they're your safety copies, and you can delete them.

**Turn report cards off before you install an older version.** Versions before report cards don't know these hooks, so each reply would open the app.

A future global board would be a separate opt-in that sends numbers only.

## Update checks

To offer new versions, the app downloads one small public file, `latest.json`, from this project's GitHub releases (`https://github.com/HTMLGuyLLC/NoActually/releases/latest/download/latest.json`, or `https://github.com/HTMLGuyLLC/NoActually/releases/download/beta-channel/latest.json` on the Beta channel). It does this a minute after launch and then every 6 hours.

- The request carries no identifiers: no account, install id, stats or chat data. It's a plain HTTPS GET with the updater's standard user agent (`tauri-plugin-updater/<version>`). Like any download, GitHub sees your IP address.
- `latest.json` only lists the newest version number and where to download it. If it's newer than yours, you see "Restart to update" in the scorecard and the menu bar menu.
- Nothing is downloaded or installed until you click it. The update file comes from the same GitHub release, and the app installs it only if its signature matches the project's key built into the app.
- Turn it off in **Settings → General → Check for app updates automatically**. With it off, the app only checks when you click **Check for app updates** on that page or in the menu bar menu.

## How to delete your data

- **Forget a source** in the app: deletes that source's events, aggregates and checkpoints, pinned corrections included. (Revoke only stops reading, and keeps your scores.) Finished goal weeks and trophies stay, as totals across sources.
- **Delete report cards and tips:** click **Delete report card and tip history** in Settings to remove all of them. **Forget** on a source removes that source's report cards and tips too. Tips stop while Claude Code is paused, revoked or forgotten, and start again when you allow it. Turning report cards off removes the plugin but keeps the history until you delete it.
- **Delete everything:** click **Delete everything** in Settings. It turns off report cards and Launch at login, deletes the data and the logs, and quits. By hand (for example if you already removed the app): first turn report cards off in **Settings → Report cards** (that removes what they added to Claude Code and Codex) and turn off **Launch at login** in **Settings → General**, then quit the app and delete the whole app data folder for `app.noactually` (macOS: `~/Library/Application Support/app.noactually/`, Windows: `%APPDATA%\app.noactually\`, Linux: `~/.local/share/app.noactually/`). It holds the database (`noactually.db`, `-wal` and `-shm`) and a few small report-card files: `report-cards.key`, `cli-path`, `report-cards-claude` and the on/off switch files (`report-cards-tips`, `report-cards-tips-off`, `source-allowed-…`, `source-stopped-…`, `report-cards-problem`, `report-cards-quiet`, and `report-cards-repin` while a plugin update is unfinished). Deleting only the database leaves those behind. Then delete the log folder (see Logs). Uninstalling the app does not remove any of this.
- Deleted text is overwritten in the database, not just unlinked, the write-ahead log is truncated, and the backup is removed after Forget and after text ages out.

## Logs

The app keeps a small log for troubleshooting (macOS: `~/Library/Logs/app.noactually/`, Windows: `%LOCALAPPDATA%\app.noactually\logs\`, Linux: `~/.local/share/app.noactually/logs/`, up to about 4 MB). It records the app version, your system, counts, timings and errors. Errors can name history files by path, which includes your project folder names. Forget doesn't clear the log, so those names stay until the log rolls over or you delete it. Logs never contain chat text.
