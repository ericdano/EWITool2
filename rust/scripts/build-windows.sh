#!/bin/bash
# Build EWITool2 for Windows (64-bit) on macOS, as a ready-to-share zip.
#
#   rust/scripts/build-windows.sh
#
# Needs, once:
#   rustup target add x86_64-pc-windows-gnu
#   brew install mingw-w64
#
# Output: rust/target/windows/EWITool2-<version>-windows-x64.zip containing
#   EWITool2/EWITool2.exe   the app (icon and version details embedded)
#   EWITool2/README.txt     how to run it on Windows
#   EWITool2/LICENSE.txt, Inter-LICENSE.txt
#
# The .exe can't run on macOS; try it on a Windows PC (or a Windows VM)
# before publishing.
set -euo pipefail

cd "$(dirname "$0")/.."            # the rust/ workspace
TARGET=x86_64-pc-windows-gnu
BIN=ewitool2
OUT=target/windows

# From Cargo.toml via `cargo metadata` (not `cargo pkgid`, which reads
# Cargo.lock and so reports the old version right after a bump).
META=$(cargo metadata --no-deps --format-version 1)   # stops here if the workspace is broken
VERSION=$(grep -o '"name":"ewitool-app","version":"[^"]*"' <<<"$META" | sed 's/.*"version":"//; s/"$//')
if [ -z "$VERSION" ]; then
  echo "Couldn't read the ewitool-app version from cargo metadata" >&2
  exit 1
fi
echo "Building EWITool2 $VERSION for Windows"

# --- check the tools ---
if ! rustup target list --installed | grep -qx "$TARGET"; then
  echo "Missing the Windows Rust target. Run: rustup target add $TARGET" >&2
  exit 1
fi
for tool in x86_64-w64-mingw32-gcc x86_64-w64-mingw32-windres; do
  if ! command -v "$tool" >/dev/null; then
    echo "Missing $tool. Run: brew install mingw-w64" >&2
    exit 1
  fi
done

# --- compile ---
cargo build --release --bin "$BIN" --target "$TARGET"
EXE="target/$TARGET/release/$BIN.exe"

# --- package ---
STAGE="$OUT/EWITool2"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp "$EXE" "$STAGE/EWITool2.exe"
cp ../LICENSE "$STAGE/LICENSE.txt"
cp ewitool-app/assets/fonts/Inter-LICENSE.txt "$STAGE/Inter-LICENSE.txt"
# Windows line endings so Notepad shows it properly everywhere.
sed 's/$/\r/' > "$STAGE/README.txt" <<TXT
EWITool2 $VERSION
Patch editor and librarian for the Akai EWI4000s.

Running it
----------
Double-click EWITool2.exe. No installation is needed; you can keep this
folder anywhere (for example in Documents) and pin EWITool2.exe to the
taskbar or Start menu.

The first time, Windows may show "Windows protected your PC", because the
app isn't signed with a paid certificate. Click "More info", then
"Run anyway". Windows remembers this.

Connecting the EWI
------------------
Choose MIDI > Ports... and select your EWI's input and output. The status
bar shows "EWI4000s" once it answers.

Windows usually lets only one program at a time use a MIDI port. If the
port won't open, close any other music software that uses the EWI and try
again.

Settings are saved in %APPDATA%\\ericdano\\EWITool2\\config (copied from
the original EWItool's %APPDATA%\\ledhed2222\\EWItool\\config the first time).

Licence
-------
EWITool2 is free software under the GNU GPL, version 3 or later
(LICENSE.txt). It includes the Inter font (Inter-LICENSE.txt).
TXT

ZIP="$OUT/EWITool2-$VERSION-windows-x64.zip"
rm -f "$ZIP"
(cd "$OUT" && zip -q -r -X "$(basename "$ZIP")" EWITool2)

echo
echo "Done:"
echo "  EXE: rust/$STAGE/EWITool2.exe  ($(du -h "$STAGE/EWITool2.exe" | cut -f1))"
echo "  ZIP: rust/$ZIP"
