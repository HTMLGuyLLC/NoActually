# Task runner: https://github.com/casey/just  (brew install just  |  cargo install just)
# `just` with no arguments lists the recipes.

set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

# Install frontend dependencies exactly as locked.
install:
    pnpm --dir app install --frozen-lockfile

# Everything CI runs, in CI order. Run this before pushing.
ci: install build fmt-check clippy test-rust test-frontend test-scripts audit

# Build the frontend. The app crate reads app/dist at compile time, so cargo needs this first.
build: install
    pnpm --dir app build

# Format Rust and the frontend (Biome).
fmt:
    cargo fmt --all
    pnpm --dir app format

fmt-check:
    cargo fmt --all --check

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Lint everything: clippy and Biome.
lint: clippy
    pnpm --dir app lint

# All tests.
test: test-rust test-frontend test-scripts

test-rust:
    cargo test --workspace

test-frontend:
    pnpm --dir app check
    pnpm --dir app lint
    pnpm --dir app test:coverage

test-scripts:
    node --test scripts/*.test.mjs
    @if command -v shellcheck >/dev/null; then shellcheck scripts/*.sh; else echo "shellcheck not installed; skipped (brew install shellcheck)"; fi

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

# Build, sign, notarize and staple the macOS release (on the release Mac, with HEAD at the tag).
release-mac *ARGS:
    scripts/release-macos.sh {{ARGS}}
