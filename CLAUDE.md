# EWITool2

EWITool2: Rust GUI patch editor/librarian for the Akai EWI4000s, talking to the instrument over MIDI sysex. Ported from a Java/JavaFX app. Everything lives in the `rust/` Cargo workspace.

## Commands (run in `rust/`)

- `cargo run --bin ewitool2` - run the GUI
- `cargo test` - all tests; UI tests drive the app headless via egui_kittest
- `EWITOOL_SCREENSHOTS=<dir> cargo test -p ewitool-app --test ui` - also save a PNG of each screen
- `cargo fmt` / `cargo clippy --all-targets` - keep both clean
- `scripts/build-macos.sh` - build `target/macos/EWITool2.app` + `.dmg` (universal if `x86_64-apple-darwin` is installed; Developer ID signing if a cert is in the keychain, else ad-hoc; notarizes if `NOTARY_PROFILE` is set)
- `scripts/build-windows.sh` - cross-compile `target/windows/EWITool2-<version>-windows-x64.zip` on macOS (needs `rustup target add x86_64-pc-windows-gnu` + `brew install mingw-w64`; linker set in `rust/.cargo/config.toml`; `ewitool-app/build.rs` embeds icon + version info for Windows targets only). Untested on real Windows so far.
- `python3 scripts/make-icons.py` - regenerate `logo.png`/`.icns`/`.ico` in `ewitool-app/assets/icons/` from `icon-source.png` (needs Pillow)
- `cargo run --bin ewitool2 -- --fetch "<port>" <file.syx>` - back up all patches from the EWI without the GUI (read-only)
- `cargo run --bin ewitool2 -- --midi-check ["port text"]` - list MIDI ports; with text, send an identity request to matching Out ports and print replies
- `cargo run --release -p ewitool-synth --example render -- <file.syx> <out-dir> [note] [breath] [patch#...]` - render patches through the synth model to WAV (for comparing with EWI recordings)
- `cargo run --release --example calibrate -- list | selftest | probe | run ...` - calibrate the synth against the real EWI: plays notes over MIDI (CC2 breath), records its audio from an input, renders the model the same way, prints a comparison and writes WAVs + `results.csv`. `run` sends patches to the edit buffer only. Options at the top of `ewitool-app/examples/calibrate.rs`; `compare` re-scores the model against saved recordings. Test patches and fitting: `scripts/calibration/`.
- `cargo run --example store_roundtrip -- "<port>" <slot> [backup.syx]` - hardware check: store a patch back unchanged and verify (writes one slot)

## Layout

- `ewitool-core` (no GUI/MIDI, fully unit-tested): `patch` (one 206-byte sysex patch; byte offsets are load-bearing and checked at compile time and in `tests/patch.rs`), `patch_set` (`.syx` files, legacy `.bnk`/`.sqs` import), `bank` (the EWI's 100 slots), `scratchpad`, `edit` (Generate/Process operations), `sysex` (identity request, patch request/dump), `fetch` (paced Fetch All), `outgoing` (`SendQueue`: live NRPN edits, patches in Save/Edit mode, paced like the Java original).
- `ewitool-synth` (no GUI/audio device, unit-tested): software model of the EWI4000s voice for previewing patches offline. `tuning.rs` holds every patch-value → Hz/seconds/gain mapping, each marked **measured** or **guess**; calibrate there, not in the DSP code. `dsp.rs` (oscillator, SVF, delay line), `fx.rs` (chorus/delay/reverb), `lib.rs` (`Synth`, `Phrase`, `render_phrase`).
- `ewitool-app`: `app.rs` (window, tabs, dialogs, MIDI wiring), `audio.rs` (`Audition`: cpal output opened on first preview; `Audition::silent()` in tests), `editor.rs` (Patch Editor panels + flow layout), `midi.rs` (midir ports/connections, CLI tools), `native_menu.rs` (macOS-only native menu bar via muda; installed from `main.rs`, so tests use the in-window `menu_bar`; both emit `app::Command`), `prefs.rs` (key=value prefs file in the OS config dir), `theme.rs` (palettes, Inter font, cards, `Look` from File ▸ Preferences).
- Branding: the name is `app::APP_NAME` ("EWITool2"); the binary is `ewitool2`. Prefs live in the OS config folder for `com.github`/`ericdano`/`EWITool2` (macOS: `~/Library/Application Support/com.github.ericdano.EWITool2/prefs.txt`); on first start they are copied from the original EWItool's `ledhed2222`/`EWItool` folder (`prefs::migrate`), which is left in place for the Java app. The macOS bundle ID is `com.github.ericdano.ewitool2`.
- Version lives only in `rust/Cargo.toml` (`[workspace.package]`); the app reads it via `CARGO_PKG_VERSION`.

## Gotchas

- Patch numbering: users see 0-99, the device stores user #0 as internal 99 (`patch::user_to_internal`).
- The app only sends to the EWI after an EWI4000s has answered the identity request on the chosen ports; offline it edits its own copy.
- MIDI ports are matched by label, with duplicate names numbered " #2", " #3" (macOS calls every unnamed external device "New external device").
- egui sliders clamp out-of-range values just by being displayed unless `SliderClamping::Edits` is set; patches from the EWI can hold out-of-range values.
- The native macOS menu binds only ⌘, ⌘S and the standard app/window keys; ⌘Z/⌘C stay with egui, because a native item bound to them would steal them from text boxes.
- Tests must not touch real MIDI or audio devices (`App::with_library` uses a silent `Audition`). The kittest harness's `click()` is an accessibility action, not a mouse click; use the `double_click` helper in `tests/ui.rs` for double-clicks.
- The Inter font (`ewitool-app/assets/fonts/`) is SIL Open Font License; `Inter-LICENSE.txt` must ship with it (the macOS build copies it into the app).
