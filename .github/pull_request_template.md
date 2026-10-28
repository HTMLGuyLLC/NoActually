## What and why

<!-- One or two sentences. -->

## Checklist

- [ ] `cargo fmt --all --check` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] Rule changes (`crates/engine/src/rules.rs`) have a test in `crates/engine/src/tests.rs`, and no rule id was renamed
- [ ] New source adapters have a fixture test with made-up data
- [ ] No raw chat data, real transcripts, or personal paths are committed
