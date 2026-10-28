# Contributing

Thanks for helping. Small, focused pull requests are easiest to review.

## Setup

- Rust via [rustup](https://rustup.rs). `rust-toolchain.toml` pins stable with rustfmt and clippy.
- Node 20+ (22 works) and [pnpm](https://pnpm.io) 10.
- On Linux, Tauri needs: `libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libssl-dev build-essential`.

Run the app:

```bash
cd app && pnpm install && pnpm tauri dev
```

The app crate reads `app/dist` at compile time. If clippy or tests fail on it, run `pnpm --dir app build` first.

## CLI

```bash
cargo run -p noactually-cli -- sources   # list detected sources
cargo run -p noactually-cli -- allow <source>
cargo run -p noactually-cli -- sync
cargo run -p noactually-cli -- stats
```

Set `WA_DB` to use a throwaway database instead of your real one:

```bash
WA_DB=/tmp/wa-test.db cargo run -p noactually-cli -- stats
```

## Add a source adapter

1. Add a module in `crates/sources` and implement the `Source` trait:
   - `roots()` where the tool keeps its history
   - `files(root)` the files to read under a root
   - `scan(path, from, finalize)` turn a file into `Turn`s
   - `append_only(path)` whether the file only grows (enables tailing by byte offset)
2. Register it with the other sources.
3. Add a fixture test with a small, **made-up** file in the tool's real format.
4. Never commit real chats, even redacted. Invent the text.

Only add a source you have real files for. Format notes alone are not enough.

## Change a rule

- Rules live in `crates/engine/src/rules.rs`.
- Every rule change needs a test in `crates/engine/src/tests.rs`. Cover the case you add and one near-miss that must not match.
- Rule ids are stable. Don't rename or reuse one. Stored events refer to them.
- Keep rules deterministic: no network, no model calls.

## Code style

Before pushing:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm --dir app check
pnpm --dir app test
```

`.editorconfig` sets indentation: 4 spaces for Rust, 2 for everything else.

## Privacy rules for contributors

- No raw chat data in commits, issues, tests or screenshots.
- No new network calls or telemetry. See [PRIVACY.md](PRIVACY.md).
