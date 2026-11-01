#!/usr/bin/env bash
# Starts a release once the version bump is merged (CONTRIBUTING.md, "Each release", step 3):
#   1. checks HEAD is the tip of origin/main and its `ci-ok` check passed
#   2. checks the tag matches the Cargo version and doesn't exist yet
#   3. creates the draft release (the Release workflow's token can't, but it fills this one in)
#   4. tags HEAD and pushes the tag, which starts the Release workflow
#
# Usage: scripts/start-release.sh vX.Y.Z
set -euo pipefail

TAG="${1:?usage: scripts/start-release.sh vX.Y.Z}"
[[ $# -eq 1 ]] || { echo "usage: scripts/start-release.sh vX.Y.Z" >&2; exit 1; }
# No -beta suffixes: a beta and the stable release share one version (publish-release.sh).
[[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "'$TAG' isn't a release tag like v1.2.3 (no -beta suffix)." >&2; exit 1; }

REPO="HTMLGuyLLC/NoActually"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
for tool in git gh node; do
  command -v "$tool" >/dev/null || { echo "$tool isn't on PATH." >&2; exit 1; }
done

VERSION="$("$ROOT/scripts/version.sh")"
[[ "$TAG" == "v$VERSION" ]] || { echo "$TAG doesn't match the Cargo version $VERSION. Is the bump merged and pulled?" >&2; exit 1; }

git fetch --quiet --no-tags origin main
SHA="$(git rev-parse HEAD)"
[[ "$SHA" == "$(git rev-parse origin/main)" ]] || { echo "HEAD ($SHA) isn't the tip of origin/main. Run: git switch main && git pull" >&2; exit 1; }
git diff --quiet HEAD -- || { echo "The working tree has uncommitted changes." >&2; exit 1; }

if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null || git ls-remote --exit-code --tags origin "refs/tags/$TAG" >/dev/null 2>&1; then
  echo "The tag $TAG already exists. Release the next patch version rather than moving a tag." >&2
  exit 1
fi
if gh release view "$TAG" -R "$REPO" >/dev/null 2>&1; then
  echo "A release for $TAG already exists." >&2
  exit 1
fi

PASSED="$(gh api "repos/$REPO/commits/$SHA/check-runs?check_name=ci-ok&filter=latest" \
  --jq '[.check_runs[] | select(.conclusion == "success")] | length')"
[[ "$PASSED" -ge 1 ]] || { echo "No passing ci-ok check on $SHA yet. Wait for CI on the merged commit, then run this again." >&2; exit 1; }

echo "Creating the draft release $TAG on ${SHA:0:9}…"
gh release create "$TAG" -R "$REPO" --draft --target "$SHA" --title "No, Actually... $TAG" \
  --notes "See the [changelog](https://github.com/$REPO/blob/main/CHANGELOG.md)."
git tag "$TAG" "$SHA"
git push origin "$TAG"

echo "Pushed $TAG. The Release workflow now builds Windows and Linux into the draft."
echo "Next: scripts/release-macos.sh on the release Mac, then scripts/finish-release.sh $TAG"
