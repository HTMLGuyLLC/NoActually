#!/usr/bin/env bash
# Finishes a draft GitHub release made by the Release workflow:
#   1. renames the CI-built files to plain names ("No-Actually-0.1.0-Windows-Setup.exe")
#   2. uploads the notarized Mac DMG from scripts/release-macos.sh
#   3. regenerates SHA256SUMS.txt from the exact files on the release
#
# Usage: scripts/finish-release.sh v0.1.0
set -euo pipefail

TAG="${1:?usage: scripts/finish-release.sh vX.Y.Z}"
VERSION="${TAG#v}"
REPO="HTMLGuyLLC/NoActually"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DMG="$ROOT/target/universal-apple-darwin/release/bundle/dmg/No-Actually-${VERSION}-Mac.dmg"
P="No-Actually-${VERSION}"

RID="$(gh api "repos/$REPO/releases" -q ".[] | select(.tag_name==\"$TAG\") | .id" | head -1)"
[[ -n "$RID" ]] || { echo "No release for $TAG yet. Push the tag and let the Release workflow finish." >&2; exit 1; }

plain_name() {
  case "$1" in
    *x64-setup.exe) echo "$P-Windows-Setup.exe" ;;
    *x64_en-US.msi) echo "$P-Windows-IT-Installer.msi" ;;
    *amd64.AppImage) echo "$P-Linux.AppImage" ;;
    *amd64.deb) echo "$P-Linux-Ubuntu-Debian.deb" ;;
    *x86_64.rpm) echo "$P-Linux-Fedora.rpm" ;;
    *universal.dmg) echo "$P-Mac.dmg" ;;
    *) echo "$1" ;;
  esac
}

echo "Renaming files…"
gh api "repos/$REPO/releases/$RID/assets" -q '.[] | "\(.id)\t\(.name)"' | while IFS=$'\t' read -r id name; do
  new="$(plain_name "$name")"
  [[ "$new" == "$name" ]] || gh api -X PATCH "repos/$REPO/releases/assets/$id" -f name="$new" -q '"  " + .name'
done

if [[ -f "$DMG" ]]; then
  xcrun stapler validate "$DMG" >/dev/null || { echo "$DMG isn't notarized; run scripts/release-macos.sh first." >&2; exit 1; }
  echo "Uploading $(basename "$DMG")…"
  gh release upload "$TAG" -R "$REPO" "$DMG" --clobber
else
  echo "No Mac DMG at $DMG; run scripts/release-macos.sh to add one." >&2
fi

echo "Writing checksums…"
WORK="$(mktemp -d)"
gh release download "$TAG" -R "$REPO" --pattern "$P-*" --dir "$WORK"
(cd "$WORK" && shasum -a 256 "$P"-* | sort -k2 > SHA256SUMS.txt && shasum -a 256 -c SHA256SUMS.txt >/dev/null)
gh release upload "$TAG" -R "$REPO" "$WORK/SHA256SUMS.txt" --clobber
rm -rf "$WORK"

echo "Done. Review the draft, then publish: gh release edit $TAG -R $REPO --draft=false"
