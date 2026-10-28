#!/usr/bin/env bash
# Sets the release version everywhere it lives:
#   - Cargo.toml          [workspace.package] version (the source of truth)
#   - app/package.json    version
#   - app/src-tauri/tauri.conf.json   version, if that file still has one
#   - Cargo.lock          refreshed for the new workspace version
#   - CHANGELOG.md        the Unreleased entries move under "## [X.Y.Z] - <today>", and the
#                         compare links at the bottom are updated
#
# Usage: scripts/bump-version.sh X.Y.Z
set -euo pipefail

NEW="${1:?usage: scripts/bump-version.sh X.Y.Z}"
[[ "$NEW" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || { echo "'$NEW' isn't a version like 1.2.3" >&2; exit 1; }

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REPO_URL="https://github.com/HTMLGuyLLC/NoActually"
TODAY="$(date +%Y-%m-%d)"
TMP="$(mktemp)"
trap 'rm -f "$TMP"' EXIT

OLD="$(awk '/^\[workspace\.package\]/{f=1;next} /^\[/{f=0} f && /^version *=/{gsub(/[" ]/,"",$0); sub(/^version=/,"",$0); print; exit}' "$ROOT/Cargo.toml")"
[[ -n "$OLD" ]] || { echo "Couldn't read the current version from Cargo.toml." >&2; exit 1; }
[[ "$OLD" != "$NEW" ]] || { echo "Already at $NEW." >&2; exit 1; }
grep -q "^## \[$NEW\]" "$ROOT/CHANGELOG.md" && { echo "CHANGELOG.md already has a $NEW entry." >&2; exit 1; }
grep -q '^## \[Unreleased\]' "$ROOT/CHANGELOG.md" || { echo "CHANGELOG.md has no '## [Unreleased]' heading." >&2; exit 1; }
echo "Bumping $OLD -> $NEW"

# Cargo.toml: only the version line inside [workspace.package].
awk -v new="$NEW" '
  /^\[/ { inpkg = ($0 == "[workspace.package]") }
  inpkg && !done && /^version *=/ { print "version = \"" new "\""; done = 1; next }
  { print }
' "$ROOT/Cargo.toml" > "$TMP"
cat "$TMP" > "$ROOT/Cargo.toml"

# JSON files: change the first top-level "version" line in place so formatting stays untouched.
bump_json() {
  # shellcheck disable=SC2016  # the JS template literal is meant to stay literal for node
  NEW="$NEW" node -e '
    const fs = require("fs");
    const file = process.argv[1];
    const text = fs.readFileSync(file, "utf8");
    if (!("version" in JSON.parse(text))) { console.log("  " + file + ": no version key, skipped"); process.exit(0); }
    const re = /^(\s*"version"\s*:\s*)"[^"]*"/m;
    if (!re.test(text)) { console.error(file + ": version line not found"); process.exit(1); }
    const out = text.replace(re, `$1"${process.env.NEW}"`);
    fs.writeFileSync(file, out);
  ' "$1"
}
bump_json "$ROOT/app/package.json"
bump_json "$ROOT/app/src-tauri/tauri.conf.json"

(cd "$ROOT" && { cargo update -w --offline || cargo update -w; })

# CHANGELOG: new heading directly under Unreleased, so its entries now belong to $NEW.
awk -v new="$NEW" -v today="$TODAY" -v old="$OLD" -v url="$REPO_URL" '
  /^## \[Unreleased\]/ { print; print ""; print "## [" new "] - " today; next }
  /^\[Unreleased\]:/ {
    print "[Unreleased]: " url "/compare/v" new "...HEAD"
    print "[" new "]: " url "/compare/v" old "...v" new
    next
  }
  { print }
' "$ROOT/CHANGELOG.md" > "$TMP"
cat "$TMP" > "$ROOT/CHANGELOG.md"

cat <<EOF

Done. Review the diff, then:
  git commit -am "release: v$NEW"
  git tag v$NEW
  git push origin main v$NEW        # the Release workflow builds Windows and Linux into a draft
  scripts/release-macos.sh          # on the release Mac
  scripts/finish-release.sh v$NEW
EOF
