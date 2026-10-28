# Privacy

No, Actually... runs on your computer. Your chats are not sent anywhere.

## What it reads

Only sources you allow. On first launch the app lists each AI tool it detects, with the exact path it would read. Every source is off until you tap **Allow**. You can also set a backfill window (everything, since a date, or none), pause a source, or revoke it.

macOS may show its own "access data from other apps" prompt for some sources. Saying no there only means that source can't be read.

## What it stores

| Data | Stored | Text? | Lifetime |
|---|---|---|---|
| Ordinary messages (not catches) | Counts only, per day, source and model | Never | Forever (just numbers) |
| Sessions | Hashed id, source, day range, message and catch counts | Never | Forever |
| Catch / humbled events | Time, source, model, label, category, matched rule ids | Per retention level | Per TTL |
| File checkpoints | Path, byte offset, size, modified time | Never | Until you forget the source |
| Message fingerprints | Short hashes of message ids, so copies are never counted twice | Never | Until you forget the source |

## Retention levels

You choose how much catch text to keep:

| Level | What's kept | Default TTL, then... |
|---|---|---|
| Full | Your whole message and the start of the AI's reply (redacted) | 30 days, then Snippet |
| Snippet (default) | The matched sentence from each side, 140 characters or less, redacted | 365 days, then Category |
| Category only | No text at all | Forever |

Levels only step down (Full, then Snippet, then Category). The step-down runs after every check (at launch, when files change, and every 5 minutes). You pick the starting level and both TTLs. Pinning a catch to the Hall of Fame keeps its text until you unpin it.

## Redaction

Stored text has emails, URLs, keys, numbers and home paths removed. This is done by fixed rules, so it can miss things. Use Category only if that matters to you.

## What never leaves your machine

- No telemetry, analytics or crash reporting
- The only network use is checking for app updates (below) and opening a share link when you click it
- Share cards are rendered locally
- Fonts are bundled, not fetched

A future global board would be a separate opt-in that sends numbers only.

## Update checks

To offer new versions, the app downloads one small public file, `latest.json`, from this project's GitHub releases (`https://github.com/HTMLGuyLLC/NoActually/releases/latest/download/latest.json`). It does this a minute after launch and then every 6 hours.

- The request carries no identifiers: no account, install id, stats or chat data. It's a plain HTTPS GET with the updater's standard user agent (`tauri-plugin-updater/<version>`). Like any download, GitHub sees your IP address.
- `latest.json` only lists the newest version number and where to download it. If it's newer than yours, you see "Restart to update" in the scorecard and the menu bar menu.
- Nothing is downloaded or installed until you click it. The update file comes from the same GitHub release, and the app installs it only if its signature matches the project's key built into the app.
- Turn it off in **Settings → Check for updates automatically**. With it off, the app only checks when you click **Check for updates**.

## How to delete your data

- **Forget a source** in the app: deletes that source's events, aggregates and checkpoints. (Revoke only stops reading.)
- **Delete everything:** quit the app and delete the three `noactually.db*` files (`noactually.db`, `-wal`, `-shm`) in the app data folder for `app.noactually` (macOS: `~/Library/Application Support/app.noactually/`, Windows: `%APPDATA%\app.noactually\`, Linux: `~/.local/share/app.noactually/`).
- Deleted text is overwritten in the database, not just unlinked, and the write-ahead log is truncated after Forget and after text ages out.

## Logs

The app keeps a small log for troubleshooting (macOS: `~/Library/Logs/app.noactually/`, up to about 2 MB). It records counts, timings and errors. Errors can name history files by path, which includes your project folder names. Logs never contain chat text.

Uninstalling the app does not remove the database. Delete it yourself.
