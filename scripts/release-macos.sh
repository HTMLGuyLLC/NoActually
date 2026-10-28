#!/usr/bin/env bash
# Builds a universal macOS app, signs it with Developer ID, notarizes it, and wraps it in a
# notarized DMG. Both the app and the DMG get Apple's ticket stapled, so they open cleanly
# even offline. It also makes the in-app update file (the stapled app as a .tar.gz) and signs it
# with the updater key.
#
# One-time setup, using an App Store Connect API key (Users and Access → Integrations →
# Team Keys, "Developer" role). The key's .p8 stays outside the repo:
#   mkdir -p ~/.appstoreconnect/private_keys && chmod 700 ~/.appstoreconnect
#   mv ~/Downloads/AuthKey_KEYID.p8 ~/.appstoreconnect/private_keys/ && chmod 600 ~/.appstoreconnect/private_keys/*
#   xcrun notarytool store-credentials noactually-notary --key ~/.appstoreconnect/private_keys/AuthKey_KEYID.p8 \
#     --key-id KEYID --issuer ISSUER_ID
#
# The updater key (from `pnpm tauri signer generate`) also stays outside the repo, at
# ~/.tauri/noactually.key, with its password in the login keychain:
#   security add-generic-password -s noactually-updater-key -a "$USER" -w
# NOACTUALLY_UPDATER_KEY (a key file) and TAURI_SIGNING_PRIVATE_KEY_PASSWORD override them.
#
# Usage: scripts/release-macos.sh [notary-profile]   (default: noactually-notary)
set -euo pipefail

PROFILE="${1:-noactually-notary}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/app"

IDENTITY="$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -1)"
[[ -n "$IDENTITY" ]] || { echo "No 'Developer ID Application' certificate in your keychain." >&2; exit 1; }
echo "Signing as: $IDENTITY"
export APPLE_SIGNING_IDENTITY="$IDENTITY"

# The bundler signs updater files too, so it needs the key before it starts.
KEY_FILE="${NOACTUALLY_UPDATER_KEY:-$HOME/.tauri/noactually.key}"
[[ -f "$KEY_FILE" ]] || { echo "No updater key at $KEY_FILE (see the top of this script)." >&2; exit 1; }
TAURI_SIGNING_PRIVATE_KEY="$(cat "$KEY_FILE")"
export TAURI_SIGNING_PRIVATE_KEY
unset TAURI_SIGNING_PRIVATE_KEY_PATH
if [[ -z "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD+set}" ]]; then
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(security find-generic-password -s noactually-updater-key -w)" \
    || { echo "No updater key password in the keychain (see the top of this script)." >&2; exit 1; }
fi
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD
VERSION="$(node -p "require('./src-tauri/tauri.conf.json').version")"
NAME="$(node -p "require('./src-tauri/tauri.conf.json').productName")"

BUNDLE="$ROOT/target/universal-apple-darwin/release/bundle"
notarize() { # file to submit, then files to staple
  local submit="$1"
  shift
  echo "Notarizing $(basename "$submit") (usually a few minutes)…"
  local out
  out="$(xcrun notarytool submit "$submit" --keychain-profile "$PROFILE" --wait 2>&1)" || true
  echo "$out"
  # Check the saved output; piping straight into `grep -q` would cut off notarytool mid-write.
  [[ "$out" == *"status: Accepted"* ]] \
    || { echo "Notarization was not accepted. Inspect with: xcrun notarytool log <id> --keychain-profile $PROFILE" >&2; exit 1; }
  for f in "$@"; do xcrun stapler staple "$f"; done
}

# 1. Build and sign the app only.
pnpm tauri build --target universal-apple-darwin --bundles app
APP="$(ls -d "$BUNDLE"/macos/*.app | head -1)"
codesign --verify --deep --strict --verbose=2 "$APP"

# 2. Notarize the app itself (zipped, as notarytool needs) and staple it.
ZIP="$(mktemp -d)/app.zip"
ditto -c -k --keepParent "$APP" "$ZIP"
notarize "$ZIP" "$APP"
rm -f "$ZIP"

# 3. The update file: the stapled app, tarred and signed. The bundler made one in step 1, but
#    from the app before it was stapled, so replace it. Plain name, as finish-release.sh expects.
rm -f "$BUNDLE"/macos/*.app.tar.gz "$BUNDLE"/macos/*.app.tar.gz.sig
UPDATE="$BUNDLE/macos/No-Actually-${VERSION}-Mac-Update.app.tar.gz"
COPYFILE_DISABLE=1 tar -C "$(dirname "$APP")" -czf "$UPDATE" "$(basename "$APP")"
pnpm -s tauri signer sign "$UPDATE" >/dev/null
[[ -s "$UPDATE.sig" ]] || { echo "Signing the update file failed." >&2; exit 1; }

# 4. Wrap the stapled app in a compressed DMG with an Applications shortcut. Built directly with
#    hdiutil: re-running Tauri's bundler would rebuild (and un-staple) the app.
mkdir -p "$BUNDLE/dmg"
# Plain file name people can read on the release page (see scripts/finish-release.sh).
DMG="$BUNDLE/dmg/No-Actually-${VERSION}-Mac.dmg"
STAGE="$(mktemp -d)"
ditto "$APP" "$STAGE/$(basename "$APP")"
ln -s /Applications "$STAGE/Applications"
rm -f "$DMG"
for attempt in 1 2 3; do
  # Spotlight sometimes holds the scratch image ("Resource busy"); try again after a pause.
  if hdiutil create -volname "$NAME" -srcfolder "$STAGE" -fs HFS+ -format UDZO -ov "$DMG" >/dev/null; then break; fi
  [[ $attempt == 3 ]] && { echo "Couldn't create the DMG." >&2; exit 1; }
  sleep 5
done
rm -rf "$STAGE"
codesign --sign "$IDENTITY" --timestamp "$DMG"

# 5. Notarize and staple the DMG too.
notarize "$DMG" "$DMG"

echo "Checking Gatekeeper…"
spctl --assess --type execute -v "$APP"
spctl --assess --type open --context context:primary-signature -v "$DMG"

echo
echo "Done: $DMG"
echo "      $UPDATE (+ .sig)"
