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
# It refuses to run on a dirty tree, or when HEAD isn't the commit tagged v<version>, so the
# notarized build always matches the tag. --force skips the tag check (never the dirty check).
#
# Usage: scripts/release-macos.sh [--force] [notary-profile]   (default: noactually-notary)
set -euo pipefail

FORCE=0
ARGS=()
for a in "$@"; do
  if [[ "$a" == "--force" ]]; then FORCE=1; else ARGS+=("$a"); fi
done
PROFILE="${ARGS[0]:-noactually-notary}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# The Cargo workspace version is the source of truth (scripts/bump-version.sh keeps the rest in step).
VERSION="$(awk '/^\[workspace\.package\]/{f=1;next} /^\[/{f=0} f && /^version *=/{gsub(/[" ]/,"",$0); sub(/^version=/,"",$0); print; exit}' "$ROOT/Cargo.toml")"
[[ -n "$VERSION" ]] || { echo "Couldn't read the version from Cargo.toml." >&2; exit 1; }

if [[ -n "$(git -C "$ROOT" status --porcelain)" ]]; then
  echo "The working tree has uncommitted changes. Commit or stash them first." >&2
  exit 1
fi
if [[ "$FORCE" != 1 ]]; then
  TAG_COMMIT="$(git -C "$ROOT" rev-parse -q --verify "refs/tags/v$VERSION^{commit}" || true)"
  [[ -n "$TAG_COMMIT" ]] || { echo "No tag v$VERSION. Tag the release commit first (or pass --force)." >&2; exit 1; }
  [[ "$TAG_COMMIT" == "$(git -C "$ROOT" rev-parse HEAD)" ]] \
    || { echo "HEAD isn't at tag v$VERSION. Check it out first (or pass --force)." >&2; exit 1; }
fi

cd "$ROOT/app"
pnpm install --frozen-lockfile

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

# 1. Build and sign the app only. A universal build needs both Mac targets on the pinned toolchain.
(cd "$ROOT" && rustup target add aarch64-apple-darwin x86_64-apple-darwin)
pnpm tauri build --target universal-apple-darwin --bundles app
# shellcheck disable=SC2012  # one app bundle, plain names
APP="$(ls -d "$BUNDLE"/macos/*.app | head -1)"
codesign --verify --deep --strict --verbose=2 "$APP"

# 2. Notarize the app itself (zipped, as notarytool needs) and staple it.
TMP_DIRS=()
trap 'rm -rf "${TMP_DIRS[@]+"${TMP_DIRS[@]}"}"' EXIT
ZIP_DIR="$(mktemp -d)"
TMP_DIRS+=("$ZIP_DIR")
ZIP="$ZIP_DIR/app.zip"
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
TMP_DIRS+=("$STAGE")
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
