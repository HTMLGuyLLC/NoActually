#!/usr/bin/env bash
# Publishes a finished release (see scripts/finish-release.sh) to one update channel:
#   beta    a prerelease: only apps on the Beta channel are offered it
#   stable  a full release, marked Latest: everyone is offered it
#
# Both copy the release's latest.json to the beta-channel release, where the Beta channel looks, so
# beta also gets full releases. A channel file for a newer version is left in place, so releasing an
# older beta to everyone can't take the newer one away.
#
# Usage: scripts/publish-release.sh vX.Y.Z beta|stable
set -euo pipefail

TAG="${1:?usage: scripts/publish-release.sh vX.Y.Z beta|stable}"
CHANNEL="${2:?usage: scripts/publish-release.sh vX.Y.Z beta|stable}"
REPO="HTMLGuyLLC/NoActually"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
HOLDER="beta-channel"
VERSION="${TAG#v}"

case "$CHANNEL" in
  beta | stable) ;;
  *) echo "'$CHANNEL' isn't a channel. Use beta or stable." >&2; exit 1 ;;
esac

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# A release with no update file would leave apps on the Beta channel looking at nothing.
mkdir "$WORK/new" "$WORK/old"
gh release download "$TAG" -R "$REPO" --pattern latest.json --dir "$WORK/new"
FOUND="$(node "$ROOT/scripts/latest-json.mjs" version "$WORK/new/latest.json")"
[[ "$FOUND" == "$VERSION" ]] || { echo "latest.json on $TAG says $FOUND." >&2; exit 1; }

if [[ "$CHANNEL" == beta ]]; then
  gh release edit "$TAG" -R "$REPO" --draft=false --prerelease --latest=false >/dev/null
else
  gh release edit "$TAG" -R "$REPO" --draft=false --prerelease=false --latest >/dev/null
fi
echo "$TAG is published to $CHANNEL."

if ! gh release view "$HOLDER" -R "$REPO" >/dev/null 2>&1; then
  gh release create "$HOLDER" -R "$REPO" --prerelease --latest=false --target main \
    --title "Beta channel" --notes "Update file for the app's Beta channel. Download releases from the versions below." >/dev/null
fi

if gh release download "$HOLDER" -R "$REPO" --pattern latest.json --dir "$WORK/old" 2>/dev/null; then
  OLD="$(node "$ROOT/scripts/latest-json.mjs" version "$WORK/old/latest.json")"
  # sort -V puts 0.6.0-beta.1 above 0.6.0, so versions must be plain X.Y.Z (bump-version.sh enforces it).
  NEWEST="$(printf '%s\n%s\n' "$OLD" "$VERSION" | sort -V | tail -1)"
  if [[ "$NEWEST" != "$VERSION" ]]; then
    echo "The Beta channel is on $OLD, which is newer, so it stays there."
    exit 0
  fi
fi
gh release upload "$HOLDER" -R "$REPO" "$WORK/new/latest.json" --clobber
echo "The Beta channel now offers $VERSION."
