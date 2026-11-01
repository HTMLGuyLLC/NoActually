#!/usr/bin/env bash
# Finishes a draft GitHub release. scripts/start-release.sh creates the draft first (the Release
# workflow's token can't create one) and the workflow fills it with the Windows and Linux builds:
#   1. renames the CI-built files to plain names ("No-Actually-0.1.0-Windows-Setup-x64.exe")
#   2. uploads the notarized Mac DMG and the Mac update file from scripts/release-macos.sh
#   3. writes latest.json for in-app updates: the CI's Windows and Linux entries, pointed at the
#      renamed files, plus the Mac entries (see scripts/latest-json.mjs)
#   4. regenerates SHA256SUMS.txt from the exact files on the release
#
# It refuses to write a latest.json that leaves out a platform (Windows, Linux or Mac), so a missing
# build can't ship silently. --allow-missing writes it anyway.
#
# Usage: scripts/finish-release.sh v0.1.0 [--allow-missing]
set -euo pipefail

USAGE="usage: scripts/finish-release.sh vX.Y.Z [--allow-missing]"
ALLOW=()
POS=()
for arg in "$@"; do
  case "$arg" in
    --allow-missing) ALLOW=(--allow-missing) ;;
    -*) echo "unknown option '$arg'. $USAGE" >&2; exit 1 ;;
    *) POS+=("$arg") ;;
  esac
done
[[ ${#POS[@]} -eq 1 ]] || { echo "$USAGE" >&2; exit 1; }
TAG="${POS[0]}"
[[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "'$TAG' isn't a release tag like v1.2.3." >&2; exit 1; }
VERSION="${TAG#v}"
REPO="HTMLGuyLLC/NoActually"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DMG="$ROOT/target/universal-apple-darwin/release/bundle/dmg/No-Actually-${VERSION}-Mac-universal.dmg"
UPDATE="$ROOT/target/universal-apple-darwin/release/bundle/macos/No-Actually-${VERSION}-Mac-Update-universal.app.tar.gz"
P="No-Actually-${VERSION}"

WORK=""
trap '[[ -z "$WORK" ]] || rm -rf "$WORK"' EXIT

RID="$(gh api "repos/$REPO/releases" -q ".[] | select(.tag_name==\"$TAG\") | .id" | head -1)"
[[ -n "$RID" ]] || { echo "No release for $TAG yet. Run scripts/start-release.sh $TAG and let the Release workflow finish." >&2; exit 1; }

plain_name() { node "$ROOT/scripts/latest-json.mjs" plain-name "$1" "$VERSION"; }

echo "Renaming files…"
# tauri-action v1 also sets a display label (the original file name), and GitHub shows the
# label instead of the name, so set both.
gh api "repos/$REPO/releases/$RID/assets" --paginate -q '.[] | "\(.id)\t\(.name)\t\(.label // "")"' | while IFS=$'\t' read -r id name label; do
  new="$(plain_name "$name")"
  [[ "$new" == "$name" && ( -z "$label" || "$label" == "$new" ) ]] && continue
  gh api -X PATCH "repos/$REPO/releases/assets/$id" -f name="$new" -f label="$new" -q '"  " + .name'
done

if [[ -f "$DMG" ]]; then
  xcrun stapler validate "$DMG" >/dev/null || { echo "$DMG isn't notarized; run scripts/release-macos.sh first." >&2; exit 1; }
  echo "Uploading $(basename "$DMG")…"
  gh release upload "$TAG" -R "$REPO" "$DMG" --clobber
else
  echo "No Mac DMG at $DMG; run scripts/release-macos.sh to add one." >&2
fi
MAC_ARGS=()
if [[ -f "$UPDATE" && -f "$UPDATE.sig" ]]; then
  echo "Uploading $(basename "$UPDATE")…"
  gh release upload "$TAG" -R "$REPO" "$UPDATE" --clobber
  MAC_ARGS=(--mac-file "$UPDATE" --mac-sig "$UPDATE.sig")
else
  echo "No Mac update file at $UPDATE; installed Mac apps won't be offered this version, and latest.json is refused without --allow-missing." >&2
fi

echo "Writing latest.json…"
WORK="$(mktemp -d)"
IN_ARGS=()
# No latest.json yet is fine (Mac-only release, or CI didn't make one); any other failure isn't.
if dl_err="$(gh release download "$TAG" -R "$REPO" --pattern latest.json --dir "$WORK" 2>&1)"; then
  IN_ARGS=(--in "$WORK/latest.json")
elif [[ "$dl_err" == *"no assets match"* || "$dl_err" == *"not found"* ]]; then
  echo "No latest.json on the release yet; starting a new one." >&2
else
  echo "Couldn't download latest.json: $dl_err" >&2
  exit 1
fi
gh api "repos/$REPO/releases/$RID/assets" --paginate -q '.[] | "\(.id)\t\(.name)"' > "$WORK/assets.txt"
node "$ROOT/scripts/latest-json.mjs" finish --version "$VERSION" --tag "$TAG" --repo "$REPO" \
  --assets "$WORK/assets.txt" "${IN_ARGS[@]+"${IN_ARGS[@]}"}" "${MAC_ARGS[@]+"${MAC_ARGS[@]}"}" "${ALLOW[@]+"${ALLOW[@]}"}" > "$WORK/latest.out.json"
mv "$WORK/latest.out.json" "$WORK/latest.json"
gh release upload "$TAG" -R "$REPO" "$WORK/latest.json" --clobber
rm -rf "$WORK"

echo "Writing checksums…"
WORK="$(mktemp -d)"
gh release download "$TAG" -R "$REPO" --pattern "$P-*" --dir "$WORK"
(cd "$WORK" && shasum -a 256 "$P"-* | sort -k2 > SHA256SUMS.txt && shasum -a 256 -c SHA256SUMS.txt >/dev/null)
gh release upload "$TAG" -R "$REPO" "$WORK/SHA256SUMS.txt" --clobber
rm -rf "$WORK"

echo "Done. Review the draft, then publish it as a beta: scripts/publish-release.sh $TAG beta"
