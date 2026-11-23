#!/bin/bash
# Build EWITool2.app and a .dmg for macOS from the Rust workspace.
#
#   rust/scripts/build-macos.sh
#
# - Universal (Apple Silicon + Intel) if the Intel target is installed
#   (`rustup target add x86_64-apple-darwin`), otherwise this Mac's CPU only.
# - Signed with a "Developer ID Application" certificate from the keychain
#   if there is one, otherwise ad-hoc (fine for your own Mac; other people
#   will see a security warning).
# - Notarized if NOTARY_PROFILE names a profile saved with
#   `xcrun notarytool store-credentials` (requires a Developer ID).
#
# Output: rust/target/macos/EWITool2.app and EWITool2-<version>.dmg
set -euo pipefail

cd "$(dirname "$0")/.."            # the rust/ workspace
OUT=target/macos
APP="$OUT/EWITool2.app"
BIN=ewitool2
# From Cargo.toml via `cargo metadata` (not `cargo pkgid`, which reads
# Cargo.lock and so reports the old version right after a bump).
META=$(cargo metadata --no-deps --format-version 1)   # stops here if the workspace is broken
VERSION=$(grep -o '"name":"ewitool-app","version":"[^"]*"' <<<"$META" | sed 's/.*"version":"//; s/"$//')
if [ -z "$VERSION" ]; then
  echo "Couldn't read the ewitool-app version from cargo metadata" >&2
  exit 1
fi
echo "Building EWITool2 $VERSION"

# --- compile ---
TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
INSTALLED=$(rustup target list --installed)
BUILT=()
for T in "${TARGETS[@]}"; do
  if grep -qx "$T" <<<"$INSTALLED"; then
    echo "Compiling for $T..."
    cargo build --release --bin "$BIN" --target "$T"
    BUILT+=("target/$T/release/$BIN")
  fi
done
if [ ${#BUILT[@]} -eq 0 ]; then
  echo "No macOS Rust targets installed (rustup target add aarch64-apple-darwin)" >&2
  exit 1
fi
if [ ${#BUILT[@]} -lt 2 ]; then
  echo "Note: building for this Mac's CPU only. For a universal app that also"
  echo "      runs on Intel Macs: rustup target add x86_64-apple-darwin"
fi

# --- assemble the .app ---
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
lipo -create "${BUILT[@]}" -output "$APP/Contents/MacOS/EWITool2"
cp ewitool-app/assets/icons/logo.icns "$APP/Contents/Resources/EWITool2.icns"
cp ewitool-app/assets/fonts/Inter-LICENSE.txt "$APP/Contents/Resources/"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>EWITool2</string>
  <key>CFBundleDisplayName</key><string>EWITool2</string>
  <key>CFBundleIdentifier</key><string>com.github.ericdano.ewitool2</string>
  <key>CFBundleExecutable</key><string>EWITool2</string>
  <key>CFBundleIconFile</key><string>EWITool2</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.music</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSHumanReadableCopyright</key><string>Copyright 2026 S.Merrony &amp; ledhed2222. GPL-3.0-or-later.</string>
</dict>
</plist>
PLIST

# --- sign ---
IDENTITY=$(security find-identity -v -p codesigning | grep "Developer ID Application" | head -1 | sed -n 's/.*"\(.*\)"/\1/p' || true)
if [ -n "$IDENTITY" ]; then
  echo "Signing with: $IDENTITY"
  codesign --force --sign "$IDENTITY" --timestamp --options runtime "$APP"
else
  echo "No Developer ID certificate found: signing ad-hoc (for this Mac)."
  codesign --force --sign - "$APP"
fi
codesign --verify --strict --verbose=2 "$APP"

# --- dmg ---
DMG="$OUT/EWITool2-$VERSION.dmg"
rm -f "$DMG"
STAGE=$(mktemp -d)
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"   # drag-to-install
hdiutil create -quiet -volname "EWITool2 $VERSION" -srcfolder "$STAGE" -ov -format UDZO "$DMG"
rm -rf "$STAGE"
if [ -n "$IDENTITY" ]; then
  codesign --force --sign "$IDENTITY" --timestamp "$DMG"
fi

# --- notarize (optional) ---
if [ -n "${NOTARY_PROFILE:-}" ] && [ -n "$IDENTITY" ]; then
  echo "Notarizing (profile $NOTARY_PROFILE)..."
  xcrun notarytool submit "$DMG" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple "$DMG"
fi

# --- check it starts ---
"$APP/Contents/MacOS/EWITool2" --smoke

echo
echo "Done:"
echo "  App: rust/$APP   ($(lipo -archs "$APP/Contents/MacOS/EWITool2"))"
echo "  DMG: rust/$DMG"
