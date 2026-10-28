# Changelog

All notable changes are listed here. Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Fixed

- CI runs with a read-only GitHub token (code scanning: missing workflow permissions).
- The "No." verdict no longer replays its animation every time a background check finishes. It plays once at launch, and again only when the verdict changes.

## [0.1.0]

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

[Unreleased]: https://github.com/HTMLGuyLLC/NoActually/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/HTMLGuyLLC/NoActually/releases/tag/v0.1.0
