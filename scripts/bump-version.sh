#!/usr/bin/env bash
# Sets the release version everywhere it lives:
#   - Cargo.toml          [workspace.package] version (the source of truth)
#   - app/package.json    version
#   - app/src-tauri/tauri.conf.json   version
#   - plugins/grade/.claude-plugin/plugin.json   the report card plugin's version
#   - Cargo.lock          refreshed for the new workspace version
#   - CHANGELOG.md        the Unreleased entries move under "## [X.Y.Z] - <today>", and the
#                         compare links at the bottom are updated
#
# Usage: scripts/bump-version.sh X.Y.Z
set -euo pipefail

NEW="${1:?usage: scripts/bump-version.sh X.Y.Z}"
# No -beta.1 style suffixes: a beta and the stable release share one version (publish-release.sh).
[[ "$NEW" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "'$NEW' isn't a version like 1.2.3 (no -beta suffix; beta and stable share a version)." >&2; exit 1; }
# Check the tools first, so a missing one can't leave the version half bumped.
for tool in cargo node; do
  command -v "$tool" >/dev/null || { echo "$tool isn't on PATH; nothing was changed." >&2; exit 1; }
done

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REPO_URL="https://github.com/HTMLGuyLLC/NoActually"
TODAY="$(date +%Y-%m-%d)"
TMP="$(mktemp)"
trap 'rm -f "$TMP"' EXIT

OLD="$("$ROOT/scripts/version.sh")"
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
    const re = /^(\s*"version"\s*:\s*)"[^"]*"/m;
    if (!re.test(text)) { console.error(file + ": version line not found"); process.exit(1); }
    const out = text.replace(re, `$1"${process.env.NEW}"`);
    fs.writeFileSync(file, out);
  ' "$1"
}
bump_json "$ROOT/app/package.json"
bump_json "$ROOT/app/src-tauri/tauri.conf.json"
bump_json "$ROOT/plugins/grade/.claude-plugin/plugin.json"

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

Done. Review the diff, then (main only takes pull requests):
  git switch -c release-$NEW && git commit -am "release: v$NEW"
  # open a pull request and merge it once CI is green, then wait for CI on the merged commit:
  git switch main && git pull
  scripts/start-release.sh v$NEW    # checks main is green, creates the draft, tags and pushes the tag
  # By hand instead: gh release create v$NEW --draft --target "\$(git rev-parse HEAD)" \\
  #   --title "No, Actually... v$NEW" --notes "See the changelog." && git tag v$NEW && git push origin v$NEW
  # The Release workflow then fills the draft with the Windows and Linux builds.
  scripts/release-macos.sh          # on the release Mac
  scripts/finish-release.sh v$NEW
EOF
