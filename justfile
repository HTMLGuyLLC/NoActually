# Task runner: https://github.com/casey/just  (brew install just  |  cargo install just)
# `just` with no arguments lists the recipes.

set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

# Install frontend dependencies exactly as locked.
install:
    pnpm --dir app install --frozen-lockfile

# Most of the checks CI runs on every change. CI also runs the MSRV check, the coverage summary, actionlint and the app self-check.
ci: install build fmt-check clippy test-rust test-frontend test-scripts wasm versions audit

# Build the frontend. The app crate reads app/dist at compile time, so cargo needs this first.
build: install
    pnpm --dir app build

# Format Rust and the frontend (Biome).
fmt:
    cargo fmt --all
    pnpm --dir app format

fmt-check:
    cargo fmt --all --check

# Needs app/dist, which `build` makes.
clippy: build
    cargo clippy --locked --workspace --all-targets -- -D warnings

# Lint everything: clippy and Biome.
lint: clippy
    pnpm --dir app lint

# All tests.
test: test-rust test-frontend test-scripts

# Needs app/dist, which `build` makes.
test-rust: build
    cargo test --locked --workspace

test-frontend:
    pnpm --dir app check
    pnpm --dir app lint
    pnpm --dir app test:coverage

test-scripts:
    node --test scripts/*.test.mjs
    @if command -v shellcheck >/dev/null; then shellcheck scripts/*.sh plugins/grade/bin/na; else echo "shellcheck not installed; skipped (brew install shellcheck)"; fi

# The engine has no I/O, so it builds for the web too.
wasm:
    cargo check --locked -p noactually-engine --target wasm32-unknown-unknown

# Cargo.toml, app/package.json and tauri.conf.json must agree on the version.
versions:
    scripts/version.sh check

# Dependency checks: cargo-deny (licenses, advisories, sources) and pnpm audit.
audit:
    @if command -v cargo-deny >/dev/null; then cargo deny check; else echo "cargo-deny not installed; skipped (cargo install cargo-deny)"; fi
    pnpm --dir app audit --prod

# Run the desktop app in development.
dev: install
    cd app && pnpm tauri dev

# Set the version everywhere and move the changelog: just bump 0.2.0
bump VERSION:
    scripts/bump-version.sh {{VERSION}}

# Create the draft release, tag the merged commit and push the tag (main, CI green): just start-release v0.2.0
start-release TAG:
    scripts/start-release.sh {{TAG}}

# Finish the draft after release-mac: rename the files, upload the Mac build, write latest.json and SHA256SUMS.txt: just finish-release v0.2.0
finish-release TAG *ARGS:
    scripts/finish-release.sh {{TAG}} {{ARGS}}

# Publish a finished draft to beta, or a beta to everyone: just publish v0.2.0 beta
publish TAG CHANNEL:
    scripts/publish-release.sh {{TAG}} {{CHANNEL}}

# Build, sign, notarize and staple the macOS release (on the release Mac, with HEAD at the tag).
release-mac *ARGS:
    scripts/release-macos.sh {{ARGS}}
