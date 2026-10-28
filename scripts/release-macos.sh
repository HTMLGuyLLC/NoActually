#!/usr/bin/env bash
# Builds, signs and notarizes the universal macOS app and DMG on a maintainer's Mac.
#
# One-time setup (stores an app-specific password in your keychain; you type it, not this script):
#   xcrun notarytool store-credentials noactually-notary --apple-id YOU@EXAMPLE.COM --team-id TEAMID
#
# Usage: scripts/release-macos.sh [notary-profile]   (default profile: noactually-notary)
set -euo pipefail

PROFILE="${1:-noactually-notary}"
cd "$(dirname "$0")/../app"

IDENTITY="$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -1)"
if [[ -z "$IDENTITY" ]]; then
  echo "No 'Developer ID Application' certificate in your keychain." >&2
  exit 1
fi
echo "Signing as: $IDENTITY"

# Tauri signs the app and DMG with the hardened runtime when APPLE_SIGNING_IDENTITY is set.
APPLE_SIGNING_IDENTITY="$IDENTITY" pnpm tauri build --target universal-apple-darwin

BUNDLE="../target/universal-apple-darwin/release/bundle"
APP="$(ls -d "$BUNDLE"/macos/*.app | head -1)"
DMG="$(ls "$BUNDLE"/dmg/*.dmg | head -1)"

echo "Verifying signature…"
codesign --verify --deep --strict --verbose=2 "$APP"

echo "Notarizing $DMG (usually a few minutes)…"
xcrun notarytool submit "$DMG" --keychain-profile "$PROFILE" --wait
xcrun stapler staple "$DMG"
xcrun stapler staple "$APP"

echo "Checking Gatekeeper…"
spctl --assess --type open --context context:primary-signature -v "$DMG"
spctl --assess --type execute -v "$APP"

echo
echo "Done: $DMG"
