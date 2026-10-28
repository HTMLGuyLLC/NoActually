#!/usr/bin/env bash
# The app version is the `version` in Cargo.toml's [workspace.package], the source of truth.
# CI, the release workflow, the justfile and bump-version.sh all read it through this script.
#
# Usage:
#   scripts/version.sh          print the version
#   scripts/version.sh check    fail unless app/package.json and tauri.conf.json agree with it
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
v="$(awk '/^\[workspace\.package\]/{f=1;next} /^\[/{f=0} f && /^version *=/{gsub(/[" ]/,"",$0); sub(/^version=/,"",$0); print; exit}' "$ROOT/Cargo.toml")"
[[ -n "$v" ]] || { echo "Couldn't read the version from Cargo.toml [workspace.package]." >&2; exit 1; }

case "${1:-}" in
  "") echo "$v" ;;
  check)
    pkg="$(node -p "require('$ROOT/app/package.json').version")"
    # tauri.conf.json may leave the version out; then Tauri uses Cargo's.
    conf="$(node -p "require('$ROOT/app/src-tauri/tauri.conf.json').version || ''")"
    echo "Cargo workspace: $v, app/package.json: $pkg, tauri.conf.json: ${conf:-<unset>}"
    [[ "$pkg" == "$v" ]] || { echo "app/package.json version differs." >&2; exit 1; }
    [[ -z "$conf" || "$conf" == "$v" ]] || { echo "tauri.conf.json version differs." >&2; exit 1; }
    ;;
  *) echo "usage: scripts/version.sh [check]" >&2; exit 2 ;;
esac
