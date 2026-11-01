## What and why

<!-- One or two sentences. -->

## Checklist

- [ ] `just ci` passes (or the equivalent commands in CONTRIBUTING.md)
- [ ] New or changed behavior has tests
- [ ] User-visible changes are noted in `CHANGELOG.md` under Unreleased
- [ ] Rule changes (`crates/engine/src/rules.rs`) have a test in `crates/engine/src/tests.rs`, and no rule id was renamed
- [ ] New source adapters have a fixture test with made-up data
- [ ] No raw chat data, real transcripts, or personal paths are committed (tests, screenshots and issue text included)
- [ ] Changes to `plugins/grade/skills/me/SKILL.md` or `crates/engine/src/tip_rules.txt` were checked with the report card evals (`node scripts/report-card-evals/run.mjs`), and the result is noted here
