# EWITool2
EWITool2 is an open source controller and patch editor/librarian for the popular Akai EWI4000s wind synthesizer. Akai's own patch editor for the EWI4000s no longer runs on macOS (and perhaps other OSes) and there is no way to edit the instrument's sounds other than through software.

EWITool2 includes a fully graphical sound (patch) editor and patch library management. For more information see the [wiki](https://github.com/ericdano/EWITool2/wiki).

EWITool2 is the successor to EWItool, which was originally developed by [Steve Merrony](https://github.com/SMerrony), who not only created the program here, but more importantly decoded the MIDI format to be able to do this development at all. It's now being maintained here.

EWITool2 is written in Rust (EWItool was a Java/JavaFX application). It runs on macOS and Windows as a single small native app, with no Java runtime needed.

The latest version may always be downloaded [here](https://github.com/ericdano/EWITool2/releases).

## Development

Requires [Rust](https://rustup.rs). Everything lives in the `rust/` workspace:

```
cd rust
cargo run --bin ewitool2  # run the app
cargo test                # all tests (UI tests run headless)
```

## Building the macOS app

```
rust/scripts/build-macos.sh
```

This builds `rust/target/macos/EWITool2.app` and `EWITool2-<version>.dmg`. The app is universal (Apple Silicon and Intel) if the Intel target is installed (`rustup target add x86_64-apple-darwin`). It is signed with a Developer ID certificate if one is in the keychain (otherwise ad-hoc, which is fine on your own Mac), and notarized if `NOTARY_PROFILE` names a profile saved with `xcrun notarytool store-credentials`.

## Building the Windows app (on a Mac)

One-time setup:

```
rustup target add x86_64-pc-windows-gnu
brew install mingw-w64
```

Then:

```
rust/scripts/build-windows.sh
```

This builds `rust/target/windows/EWITool2-<version>-windows-x64.zip`, containing `EWITool2.exe` (with its icon and version details embedded), a short `README.txt` for Windows users and the licences. No installer is needed: unzip and run. The `.exe` can't run on macOS, so try it on a Windows PC before publishing.

## Releasing

Bump `version` in `rust/Cargo.toml` (the app shows it in Help ▸ About), then build as above.

### Changing icon

Replace `rust/ewitool-app/assets/icons/icon-source.png` (square, ideally 1024×1024) and run `python3 rust/scripts/make-icons.py` (needs Pillow: `pip install pillow`). It regenerates `logo.png` (window icon), `logo.icns` (macOS) and `logo.ico` (Windows), placing the artwork on macOS's standard rounded-square icon shape.

## Licence

EWITool2 is GPL-3.0-or-later (see `LICENSE`). It bundles the Inter font, licensed under the SIL Open Font License (`rust/ewitool-app/assets/fonts/Inter-LICENSE.txt`).
