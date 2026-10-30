# Contributing

Thanks for helping. Small, focused pull requests are easiest to review.

## Setup

- Rust via [rustup](https://rustup.rs). `rust-toolchain.toml` pins the toolchain, with rustfmt and clippy.
- Node 20.19+ (22 works) and [pnpm](https://pnpm.io) 10 (the exact version is in `app/package.json`).
- [just](https://github.com/casey/just) to run the checks: `brew install just` or `cargo install just`. Optional but recommended; every recipe is plain commands you can copy from the `justfile`.
- Optional for the full CI set: `cargo install cargo-deny`, and ShellCheck (`brew install shellcheck`).
- On Linux, Tauri needs: `libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libssl-dev build-essential`.

Run the app:

```bash
just dev    # same as: cd app && pnpm install && pnpm tauri dev
```

The app crate reads `app/dist` at compile time. If clippy or tests fail on it, run `pnpm --dir app build` first.

## CLI

```bash
cargo run -p noactually-cli -- sources   # list detected sources
cargo run -p noactually-cli -- allow <source>
cargo run -p noactually-cli -- sync
cargo run -p noactually-cli -- stats
```

Set `NOACTUALLY_DB` to use a throwaway database instead of your real one (`WA_DB` still works but is deprecated):

```bash
NOACTUALLY_DB=/tmp/na-test.db cargo run -p noactually-cli -- stats
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

## Checks and code style

Before pushing, run everything CI runs:

```bash
just ci
```

It's the same as running these by hand (build the frontend first; the app crate reads `app/dist` at compile time):

```bash
pnpm --dir app install --frozen-lockfile
pnpm --dir app build
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm --dir app check
pnpm --dir app lint
pnpm --dir app test
node --test scripts/*.test.mjs
```

- **Rust** is formatted with `cargo fmt`; clippy warnings fail the build.
- **TypeScript, CSS and JSON in `app/`** are formatted and linted by [Biome](https://biomejs.dev). `just fmt` formats both Rust and TypeScript.
- `.editorconfig` sets indentation: 4 spaces for Rust, 2 for everything else.

CI also runs cargo-deny, an MSRV check, a wasm32 build of the engine and a pnpm audit. Only the `ci-ok` job is required to merge; it passes when all the others do.

## Tests

- New or changed behavior comes with tests, following the existing style in the crate or folder you touch.
- Rules: a test for the new case and one near-miss that must not match (`crates/engine/src/tests.rs`).
- Source adapters: a fixture test with a small, made-up file.
- Frontend logic: `vitest` tests next to the code in `app/src`.
- While working, run just the tests for what you changed (for example `cargo test -p noactually-engine`); `just ci` runs everything.
- Add a line under **Unreleased** in `CHANGELOG.md` for anything a user would notice.

## Privacy rules for contributors

- No raw chat data in commits, issues, tests or screenshots.
- No new network calls or telemetry. The update check is the one exception, and it sends nothing about the user. See [PRIVACY.md](PRIVACY.md).

## Releasing (maintainers)

### One-time setup: the updater key

In-app updates are signed with a Tauri updater key. Its public half is in `app/src-tauri/tauri.conf.json` (`plugins.updater.pubkey`); the private half never goes in the repo.

- The private key lives at `~/.tauri/noactually.key` (mode 600) on the release Mac, and its password in the login keychain as `noactually-updater-key`. `scripts/release-macos.sh` reads both.
- Add both to the repo's Actions secrets so CI can sign Windows and Linux updates:

  ```bash
  gh secret set TAURI_SIGNING_PRIVATE_KEY -R HTMLGuyLLC/NoActually < ~/.tauri/noactually.key
  security find-generic-password -s noactually-updater-key -w | gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD -R HTMLGuyLLC/NoActually
  ```

- Back up the key and password somewhere safe (a password manager). If the key is lost, installed copies can't be updated in-app anymore; people would have to download the next version by hand.

`pnpm tauri build` fails without the key because `bundle.createUpdaterArtifacts` is on. To build locally without it, add `-c '{"bundle":{"createUpdaterArtifacts":false}}'`.

### Each release

1. Make sure `main` is green and `CHANGELOG.md` has the user-facing changes under **Unreleased**.
2. Run `just bump 0.2.0` (`scripts/bump-version.sh 0.2.0`). It sets the version in `Cargo.toml` (the source of truth), `app/package.json` and, if present, `app/src-tauri/tauri.conf.json`, refreshes `Cargo.lock`, and moves the Unreleased notes under a dated `## [0.2.0]` heading with updated compare links. Commit it on a branch (`git switch -c release-0.2.0 && git commit -am "release: v0.2.0"`) and merge it with a pull request once CI is green (`main` only takes pull requests).
3. Tag the merged commit on `main` and push the tag: `git switch main && git pull && git tag v0.2.0 && git push origin v0.2.0`. The Release workflow checks the tag against the Cargo version, then builds Windows and Linux into a draft release, with signed update files and a `latest.json` for them.
4. On a Mac with the Developer ID certificate and the updater key, check out the tag and run `just release-mac` (`scripts/release-macos.sh`). It refuses to run on a dirty tree or when HEAD isn't the tagged commit (`--force` skips the tag check). It builds a universal app, signs it, notarizes and staples both the app and the DMG, and makes the signed update file (`No-Actually-0.2.0-Mac-Update-universal.app.tar.gz` + `.sig`).
5. Run `scripts/finish-release.sh v0.2.0`. It renames the files to plain names people can pick from ("No-Actually-0.2.0-Mac-universal.dmg", "-Windows-Setup-x64.exe", "-x86_64.AppImage"…), setting both the asset name and the label tauri-action v1 adds; uploads the Mac DMG and update file; rewrites `latest.json` so every entry points at the renamed files and adds the Mac entries (`darwin-universal`, `darwin-aarch64`, `darwin-x86_64`); and writes `SHA256SUMS.txt`. It refuses to write a `latest.json` that points at a file missing from the release.
6. Check the draft and publish it (`gh release edit v0.2.0 -R HTMLGuyLLC/NoActually --draft=false`). Installed apps see the update only once it's published, because they read `releases/latest/download/latest.json`.

If the `APPLE_*` secrets are set in the repo, the Release workflow also signs and notarizes macOS, and step 4 isn't needed.
