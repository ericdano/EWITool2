//! Main window: menus, tabs, status bar and dialogs (port of `UiMain`,
//! `UiStatusBar`, `ScratchPadTab`, `PatchSetsTab`, `CurrentPatchSetTab` and
//! the menu actions of `PatchEditorTab`).
//!
//! Everything that changes the EWI pushes onto `live_out` / `patch_out`.
//! At the end of each frame those go into a paced [`SendQueue`], but only
//! when an EWI4000s has answered on the chosen ports; otherwise they are
//! dropped and the app works offline on its own copy of the patches.

use std::fmt::Display;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Button, Color32, ComboBox, Key, KeyboardShortcut, Modifiers, RichText, Sense, Stroke, Ui,
};
use ewitool_core::fetch::{self, Fetcher};
use ewitool_core::outgoing::SendQueue;
use ewitool_core::sysex::{self, DeviceId};
use ewitool_core::{edit, patch_set, Bank, Mode, Patch, ScratchPad, NUM_PATCHES};

use crate::audio::{self, Audition};
use crate::editor::{self, LiveControl};
use crate::midi::{self, Midi};
use crate::prefs::Prefs;
use crate::theme;

pub const APP_NAME: &str = "EWITool2";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const COPYRIGHT: &str = "Copyright 2026 S.Merrony,ledhed2222,ericdano";
const ONLINE_HELP: &str = "https://github.com/ledhed2222/EWItool/wiki/Using-EWItool";
const STATUS_TIMEOUT: Duration = Duration::from_secs(10);
const PREVIEW_HELP: &str = "Hear this patch on the computer, using EWITool2's model of the \
     EWI4000s sound engine (an approximation; no EWI needed).";
const NEED_EWI: &str = "Choose your EWI's ports in MIDI ▸ Ports… first";
/// How long to wait for the EWI to answer an identity request (as Java).
const ID_TIMEOUT: Duration = Duration::from_secs(3);

/// What we know about the device on the chosen MIDI ports.
#[derive(Clone, PartialEq, Eq)]
enum EwiStatus {
    NotConnected,
    Checking(Instant),
    Ewi4000s,
    NoReply,
    Other(String),
}

const STORE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const REVERT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
const PREFERENCES: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Comma);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Scratchpad,
    Library,
    CurrentSet,
    Editor,
}

#[derive(Default)]
struct Library {
    sets: Vec<String>,
    selected_set: Option<usize>,
    set_patches: Vec<Patch>,
    selected_patch: Option<usize>,
}

/// Where the patch being edited came from, and where Store puts it back.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    /// A slot of the Current Patch Set (the EWI).
    Slot(usize),
    /// An entry in the Patch Scratchpad.
    Scratch(usize),
    /// A Patch Scratchpad entry that has since been deleted.
    Detached,
}

struct Editor {
    source: Source,
    patch: Patch,
    /// The patch as it was when opened or last stored, for Revert.
    unedited: Patch,
    /// The patch as it was when the user last answered "copy your edits to
    /// the Patch Scratchpad?", so they're only asked again after new edits.
    asked: Option<Patch>,
}

impl Editor {
    fn new(source: Source, patch: Patch) -> Self {
        Self {
            source,
            unedited: patch.clone(),
            patch,
            asked: None,
        }
    }

    fn has_unasked_edits(&self) -> bool {
        self.patch != self.unedited && self.asked.as_ref() != Some(&self.patch)
    }
}

/// Where to go once any unsaved edits have been dealt with.
#[derive(Clone, Copy)]
enum After {
    Tab(Tab),
    OpenSlot(usize),
    OpenScratch(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditOp {
    Store,
    Revert,
    CopyToScratchpad,
    Default,
    Random,
    MakeDry,
    MaxVolume,
    RemoveNoise,
    Randomise10,
    Merge,
}

/// Patch ▸ Generate items.
pub const GENERATE: [(&str, EditOp); 2] = [
    ("Default Patch", EditOp::Default),
    ("Random Patch", EditOp::Random),
];
/// Patch ▸ Process items.
pub const PROCESS: [(&str, EditOp); 5] = [
    ("Make Dry", EditOp::MakeDry),
    ("Maximise Volume", EditOp::MaxVolume),
    ("Remove Noise", EditOp::RemoveNoise),
    ("Randomise by 10%", EditOp::Randomise10),
    ("Merge with…", EditOp::Merge),
];

/// Everything the menus can do. Both the in-window menu bar and the native
/// macOS menu bar produce these, so they always behave the same.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Preferences,
    Quit,
    Ports,
    CheckEwi,
    FetchAll,
    Edit(EditOp),
    Theme(egui::ThemePreference),
    EditorSize(u32),
    OnlineHelp,
    About,
}

/// What the menus should show as enabled, checked or labelled right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuState {
    pub check_ewi: bool,
    pub fetch_all: bool,
    pub patch: bool,
    pub store_label: &'static str,
    pub theme: egui::ThemePreference,
    pub editor_size: u32,
}

pub const THEMES: [(egui::ThemePreference, &str); 3] = [
    (egui::ThemePreference::Dark, "Dark"),
    (egui::ThemePreference::Light, "Light"),
    (egui::ThemePreference::System, "Follow System"),
];

enum Dialog {
    Message {
        title: String,
        text: String,
    },
    Confirm {
        title: String,
        text: String,
        action: Confirm,
    },
    Input {
        title: String,
        prompt: String,
        value: String,
        action: Input,
    },
    Hex {
        title: String,
        hex: String,
    },
    PatchActions {
        slot: usize,
        name: String,
        paste: Option<usize>,
    },
    Merge {
        selected: Option<usize>,
    },
    About,
    UnsavedEdits {
        after: After,
    },
    Ports {
        ins: Vec<String>,
        outs: Vec<String>,
        sel_in: Option<usize>,
        sel_out: Option<usize>,
    },
}

#[derive(Clone, Copy)]
enum Confirm {
    ClearScratchpad,
    LoadSet,
    DeleteSet,
}

#[derive(Clone, Copy)]
enum Input {
    RenameScratch(usize),
    ExportScratch(usize),
    SaveBank,
}

/// What a dialog asked for when it closed.
enum Action {
    Confirmed(Confirm),
    Entered(Input, String),
    Edit(usize),
    CopyToScratchpad(usize),
    Paste {
        slot: usize,
        from: Option<usize>,
    },
    Rename {
        slot: usize,
        name: String,
    },
    MergeWith(usize),
    LeaveEditor {
        copy: bool,
        after: After,
    },
    SetPorts {
        midi_in: Option<String>,
        midi_out: Option<String>,
    },
    RefreshPorts,
}

pub struct App {
    prefs: Prefs,
    tab: Tab,
    scratch: ScratchPad,
    scratch_sel: Option<usize>,
    library: Library,
    /// The Current Patch Set (the EWI's 100 slots), once loaded.
    bank: Option<Bank>,
    editor: Option<Editor>,
    dialog: Option<Dialog>,
    status: Option<(String, Instant)>,
    /// Parameter changes for the EWI. Nothing sends these yet.
    live_out: Vec<LiveControl>,
    /// Whole patches for the EWI. Nothing sends these yet.
    patch_out: Vec<(Patch, Mode)>,
    rng: fastrand::Rng,
    midi: Midi,
    ewi: EwiStatus,
    /// (Re)open the MIDI ports chosen in the preferences on the next frame.
    reconnect: bool,
    /// A "Fetch All Patches" in progress.
    fetch: Option<Fetcher>,
    /// Messages waiting to go to the EWI.
    send: SendQueue,
    /// Total patch messages in a "Load into EWI" in progress.
    bulk_total: Option<usize>,
    /// The saved theme has been applied (needs the context, so first frame).
    theme_applied: bool,
    /// The native macOS menu bar, when the real app (not a test) runs.
    #[cfg(target_os = "macos")]
    native_menu: Option<crate::native_menu::NativeMenu>,
    /// The Preferences window is showing.
    prefs_open: bool,
    /// Plays patches on the computer's speakers.
    audio: Audition,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    /// The app with the user's saved preferences.
    /// Use the native macOS menu bar instead of the in-window menus. Call
    /// once, on the main thread, from eframe's creation callback.
    #[cfg(target_os = "macos")]
    pub fn install_native_menu(&mut self, ctx: &egui::Context) {
        self.native_menu = Some(crate::native_menu::NativeMenu::install(ctx));
    }

    fn has_native_menu(&self) -> bool {
        #[cfg(target_os = "macos")]
        return self.native_menu.is_some();
        #[cfg(not(target_os = "macos"))]
        return false;
    }

    pub fn new() -> Self {
        Self::with_prefs(Prefs::load())
    }

    /// The app with a given library folder and no saved preferences
    /// (for tests).
    pub fn with_library(library: Option<PathBuf>) -> Self {
        let mut app = Self::with_prefs(Prefs::in_memory(library));
        app.audio = Audition::silent();
        app
    }

    /// What patch previews would play (for tests, which never open audio).
    pub fn previewing(&self) -> Option<(Patch, bool)> {
        self.audio
            .current_patch()
            .map(|p| (p, self.audio.is_playing()))
    }

    fn with_prefs(prefs: Prefs) -> Self {
        let mut app = Self {
            scratch: ScratchPad::in_memory(),
            prefs,
            tab: Tab::Scratchpad,
            scratch_sel: None,
            library: Library::default(),
            bank: None,
            editor: None,
            dialog: None,
            status: None,
            live_out: Vec::new(),
            patch_out: Vec::new(),
            rng: fastrand::Rng::new(),
            midi: Midi::default(),
            ewi: EwiStatus::NotConnected,
            reconnect: false,
            fetch: None,
            send: SendQueue::default(),
            bulk_total: None,
            theme_applied: false,
            prefs_open: false,
            audio: Audition::new(),
            #[cfg(target_os = "macos")]
            native_menu: None,
        };
        // Reopen the ports used last time, like the Java version.
        app.reconnect = app.prefs.midi_in().is_some() || app.prefs.midi_out().is_some();
        app.load_scratchpad();
        app.refresh_library();
        app
    }

    // ---- helpers ----

    /// Play `patch` once on the computer.
    fn preview(&mut self, patch: &Patch) {
        self.audio.play(patch, false);
        self.audio_error();
    }

    fn audio_error(&mut self) {
        if let Some(e) = self.audio.error.take() {
            self.message("Preview", e);
        }
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now()));
    }

    fn message(&mut self, title: &str, text: impl Display) {
        self.dialog = Some(Dialog::Message {
            title: title.into(),
            text: text.to_string(),
        });
    }

    fn load_scratchpad(&mut self) {
        self.scratch_sel = None;
        match ScratchPad::load(self.prefs.library()) {
            Ok(sp) => self.scratch = sp,
            Err(e) => {
                self.scratch = ScratchPad::in_memory();
                self.message(
                    "Patch Scratchpad",
                    format!("Could not load the Patch Scratchpad: {e}"),
                );
            }
        }
    }

    fn add_to_scratchpad(&mut self, patch: Patch) {
        let name = patch.name();
        match self.scratch.add(patch) {
            Ok(()) => self.set_status(format!("{name} added to the Patch Scratchpad")),
            Err(e) => self.scratch_save_error(e),
        }
    }

    fn scratch_save_error(&mut self, e: ewitool_core::Error) {
        self.message(
            "Patch Scratchpad",
            format!("Could not save the Patch Scratchpad: {e}"),
        );
    }

    /// Re-read the library folder's file list, keeping the selected set
    /// selected if it is still there.
    fn refresh_library(&mut self) {
        let keep = self.selected_set_name();
        self.library = Library::default();
        let Some(dir) = self.prefs.library() else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut sets: Vec<String> = entries
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.to_lowercase().ends_with(".syx"))
            .collect();
        sets.sort_by_key(|n| n.to_lowercase());
        self.library.sets = sets;
        if let Some(name) = keep {
            self.select_set_named(&name);
        }
    }

    /// Select (and show the patches of) the set with this file name.
    fn select_set_named(&mut self, name: &str) -> bool {
        match self.library.sets.iter().position(|n| n == name) {
            Some(i) => {
                self.select_set(i);
                true
            }
            None => false,
        }
    }

    /// Ask whether to load the selected set as the Current Patch Set
    /// (`intro` is shown first, e.g. after an import).
    fn confirm_load_set(&mut self, intro: &str) {
        let set_name = self.selected_set_name().unwrap_or_default();
        let question = if self.ewi_ready() {
            format!(
                "This will overwrite all the patches in the EWI with \"{set_name}\". \
                 Do you really want to do this?"
            )
        } else {
            format!(
                "Load \"{set_name}\" as the Current Patch Set? (Not connected to an \
                 EWI4000s, so nothing is sent to the EWI.)"
            )
        };
        self.dialog = Some(Dialog::Confirm {
            title: "Load Patch Set".into(),
            text: if intro.is_empty() {
                question
            } else {
                format!("{intro}\n\n{question}")
            },
            action: Confirm::LoadSet,
        });
    }

    fn select_set(&mut self, ix: usize) {
        let lib = &mut self.library;
        lib.selected_set = Some(ix);
        lib.selected_patch = None;
        lib.set_patches.clear();
        let (Some(dir), Some(name)) = (self.prefs.library(), lib.sets.get(ix)) else {
            return;
        };
        match patch_set::load_syx(&dir.join(name)) {
            Ok(patches) => lib.set_patches = patches,
            Err(e) => {
                let text = format!("Could not read {name}: {e}");
                self.message("Patch Set Library", text);
            }
        }
    }

    fn selected_set_name(&self) -> Option<String> {
        self.library.sets.get(self.library.selected_set?).cloned()
    }

    fn choose_library(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title("Choose EWI Patch Set Library location…");
        if let Some(dir) = self.prefs.library() {
            dialog = dialog.set_directory(dir);
        }
        let Some(dir) = dialog.pick_folder() else {
            return;
        };
        if let Err(e) = self.prefs.set_library(dir) {
            self.message("Preferences", format!("Could not save preferences: {e}"));
        }
        self.load_scratchpad();
        self.refresh_library();
    }

    fn import_legacy(&mut self) {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Choose Soundbank to Import…")
            .add_filter(
                "Legacy Soundbanks (.bnk, .sqs)",
                &["bnk", "sqs", "BNK", "SQS"],
            );
        if let Some(dir) = self.prefs.library() {
            dialog = dialog.set_directory(dir);
        }
        let Some(file) = dialog.pick_file() else {
            return;
        };
        // Import into the library when there is one, else next to the file.
        let out_dir = self
            .prefs
            .library()
            .map(PathBuf::from)
            .or_else(|| file.parent().map(PathBuf::from))
            .unwrap_or_default();
        match patch_set::convert_legacy_to(&file, &out_dir) {
            Ok((syx, n)) => {
                let name = syx
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                self.refresh_library();
                let intro = format!("Imported {n} patches into \"{name}\".");
                if self
                    .prefs
                    .library()
                    .is_some_and(|lib| syx.parent() == Some(lib))
                    && self.select_set_named(&name)
                {
                    self.confirm_load_set(&intro);
                } else {
                    self.message(
                        "Import",
                        format!("{intro} It was saved next to the original file."),
                    );
                }
            }
            Err(e) => self.message("Import Error", e),
        }
    }

    /// Go somewhere that would leave the editor or replace its patch,
    /// first offering to copy unsaved edits to the Patch Scratchpad.
    fn navigate(&mut self, after: After) {
        let leaving = match after {
            After::Tab(tab) => self.tab == Tab::Editor && tab != Tab::Editor,
            After::OpenSlot(_) | After::OpenScratch(_) => true,
        };
        if leaving && self.editor.as_ref().is_some_and(Editor::has_unasked_edits) {
            self.dialog = Some(Dialog::UnsavedEdits { after });
        } else {
            self.go(after);
        }
    }

    fn go(&mut self, after: After) {
        match after {
            After::Tab(tab) => self.tab = tab,
            After::OpenSlot(slot) => self.open_slot_editor(slot),
            After::OpenScratch(ix) => self.open_scratch_editor(ix),
        }
    }

    fn open_slot_editor(&mut self, slot: usize) {
        let Some(patch) = self.bank.as_ref().and_then(|b| b.get(slot)).cloned() else {
            self.set_status(format!(
                "Patch #{slot} is empty; paste a patch into it first"
            ));
            return;
        };
        self.patch_out.push((patch.clone(), Mode::Edit));
        self.editor = Some(Editor::new(Source::Slot(slot), patch));
        self.tab = Tab::Editor;
    }

    fn open_scratch_editor(&mut self, ix: usize) {
        let Some(patch) = self.scratch.patches().get(ix).cloned() else {
            return;
        };
        // Loads it into the EWI's edit buffer only; nothing is stored.
        self.patch_out.push((patch.clone(), Mode::Edit));
        self.editor = Some(Editor::new(Source::Scratch(ix), patch));
        self.tab = Tab::Editor;
    }

    /// Keep the editor's Patch Scratchpad index right after entry `removed`
    /// is deleted (`None`: the whole Patch Scratchpad was cleared).
    fn scratch_entry_removed(&mut self, removed: Option<usize>) {
        if let Some(ed) = &mut self.editor {
            if let Source::Scratch(i) = ed.source {
                ed.source = match removed {
                    Some(r) if r < i => Source::Scratch(i - 1),
                    Some(r) if r > i => Source::Scratch(i),
                    _ => Source::Detached,
                };
            }
        }
    }

    /// Drop the editor if it was editing a slot of the old Current Patch
    /// Set (a Patch Scratchpad patch stays open).
    fn bank_replaced(&mut self) {
        if self
            .editor
            .as_ref()
            .is_some_and(|e| matches!(e.source, Source::Slot(_)))
        {
            self.editor = None;
            if self.tab == Tab::Editor {
                self.tab = Tab::CurrentSet;
            }
        }
    }

    fn store_label(&self) -> &'static str {
        match self.editor.as_ref().map(|e| e.source) {
            Some(Source::Scratch(_)) => "Save to Patch Scratchpad",
            _ => "Store to EWI",
        }
    }

    fn editing(&self) -> bool {
        self.tab == Tab::Editor && self.editor.is_some()
    }

    fn apply_op(&mut self, op: EditOp) {
        let Some(ed) = self.editor.as_mut() else {
            return;
        };
        match op {
            EditOp::Store => {
                match ed.source {
                    Source::Slot(slot) => {
                        if let Some(bank) = &mut self.bank {
                            bank.set(slot, ed.patch.clone());
                            ed.patch = bank.get(slot).unwrap().clone();
                        }
                        ed.unedited = ed.patch.clone();
                        self.patch_out.push((ed.patch.clone(), Mode::Save));
                        let msg = if self.ewi == EwiStatus::Ewi4000s {
                            format!("Patch #{slot} stored in the EWI")
                        } else {
                            format!("Patch #{slot} stored in {APP_NAME} only (EWI not connected)")
                        };
                        self.set_status(msg);
                    }
                    Source::Scratch(ix) => {
                        let patch = ed.patch.clone();
                        ed.unedited = patch.clone();
                        let name = patch.name();
                        match self.scratch.replace(ix, patch) {
                            Ok(()) => {
                                self.set_status(format!("{name} saved to the Patch Scratchpad"))
                            }
                            Err(e) => self.scratch_save_error(e),
                        }
                    }
                    Source::Detached => self.set_status(
                        "This patch was deleted from the Patch Scratchpad; \
                         use Copy to Patch Scratchpad to keep it",
                    ),
                }
                return;
            }
            EditOp::Revert => ed.patch = ed.unedited.clone(),
            EditOp::CopyToScratchpad => {
                let patch = ed.patch.clone();
                self.add_to_scratchpad(patch);
                return;
            }
            EditOp::Default => edit::default_patch(&mut ed.patch),
            EditOp::Random => edit::random_patch(&mut ed.patch, &mut self.rng),
            EditOp::MakeDry => edit::make_dry(&mut ed.patch),
            EditOp::MaxVolume => edit::maximise_volume(&mut ed.patch),
            EditOp::RemoveNoise => edit::remove_noise(&mut ed.patch),
            EditOp::Randomise10 => edit::randomise_by_10pct(&mut ed.patch, &mut self.rng),
            EditOp::Merge => {
                self.dialog = Some(Dialog::Merge { selected: None });
                return;
            }
        }
        // Whole-patch changes go to the EWI's edit buffer in one message.
        self.patch_out.push((ed.patch.clone(), Mode::Edit));
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.dialog.is_none() && ctx.input_mut(|i| i.consume_shortcut(&PREFERENCES)) {
            self.prefs_open = true;
        }
        if !self.editing() || self.dialog.is_some() {
            return;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&STORE)) {
            self.apply_op(EditOp::Store);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&REVERT)) {
            self.apply_op(EditOp::Revert);
        }
        // Cmd/Ctrl+C arrives as a Copy event rather than a key press.
        let copy = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)));
        if copy && !ctx.egui_wants_keyboard_input() {
            self.apply_op(EditOp::CopyToScratchpad);
        }
    }

    // ---- top-level layout ----

    // ---- MIDI ----

    fn open_ports_dialog(&mut self) {
        match midi::list_ports() {
            Ok((ins, outs)) => {
                let find = |list: &[String], name: Option<&str>| {
                    name.and_then(|n| list.iter().position(|p| p == n))
                };
                let sel_in = find(&ins, self.prefs.midi_in());
                let sel_out = find(&outs, self.prefs.midi_out());
                self.dialog = Some(Dialog::Ports {
                    ins,
                    outs,
                    sel_in,
                    sel_out,
                });
            }
            Err(e) => self.message("MIDI", format!("Could not list MIDI ports: {e}")),
        }
    }

    /// Open the ports named in the preferences, then look for the EWI.
    fn connect_midi(&mut self, ctx: &egui::Context) {
        self.midi = Midi::default(); // close the old ports first
        self.ewi = EwiStatus::NotConnected;
        let (midi, errors) = Midi::connect(self.prefs.midi_in(), self.prefs.midi_out(), ctx);
        self.midi = midi;
        if !errors.is_empty() {
            self.message("MIDI", format!("Could not open:\n{}", errors.join("\n")));
        }
        if self.midi.is_connected() {
            self.start_id_check();
        }
    }

    /// Ask the device to identify itself; the reply is handled in `poll_midi`.
    fn start_id_check(&mut self) {
        match self.midi.send(&sysex::DEVICE_ID_REQUEST) {
            Ok(()) => self.ewi = EwiStatus::Checking(Instant::now()),
            Err(e) => {
                self.ewi = EwiStatus::NotConnected;
                self.set_status(format!("Could not send to MIDI Out: {e}"));
            }
        }
    }

    fn poll_midi(&mut self, ctx: &egui::Context) {
        for msg in self.midi.take_received() {
            if let Some(f) = &mut self.fetch {
                if f.receive(&msg) {
                    continue;
                }
            }
            if let Some(id) = sysex::parse_device_id(&msg) {
                self.ewi = match id {
                    DeviceId::Ewi4000s => EwiStatus::Ewi4000s,
                    DeviceId::OtherAkai(f) => EwiStatus::Other(format!("Akai device {f:#04x}")),
                    DeviceId::NotAkai(m) => EwiStatus::Other(format!("manufacturer {m:#04x}")),
                };
                if self.ewi == EwiStatus::Ewi4000s {
                    self.set_status("EWI4000s found");
                }
            }
        }
        self.drive_fetch(ctx);
        if let EwiStatus::Checking(since) = self.ewi {
            if since.elapsed() > ID_TIMEOUT {
                self.ewi = EwiStatus::NoReply;
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
    }

    /// Send the next fetch request, or finish the fetch.
    fn drive_fetch(&mut self, ctx: &egui::Context) {
        let Some(f) = &mut self.fetch else { return };
        match f.poll(Instant::now()) {
            fetch::Step::Send(bytes) => {
                if let Err(e) = self.midi.send(&bytes) {
                    self.fetch = None;
                    self.message(
                        "Fetch All Patches",
                        format!("Could not send to the EWI: {e}"),
                    );
                    return;
                }
                ctx.request_repaint_after(Duration::from_millis(20));
            }
            fetch::Step::Wait => ctx.request_repaint_after(Duration::from_millis(20)),
            fetch::Step::Done => {
                let bank = self.fetch.take().unwrap().into_bank();
                self.bank = Some(bank);
                self.bank_replaced();
                self.tab = Tab::CurrentSet;
                self.set_status("Fetched 100 patches from the EWI");
            }
            fetch::Step::Failed(why) => {
                self.fetch = None;
                self.message(
                    "Fetch All Patches",
                    format!("{why}\n\nCheck the EWI is switched on and connected, then try again."),
                );
            }
        }
    }

    /// Progress window shown while fetching.
    fn show_fetch_progress(&mut self, ctx: &egui::Context) {
        let Some(f) = &self.fetch else { return };
        let done = f.received();
        let mut cancel = false;
        egui::Modal::new(egui::Id::unique("fetch")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.heading("Fetching all patches");
            ui.label(format!("{done} of 100"));
            ui.add(egui::ProgressBar::new(done as f32 / NUM_PATCHES as f32));
            ui.add_space(8.0);
            cancel = ui.button("Cancel").clicked();
        });
        if cancel {
            self.fetch = None;
            self.set_status("Fetch cancelled; the Current Patch Set is unchanged");
        }
    }

    /// True when an EWI4000s has answered on the chosen ports, so it is
    /// safe to send it patches and parameter changes.
    fn ewi_ready(&self) -> bool {
        self.midi.is_connected() && self.ewi == EwiStatus::Ewi4000s
    }

    /// Queue this frame's changes for the EWI and send whatever is due.
    fn flush_to_ewi(&mut self, ctx: &egui::Context) {
        let live = std::mem::take(&mut self.live_out);
        let patches = std::mem::take(&mut self.patch_out);
        if !self.ewi_ready() {
            return; // offline: the app's own copy is already updated
        }
        for c in live {
            self.send.push_live(c.nrpn, c.index, c.value);
        }
        for (patch, mode) in &patches {
            self.send.push_patch(patch, *mode);
        }

        let now = Instant::now();
        while let Some(msgs) = self.send.next(now) {
            for msg in msgs {
                if let Err(e) = self.midi.send(&msg) {
                    self.send.clear();
                    self.bulk_total = None;
                    self.ewi = EwiStatus::NotConnected;
                    self.message(
                        "MIDI",
                        format!("Lost the connection to the EWI: {e}\n\nCheck the cable, then use MIDI ▸ Ports…"),
                    );
                    return;
                }
            }
        }
        if let Some(at) = self.send.wake_at() {
            ctx.request_repaint_after(at.saturating_duration_since(now));
        }
        if self.bulk_total.is_some() && self.send.pending_patches() == 0 {
            let n = self.bulk_total.take().unwrap();
            self.set_status(format!("Sent {n} patches to the EWI"));
        }
    }

    /// Progress window shown while "Load into EWI" sends a whole set.
    fn show_send_progress(&mut self, ctx: &egui::Context) {
        let Some(total) = self.bulk_total else { return };
        let done = total.saturating_sub(self.send.pending_patches());
        let mut cancel = false;
        egui::Modal::new(egui::Id::unique("sending")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.heading("Sending patches to the EWI");
            ui.label(format!("{done} of {total}"));
            ui.add(egui::ProgressBar::new(done as f32 / total.max(1) as f32));
            ui.add_space(8.0);
            cancel = ui.button("Cancel").clicked();
        });
        if cancel {
            self.send.clear();
            self.bulk_total = None;
            self.message(
                "Load into EWI",
                format!(
                    "Stopped after {done} of {total} patches. The EWI now has a mix of old \
                     and new patches; use EWI ▸ Fetch All Patches to see what it holds."
                ),
            );
        }
    }

    fn ewi_label(&self) -> String {
        match &self.ewi {
            EwiStatus::NotConnected => "Not connected".into(),
            EwiStatus::Checking(_) => "Checking…".into(),
            EwiStatus::Ewi4000s => "EWI4000s".into(),
            EwiStatus::NoReply => "Not detected".into(),
            EwiStatus::Other(what) => format!("Not an EWI4000s ({what})"),
        }
    }

    /// Current enabled/checked state for the menus.
    fn menu_state(&self) -> MenuState {
        MenuState {
            check_ewi: self.midi.is_connected(),
            fetch_all: self.ewi_ready() && self.send.is_empty(),
            patch: self.editing() && self.dialog.is_none(),
            store_label: self.store_label(),
            theme: self.prefs.theme(),
            editor_size: self.prefs.editor_size(),
        }
    }

    fn run_command(&mut self, ctx: &egui::Context, cmd: Command) {
        match cmd {
            Command::Preferences => self.prefs_open = true,
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Command::Ports => self.open_ports_dialog(),
            Command::CheckEwi => {
                if self.midi.is_connected() {
                    self.start_id_check();
                }
            }
            Command::FetchAll => {
                if self.ewi_ready() && self.send.is_empty() {
                    self.fetch = Some(Fetcher::new());
                }
            }
            Command::Edit(op) => {
                if self.editing() && self.dialog.is_none() {
                    self.apply_op(op);
                }
            }
            Command::Theme(t) => self.set_theme(ctx, t),
            Command::EditorSize(n) => {
                if let Err(e) = self.prefs.set_editor_size(n) {
                    self.message("Preferences", format!("Could not save preferences: {e}"));
                }
            }
            Command::OnlineHelp => ctx.open_url(egui::OpenUrl::new_tab(ONLINE_HELP)),
            Command::About => self.dialog = Some(Dialog::About),
        }
    }

    /// The in-window menu bar (Windows and Linux, and the UI tests; macOS
    /// uses the native menu bar instead).
    fn menu_bar(&mut self, ui: &mut Ui) {
        let state = self.menu_state();
        let mut cmd = None;
        egui::MenuBar::new().ui(ui, |ui| {
            let ctx = ui.ctx().clone();
            ui.menu_button("File", |ui| {
                let shortcut = ctx.format_shortcut(&PREFERENCES);
                if ui
                    .add(Button::new("Preferences…").shortcut_text(shortcut))
                    .clicked()
                {
                    cmd = Some(Command::Preferences);
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    cmd = Some(Command::Quit);
                }
            });
            ui.menu_button("MIDI", |ui| {
                if ui.button("Ports…").clicked() {
                    cmd = Some(Command::Ports);
                }
                if ui
                    .add_enabled(state.check_ewi, Button::new("Check for EWI"))
                    .on_disabled_hover_text("Choose a MIDI In and Out port first")
                    .clicked()
                {
                    cmd = Some(Command::CheckEwi);
                }
            });
            ui.menu_button("EWI", |ui| {
                if ui
                    .add_enabled(state.fetch_all, Button::new("Fetch All Patches"))
                    .on_disabled_hover_text(NEED_EWI)
                    .clicked()
                {
                    cmd = Some(Command::FetchAll);
                }
            });
            ui.add_enabled_ui(state.patch, |ui| {
                ui.menu_button("Patch", |ui| {
                    let edit = |op| Some(Command::Edit(op));
                    let store =
                        Button::new(state.store_label).shortcut_text(ctx.format_shortcut(&STORE));
                    if ui.add(store).clicked() {
                        cmd = edit(EditOp::Store);
                    }
                    let revert =
                        Button::new("Revert Edits").shortcut_text(ctx.format_shortcut(&REVERT));
                    if ui.add(revert).clicked() {
                        cmd = edit(EditOp::Revert);
                    }
                    let copy_key =
                        ctx.format_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::C));
                    let copy = Button::new("Copy to Patch Scratchpad").shortcut_text(copy_key);
                    if ui.add(copy).clicked() {
                        cmd = edit(EditOp::CopyToScratchpad);
                    }
                    ui.separator();
                    for (title, items) in [("Generate", &GENERATE[..]), ("Process", &PROCESS[..])] {
                        ui.menu_button(title, |ui| {
                            for &(label, op) in items {
                                if ui.button(label).clicked() {
                                    cmd = edit(op);
                                }
                            }
                        });
                    }
                });
            });
            ui.menu_button("View", |ui| {
                ui.label(RichText::new("Theme").weak());
                for (t, label) in THEMES {
                    if ui.radio(state.theme == t, label).clicked() {
                        cmd = Some(Command::Theme(t));
                    }
                }
                ui.separator();
                ui.label(RichText::new("Patch Editor Size").weak());
                for n in crate::prefs::EDITOR_SIZES {
                    if ui.radio(state.editor_size == n, format!("{n}%")).clicked() {
                        cmd = Some(Command::EditorSize(n));
                    }
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Online Help").clicked() {
                    cmd = Some(Command::OnlineHelp);
                }
                if ui.button(format!("About {APP_NAME}")).clicked() {
                    cmd = Some(Command::About);
                }
            });
        });
        if let Some(cmd) = cmd {
            self.run_command(&ui.ctx().clone(), cmd);
        }
    }

    /// One-click switch between light and dark.
    fn theme_toggle(&mut self, ui: &mut Ui) {
        let (label, theme) = if ui.visuals().dark_mode {
            ("Light mode", egui::ThemePreference::Light)
        } else {
            ("Dark mode", egui::ThemePreference::Dark)
        };
        if ui.button(label).clicked() {
            self.set_theme(&ui.ctx().clone(), theme);
        }
    }

    /// File ▸ Preferences: appearance settings, applied as they change.
    fn preferences_window(&mut self, ctx: &egui::Context) {
        let mut look = self.prefs.look();
        let mut theme_pref = self.prefs.theme();
        let mut size = self.prefs.editor_size();
        let (mut open, mut done, mut reset) = (true, false, false);

        egui::Window::new("Preferences")
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.label(theme::title("Appearance", 16.0));
                ui.add_space(4.0);
                egui::Grid::new("prefs_grid")
                    .num_columns(2)
                    .spacing([18.0, 12.0])
                    .show(ui, |ui| {
                        ui.label("Theme");
                        ui.horizontal(|ui| {
                            for (t, label) in [
                                (egui::ThemePreference::Dark, "Dark"),
                                (egui::ThemePreference::Light, "Light"),
                                (egui::ThemePreference::System, "Follow System"),
                            ] {
                                ui.selectable_value(&mut theme_pref, t, label);
                            }
                        });
                        ui.end_row();

                        let name = ui.label("Accent colour").id;
                        ui.horizontal_wrapped(|ui| {
                            let dark = ui.visuals().dark_mode;
                            let default = ("Default", theme::default_accent(dark), None);
                            let choices = std::iter::once(default)
                                .chain(theme::ACCENTS.iter().map(|&(n, c)| (n, c, Some(c))));
                            for (label, colour, value) in choices {
                                if accent_swatch(ui, label, colour, look.accent == value) {
                                    look.accent = value;
                                }
                            }
                            // Any other colour.
                            let current = look.accent.unwrap_or(theme::default_accent(dark));
                            let mut rgb = [current.r(), current.g(), current.b()];
                            let r = egui::color_picker::color_edit_button_srgb(ui, &mut rgb)
                                .labelled_by(name)
                                .on_hover_text("Custom colour");
                            if r.changed() {
                                look.accent = Some(Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
                            }
                        });
                        ui.end_row();

                        let name = ui.label("Corners").id;
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Square").weak());
                            ui.add(
                                egui::Slider::new(&mut look.corners, 0..=theme::MAX_CORNERS)
                                    .show_value(false),
                            )
                            .labelled_by(name);
                            ui.label(RichText::new("Round").weak());
                        });
                        ui.end_row();

                        ui.label("Text size");
                        ui.horizontal(|ui| {
                            let mut pct = (look.text_scale * 100.0).round() as u32;
                            for n in crate::prefs::TEXT_SIZES {
                                ui.selectable_value(&mut pct, n, format!("{n}%"));
                            }
                            look.text_scale = pct as f32 / 100.0;
                        });
                        ui.end_row();

                        ui.label("Sliders");
                        ui.checkbox(&mut look.slider_fill, "Fill the track up to the handle");
                        ui.end_row();
                    });

                ui.add_space(10.0);
                ui.label(theme::title("Patch Editor", 16.0));
                ui.add_space(4.0);
                egui::Grid::new("prefs_editor_grid")
                    .num_columns(2)
                    .spacing([18.0, 12.0])
                    .show(ui, |ui| {
                        ui.label("Size");
                        ui.horizontal(|ui| {
                            for n in crate::prefs::EDITOR_SIZES {
                                ui.selectable_value(&mut size, n, format!("{n}%"));
                            }
                        });
                        ui.end_row();
                    });

                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    reset = ui.button("Reset to Defaults").clicked();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        done = ui.add(theme::primary_button(ui, "Done")).clicked();
                    });
                });
            });

        if reset {
            look = theme::Look::default();
            theme_pref = egui::ThemePreference::System;
            size = crate::prefs::DEFAULT_EDITOR_SIZE;
        }
        let mut result = Ok(());
        if look != self.prefs.look() {
            theme::apply(ctx, look);
            result = result.and(self.prefs.set_look(look));
        }
        if theme_pref != self.prefs.theme() {
            ctx.set_theme(theme_pref);
            result = result.and(self.prefs.set_theme(theme_pref));
        }
        if size != self.prefs.editor_size() {
            result = result.and(self.prefs.set_editor_size(size));
        }
        if let Err(e) = result {
            self.message("Preferences", format!("Could not save preferences: {e}"));
        }
        if done || !open {
            self.prefs_open = false;
        }
    }

    fn set_theme(&mut self, ctx: &egui::Context, theme: egui::ThemePreference) {
        ctx.set_theme(theme);
        if let Err(e) = self.prefs.set_theme(theme) {
            self.message("Preferences", format!("Could not save preferences: {e}"));
        }
    }

    fn tab_bar(&mut self, ui: &mut Ui) {
        let loaded = self.bank.is_some();
        let editing = loaded || self.editor.is_some();
        let mut picked = None;
        let p = theme::palette(ui);
        egui::Frame::new()
            .fill(p.control)
            .corner_radius(egui::CornerRadius::same(10))
            .inner_margin(egui::Margin::same(3))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.spacing_mut().button_padding = egui::vec2(14.0, 6.0);
                    for (tab, label, enabled) in [
                        (Tab::Scratchpad, "Patch Scratchpad", true),
                        (Tab::Library, "Patch Set Library", true),
                        (Tab::CurrentSet, "Current Patch Set", loaded),
                        (Tab::Editor, "Patch Editor", editing),
                    ] {
                        let r = ui.add_enabled(enabled, Button::selectable(self.tab == tab, label));
                        let r = r.on_disabled_hover_text(
                            "Load a patch set first (Patch Set Library tab), or open a \
                     Patch Scratchpad patch",
                        );
                        if r.clicked() && self.tab != tab {
                            picked = Some(tab);
                        }
                    }
                })
            });
        if let Some(tab) = picked {
            self.navigate(After::Tab(tab));
        }
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let p = theme::palette(ui);
        ui.horizontal(|ui| {
            // EWI connection first, with a coloured dot.
            let dot = match self.ewi {
                EwiStatus::Ewi4000s => p.good,
                EwiStatus::Checking(_) => p.warn,
                _ => p.text_weak,
            };
            theme::dot(ui, dot);
            ui.label(RichText::new("EWI").weak());
            ui.label(self.ewi_label());
            ui.add_space(14.0);
            for (label, value) in [
                (
                    "MIDI In",
                    self.midi.in_name().unwrap_or("Not set").to_string(),
                ),
                (
                    "MIDI Out",
                    self.midi.out_name().unwrap_or("Not set").to_string(),
                ),
                (
                    "Patch Scratchpad:",
                    format!("{} patches", self.scratch.len()),
                ),
            ] {
                ui.label(RichText::new(label).weak());
                ui.label(value);
                ui.add_space(14.0);
            }
            if let Some((msg, at)) = &self.status {
                let elapsed = at.elapsed();
                if elapsed < STATUS_TIMEOUT {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(msg.as_str()).truncate());
                    });
                    ui.ctx().request_repaint_after(STATUS_TIMEOUT - elapsed);
                }
            }
        });
    }

    // ---- tabs ----

    fn scratchpad_tab(&mut self, ui: &mut Ui) {
        if !self.scratch.is_persistent() {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "No library folder chosen, so the Patch Scratchpad won't be saved. \
                 Choose one on the Patch Set Library tab.",
            );
        }
        let names: Vec<String> = self.scratch.patches().iter().map(Patch::name).collect();
        let sel = self.scratch_sel.filter(|&i| i < names.len());
        let mut clicked = None;
        let mut button = None;
        ui.horizontal_top(|ui| {
            let size = egui::vec2(
                (ui.available_width() - 180.0).max(200.0),
                ui.available_height(),
            );
            ui.allocate_ui(size, |ui| {
                clicked = select_list(ui, "scratch", &names, sel, size.y - 20.0);
            });
            ui.vertical(|ui| {
                for (label, enabled) in [
                    ("Open in Patch Editor", sel.is_some()),
                    ("Preview", sel.is_some()),
                    ("Clear", !names.is_empty()),
                    ("Delete", sel.is_some()),
                    ("Rename…", sel.is_some()),
                    ("View in Hex", sel.is_some()),
                    ("Export…", sel.is_some()),
                ] {
                    if ui
                        .add_enabled(enabled, Button::new(label).min_size(egui::vec2(150.0, 0.0)))
                        .clicked()
                    {
                        button = Some(label);
                    }
                }
            });
        });
        if let Some((i, double)) = clicked {
            self.scratch_sel = Some(i);
            if double {
                self.navigate(After::OpenScratch(i));
                return;
            }
        }
        let name = sel.map(|i| names[i].clone()).unwrap_or_default();
        match (button, sel) {
            (Some("Open in Patch Editor"), Some(i)) => self.navigate(After::OpenScratch(i)),
            (Some("Preview"), Some(i)) => {
                let patch = self.scratch.patches()[i].clone();
                self.preview(&patch);
            }
            (Some("Clear"), _) => {
                self.dialog = Some(Dialog::Confirm {
                    title: "Clear Patch Scratchpad".into(),
                    text: "Do you really want to clear the Patch Scratchpad?".into(),
                    action: Confirm::ClearScratchpad,
                })
            }
            (Some("Delete"), Some(i)) => {
                self.scratch_sel = None;
                self.scratch_entry_removed(Some(i));
                if let Err(e) = self.scratch.remove(i) {
                    self.scratch_save_error(e);
                }
            }
            (Some("Rename…"), Some(i)) => {
                self.dialog = Some(Dialog::Input {
                    title: "Rename".into(),
                    prompt: "New name for this Patch Scratchpad patch:".into(),
                    value: name,
                    action: Input::RenameScratch(i),
                })
            }
            (Some("View in Hex"), Some(i)) => {
                let hex = self.scratch.patches()[i].to_hex(true);
                self.dialog = Some(Dialog::Hex { title: name, hex });
            }
            (Some("Export…"), Some(i)) => {
                self.dialog = Some(Dialog::Input {
                    title: "Export Patch".into(),
                    prompt: "File name for the patch (saved in the library's export folder):"
                        .into(),
                    value: format!("{name}.syx"),
                    action: Input::ExportScratch(i),
                })
            }
            _ => {}
        }
    }

    fn library_tab(&mut self, ui: &mut Ui) {
        let (mut change, mut rescan) = (false, false);
        ui.horizontal(|ui| {
            ui.label("Patch Set Library location:");
            match self.prefs.library() {
                Some(dir) => ui.strong(dir.display().to_string()),
                None => ui.strong("<Not Chosen>"),
            };
            change = ui.button("Change…").clicked();
            rescan = ui
                .add_enabled(self.prefs.library().is_some(), Button::new("Rescan"))
                .on_hover_text("Look in the folder again for new or removed patch sets")
                .clicked();
        });
        ui.separator();

        let set_sel = self.library.selected_set;
        let patch_names: Vec<String> = self.library.set_patches.iter().map(Patch::name).collect();
        let patch_sel = self
            .library
            .selected_patch
            .filter(|&i| i < patch_names.len());
        let list_h = (ui.available_height() - 70.0).max(100.0);
        let (mut import, mut load, mut delete, mut copy) = (false, false, false, false);
        let mut preview = false;
        let (mut set_clicked, mut patch_clicked) = (None, None);

        ui.columns(2, |cols| {
            let ui = &mut cols[0];
            ui.label(theme::title("Patch Sets", 15.0))
                .on_hover_text("Double-click a set to load it");
            set_clicked = select_list(ui, "sets", &self.library.sets, set_sel, list_h);
            ui.horizontal(|ui| {
                import = ui.button("Import…").clicked();
                let can_load = set_sel.is_some() && !patch_names.is_empty();
                load = ui
                    .add_enabled(can_load, Button::new("Load into EWI"))
                    .on_hover_text(if self.ewi_ready() {
                        "Send all the patches in this set to the EWI"
                    } else {
                        "Not connected to an EWI4000s, so this only loads the set \
                         into the Current Patch Set tab"
                    })
                    .clicked();
                delete = ui
                    .add_enabled(set_sel.is_some(), Button::new("Delete"))
                    .clicked();
            });

            let ui = &mut cols[1];
            ui.label(theme::title("Patches", 15.0));
            patch_clicked = select_list(ui, "set_patches", &patch_names, patch_sel, list_h);
            ui.horizontal(|ui| {
                copy = ui
                    .add_enabled(patch_sel.is_some(), Button::new("Copy to Patch Scratchpad"))
                    .clicked();
                preview = ui
                    .add_enabled(patch_sel.is_some(), Button::new("Preview"))
                    .on_hover_text(PREVIEW_HELP)
                    .clicked();
            });
        });

        if change {
            self.choose_library();
        }
        if rescan {
            self.refresh_library();
            let n = self.library.sets.len();
            self.set_status(format!("Found {n} patch sets"));
        }
        if let Some((i, double)) = set_clicked {
            if self.library.selected_set != Some(i) {
                self.select_set(i);
            }
            if double && !self.library.set_patches.is_empty() {
                self.confirm_load_set("");
            }
        }
        if let Some((i, _)) = patch_clicked {
            self.library.selected_patch = Some(i);
        }
        if import {
            self.import_legacy();
        }
        let set_name = self.selected_set_name().unwrap_or_default();
        if load {
            self.confirm_load_set("");
        }
        if delete {
            self.dialog = Some(Dialog::Confirm {
                title: "Delete Patch Set".into(),
                text: format!(
                    "This will delete \"{set_name}\" from the library. \
                     Do you really want to do this?"
                ),
                action: Confirm::DeleteSet,
            });
        }
        if let (true, Some(i)) = (copy, patch_sel) {
            let patch = self.library.set_patches[i].clone();
            self.add_to_scratchpad(patch);
        }
        if let (true, Some(i)) = (preview, patch_sel) {
            let patch = self.library.set_patches[i].clone();
            self.preview(&patch);
        }
    }

    fn current_set_tab(&mut self, ui: &mut Ui) {
        let Some(bank) = &self.bank else { return };
        let note = if self.ewi_ready() {
            "Click a patch for actions, or drag one onto another to swap them. \
             Changes are sent to the EWI straight away."
        } else {
            "Click a patch for actions, or drag one onto another to swap them. \
             Not connected to an EWI4000s: changes stay in EWITool2."
        };
        ui.label(RichText::new(note).weak());
        let spacing = egui::vec2(6.0, 4.0);
        let label_w = 26.0;
        // 5 label + 5 button columns with 9 gaps between them.
        let col_w =
            ((ui.available_width() - 5.0 * label_w - 9.0 * spacing.x - 4.0) / 5.0).max(60.0);
        let row_h = ((ui.available_height() - 50.0) / 20.0 - spacing.y).clamp(16.0, 60.0);
        let mut clicked = None;
        let mut swap = None;

        egui::Grid::new("bank_grid")
            .num_columns(10)
            .spacing(spacing)
            .min_col_width(label_w)
            .show(ui, |ui| {
                for row in 0..20 {
                    for col in 0..5 {
                        let ix = col * 20 + row;
                        ui.add_sized(
                            [label_w, row_h],
                            egui::Label::new(RichText::new(ix.to_string()).weak()),
                        );
                        let button = Button::new(bank.name(ix))
                            .truncate()
                            .sense(Sense::click_and_drag());
                        let r = ui.add_sized([col_w, row_h], button);
                        if r.drag_started() {
                            egui::DragAndDrop::set_payload(ui.ctx(), ix);
                        }
                        if r.dragged() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                        }
                        if r.dnd_hover_payload::<usize>().is_some_and(|src| *src != ix) {
                            let stroke = Stroke::new(2.0, ui.visuals().selection.stroke.color);
                            ui.painter().rect_stroke(
                                r.rect,
                                3.0,
                                stroke,
                                egui::StrokeKind::Outside,
                            );
                        }
                        if let Some(src) = r.dnd_release_payload::<usize>() {
                            if *src != ix {
                                swap = Some((*src, ix));
                            }
                        }
                        if r.clicked() {
                            clicked = Some(ix);
                        }
                    }
                    ui.end_row();
                }
            });

        let save = ui.button("Save to Library…").clicked();

        if let Some((a, b)) = swap {
            let bank = self.bank.as_mut().unwrap();
            bank.swap(a, b);
            for slot in [a, b] {
                if let Some(p) = bank.get(slot) {
                    self.patch_out.push((p.clone(), Mode::Save));
                }
            }
            if let Some(ed) = &mut self.editor {
                if ed.source == Source::Slot(a) {
                    ed.source = Source::Slot(b);
                } else if ed.source == Source::Slot(b) {
                    ed.source = Source::Slot(a);
                }
            }
            self.set_status(format!("Patches {a} and {b} swapped"));
        }
        if let Some(slot) = clicked {
            let name = self
                .bank
                .as_ref()
                .unwrap()
                .get(slot)
                .map(Patch::name)
                .unwrap_or_default();
            self.dialog = Some(Dialog::PatchActions {
                slot,
                name,
                paste: None,
            });
        }
        if save {
            self.dialog = Some(Dialog::Input {
                title: "Save Patch Set to Library".into(),
                prompt: "File name for the Patch Set (.syx is added if missing):".into(),
                value: String::new(),
                action: Input::SaveBank,
            });
        }
    }

    fn editor_tab(&mut self, ui: &mut Ui) {
        let source = self.editor.as_ref().map(|e| e.source);
        let selected = match source {
            Some(Source::Slot(s)) => Some(s),
            _ => None,
        };
        let mut pick = None;
        ui.horizontal(|ui| {
            let name = ui.label("Patch:").id;
            let ed_name = self
                .editor
                .as_ref()
                .map(|e| e.patch.name())
                .unwrap_or_default();
            let text = match (source, &self.bank) {
                (Some(Source::Slot(s)), Some(bank)) => format!("{s} - {}", bank.name(s)),
                (Some(Source::Scratch(_)), _) => format!("Patch Scratchpad: {ed_name}"),
                (Some(Source::Detached), _) => format!("{ed_name} (deleted from Patch Scratchpad)"),
                _ => "Choose a patch…".to_string(),
            };
            let combo = ComboBox::from_id_salt("editor_patch")
                .selected_text(text)
                .width(300.0)
                .height(500.0);
            match &self.bank {
                Some(bank) => {
                    combo
                        .show_ui(ui, |ui| {
                            for s in 0..NUM_PATCHES {
                                let label = format!("{s} - {}", bank.name(s));
                                if ui.selectable_label(selected == Some(s), label).clicked() {
                                    pick = Some(s);
                                }
                            }
                        })
                        .response
                        .labelled_by(name);
                }
                // No Current Patch Set yet: just show what's being edited.
                None => {
                    ui.add_enabled_ui(false, |ui| combo.show_ui(ui, |_| {}))
                        .inner
                        .response
                        .labelled_by(name);
                }
            }
            if let Some(ed) = &self.editor {
                if ed.patch != ed.unedited {
                    ui.colored_label(ui.visuals().warn_fg_color, "(edited)");
                }
            }
            ui.label(
                RichText::new("Use the Patch menu to store, revert, generate or process.").weak(),
            );
        });
        if let Some(slot) = pick {
            self.navigate(After::OpenSlot(slot));
        }
        self.preview_bar(ui);
        ui.separator();
        match &mut self.editor {
            Some(ed) => {
                let size = self.prefs.editor_size() as f32 / 100.0;
                editor::patch_editor(ui, &mut ed.patch, &mut self.live_out, size);
                // Edits are heard straight away while a preview plays.
                if self.audio.is_playing() {
                    self.audio.update_patch(&ed.patch);
                }
            }
            None => {
                ui.label("Choose a patch to edit.");
            }
        }
    }

    /// Play/Stop, note and breath for hearing the edited patch.
    fn preview_bar(&mut self, ui: &mut Ui) {
        let mut toggle = false;
        let old_note = self.audio.note;
        ui.horizontal(|ui| {
            let looping = self.audio.is_looping();
            let label = if looping { "■ Stop" } else { "▶ Play" };
            toggle = ui
                .add_enabled(self.editor.is_some(), Button::new(label))
                .on_hover_text(format!(
                    "{PREVIEW_HELP} Repeats the note until stopped, so you hear \
                     each change as you make it."
                ))
                .clicked();
            let label = ui.label("Note:").id;
            ComboBox::from_id_salt("preview_note")
                .selected_text(audio::note_name(self.audio.note))
                .width(60.0)
                .height(300.0)
                .show_ui(ui, |ui| {
                    for n in audio::PREVIEW_NOTES {
                        ui.selectable_value(&mut self.audio.note, n, audio::note_name(n));
                    }
                })
                .response
                .labelled_by(label);
            let label = ui.label("Breath:").id;
            let mut breath = self.audio.breath;
            let r = ui
                .add(egui::Slider::new(&mut breath, 0.05..=1.0).show_value(false))
                .labelled_by(label);
            if r.changed() {
                self.audio.set_breath(breath);
            }
            ui.label(
                RichText::new("Preview is EWITool2's approximation of the EWI's sound.").weak(),
            );
        });
        let Some(ed) = &self.editor else { return };
        if toggle {
            if self.audio.is_looping() {
                self.audio.stop();
            } else {
                let patch = ed.patch.clone();
                self.audio.play(&patch, true);
                self.audio_error();
            }
        } else if self.audio.note != old_note && self.audio.is_looping() {
            let patch = ed.patch.clone();
            self.audio.play(&patch, true);
        }
    }

    // ---- dialogs ----

    fn show_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };
        let mut close = false;
        let mut action = None;
        let mut preview_slot = None;

        let modal = egui::Modal::new(egui::Id::unique("dialog")).show(ctx, |ui| {
            ui.set_max_width(560.0);
            match &mut dialog {
                Dialog::Message { title, text } => {
                    ui.heading(title.as_str());
                    ui.label(text.as_str());
                    close = ok_button(ui);
                }
                Dialog::Ports {
                    ins,
                    outs,
                    sel_in,
                    sel_out,
                } => {
                    ui.heading("Select MIDI Ports");
                    ui.label("Choose the ports your EWI4000s is connected to.");
                    ui.columns(2, |cols| {
                        cols[0].strong("MIDI In Ports");
                        if let Some((i, _)) = select_list(&mut cols[0], "ins", ins, *sel_in, 160.0)
                        {
                            *sel_in = Some(i);
                        }
                        cols[1].strong("MIDI Out Ports");
                        if let Some((i, _)) =
                            select_list(&mut cols[1], "outs", outs, *sel_out, 160.0)
                        {
                            *sel_out = Some(i);
                        }
                    });
                    if ins.is_empty() && outs.is_empty() {
                        ui.colored_label(
                            ui.visuals().warn_fg_color,
                            "No MIDI ports found. Check the EWI's USB cable, then Refresh.",
                        );
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.add(theme::primary_button(ui, "OK")).clicked() {
                            action = Some(Action::SetPorts {
                                midi_in: sel_in.and_then(|i| ins.get(i).cloned()),
                                midi_out: sel_out.and_then(|i| outs.get(i).cloned()),
                            });
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                        if ui.button("Refresh").clicked() {
                            action = Some(Action::RefreshPorts);
                        }
                    });
                }
                Dialog::UnsavedEdits { after } => {
                    let after = *after;
                    let (name, to_scratch) = self
                        .editor
                        .as_ref()
                        .map(|e| (e.patch.name(), matches!(e.source, Source::Scratch(_))))
                        .unwrap_or_default();
                    ui.heading("Unsaved Edits");
                    ui.label(if to_scratch {
                        format!(
                            "You've edited \"{name}\". Save the changes to the Patch Scratchpad?"
                        )
                    } else {
                        format!(
                            "You've edited \"{name}\" without storing it. Copy the edited \
                             patch to the Patch Scratchpad?"
                        )
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let copy_label = if to_scratch {
                            "Save to Patch Scratchpad"
                        } else {
                            "Copy to Patch Scratchpad"
                        };
                        if ui.add(theme::primary_button(ui, copy_label)).clicked() {
                            action = Some(Action::LeaveEditor { copy: true, after });
                        }
                        let skip_label = if to_scratch {
                            "Don't Save"
                        } else {
                            "Don't Copy"
                        };
                        if ui.button(skip_label).clicked() {
                            action = Some(Action::LeaveEditor { copy: false, after });
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::About => {
                    ui.heading(format!("{APP_NAME} version {APP_VERSION}"));
                    ui.label(COPYRIGHT);
                    close = ok_button(ui);
                }
                Dialog::Confirm {
                    title,
                    text,
                    action: a,
                } => {
                    ui.heading(title.as_str());
                    ui.label(text.as_str());
                    match ok_cancel(ui, "OK") {
                        Some(true) => action = Some(Action::Confirmed(*a)),
                        Some(false) => close = true,
                        None => {}
                    }
                }
                Dialog::Input {
                    title,
                    prompt,
                    value,
                    action: a,
                } => {
                    ui.heading(title.as_str());
                    let name = ui.label(prompt.as_str()).id;
                    let r = ui
                        .add(egui::TextEdit::singleline(value).desired_width(400.0))
                        .labelled_by(name);
                    if ui.memory(|m| m.focused().is_none()) {
                        r.request_focus();
                    }
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    match ok_cancel(ui, "OK") {
                        Some(true) => action = Some(Action::Entered(*a, value.clone())),
                        Some(false) => close = true,
                        None if enter => action = Some(Action::Entered(*a, value.clone())),
                        None => {}
                    }
                }
                Dialog::Hex { title, hex } => {
                    ui.heading(format!("{title} (hex)"));
                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut hex.as_str())
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(520.0),
                            );
                        });
                    close = ok_button(ui);
                }
                Dialog::PatchActions { slot, name, paste } => {
                    let slot = *slot;
                    let empty = self.bank.as_ref().is_none_or(|b| b.get(slot).is_none());
                    ui.heading(format!("Patch #{slot}"));
                    ui.horizontal(|ui| {
                        let label = ui.label("Name:").id;
                        ui.add_enabled(!empty, egui::TextEdit::singleline(name))
                            .labelled_by(label);
                    });
                    ui.horizontal(|ui| {
                        let label = ui.label("Patch Scratchpad patch to paste:").id;
                        let names: Vec<String> =
                            self.scratch.patches().iter().map(Patch::name).collect();
                        let text = paste
                            .and_then(|i| names.get(i).cloned())
                            .unwrap_or_default();
                        ComboBox::from_id_salt("paste_choice")
                            .selected_text(text)
                            .width(200.0)
                            .show_ui(ui, |ui| {
                                for (i, n) in names.iter().enumerate() {
                                    ui.selectable_value(paste, Some(i), n);
                                }
                            })
                            .response
                            .labelled_by(label);
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Edit").clicked() {
                            action = Some(Action::Edit(slot));
                        }
                        if !empty && ui.button("Preview").on_hover_text(PREVIEW_HELP).clicked() {
                            preview_slot = Some(slot);
                        }
                        if !empty && ui.button("Copy to Patch Scratchpad").clicked() {
                            action = Some(Action::CopyToScratchpad(slot));
                        }
                        if ui.button("Paste from Patch Scratchpad").clicked() {
                            action = Some(Action::Paste { slot, from: *paste });
                        }
                        if !empty && ui.button("Rename").clicked() {
                            action = Some(Action::Rename {
                                slot,
                                name: name.clone(),
                            });
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::Merge { selected } => {
                    ui.heading("Merge Patches");
                    ui.label("Choose patch to merge with…");
                    let names: Vec<String> = (0..NUM_PATCHES)
                        .map(|s| {
                            let name = self.bank.as_ref().map(|b| b.name(s)).unwrap_or_default();
                            format!("{s} - {name}")
                        })
                        .collect();
                    if let Some((i, _)) = select_list(ui, "merge", &names, *selected, 300.0) {
                        *selected = Some(i);
                    }
                    match ok_cancel(ui, "OK") {
                        Some(true) => match selected {
                            Some(i) => action = Some(Action::MergeWith(*i)),
                            None => close = true,
                        },
                        Some(false) => close = true,
                        None => {}
                    }
                }
            }
        });

        if let Some(slot) = preview_slot {
            if let Some(p) = self.bank.as_ref().and_then(|b| b.get(slot)).cloned() {
                self.preview(&p);
            }
        }
        if let Some(action) = action {
            self.run(action);
        } else if !close && !modal.should_close() {
            self.dialog = Some(dialog);
        }
    }

    fn run(&mut self, action: Action) {
        match action {
            Action::Confirmed(Confirm::ClearScratchpad) => {
                self.scratch_sel = None;
                self.scratch_entry_removed(None);
                if let Err(e) = self.scratch.clear() {
                    self.scratch_save_error(e);
                }
            }
            Action::Confirmed(Confirm::LoadSet) => {
                let name = self.selected_set_name().unwrap_or_default();
                let bank = Bank::from_patches(self.library.set_patches.clone());
                let n = bank.len();
                if self.ewi_ready() {
                    self.patch_out
                        .extend(bank.patches().into_iter().map(|p| (p, Mode::Save)));
                    self.bulk_total = Some(n);
                    self.set_status(format!("Sending {name} to the EWI…"));
                } else {
                    self.set_status(format!(
                        "Loaded {name} ({n} patches) into {APP_NAME} (EWI not connected)"
                    ));
                }
                self.bank = Some(bank);
                self.bank_replaced();
                self.tab = Tab::CurrentSet;
            }
            Action::Confirmed(Confirm::DeleteSet) => {
                let (Some(dir), Some(name)) = (self.prefs.library(), self.selected_set_name())
                else {
                    return;
                };
                match std::fs::remove_file(dir.join(&name)) {
                    Ok(()) => {
                        self.refresh_library();
                        self.set_status(format!("{name} deleted"));
                    }
                    Err(e) => {
                        self.message("Delete Patch Set", format!("Could not delete {name}: {e}"))
                    }
                }
            }
            Action::Entered(Input::RenameScratch(i), name) => {
                if let Err(e) = self.scratch.rename(i, name.trim()) {
                    self.message("Rename", e);
                }
            }
            Action::Entered(Input::ExportScratch(i), file) => {
                let result = self.library_file(&file).and_then(|(dir, file)| {
                    let dir = dir.join("export");
                    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                    let patch = &self.scratch.patches()[i];
                    patch_set::save_patch(&dir.join(&file), patch).map_err(save_error)?;
                    Ok(file)
                });
                match result {
                    Ok(file) => self.set_status(format!("Patch exported to export/{file}")),
                    Err(e) => self.message("Export Patch", e),
                }
            }
            Action::Entered(Input::SaveBank, file) => {
                let patches = self.bank.as_ref().map(Bank::patches).unwrap_or_default();
                let result = self.library_file(&file).and_then(|(dir, file)| {
                    patch_set::save_syx(&dir.join(&file), &patches).map_err(save_error)?;
                    Ok(file)
                });
                match result {
                    Ok(file) => {
                        self.refresh_library();
                        self.set_status(format!("Patch set {file} saved to library"));
                    }
                    Err(e) => self.message("Save Patch Set to Library", e),
                }
            }
            Action::Edit(slot) => self.navigate(After::OpenSlot(slot)),
            Action::CopyToScratchpad(slot) => {
                if let Some(p) = self.bank.as_ref().and_then(|b| b.get(slot)).cloned() {
                    self.add_to_scratchpad(p);
                }
            }
            Action::Paste { slot, from } => {
                let Some(patch) = from.and_then(|i| self.scratch.patches().get(i)).cloned() else {
                    self.message("Paste", "Choose a Patch Scratchpad patch to paste first.");
                    return;
                };
                let Some(bank) = &mut self.bank else { return };
                let name = patch.name();
                bank.set(slot, patch);
                self.patch_out
                    .push((bank.get(slot).unwrap().clone(), Mode::Save));
                if self
                    .editor
                    .as_ref()
                    .is_some_and(|e| e.source == Source::Slot(slot))
                {
                    self.open_slot_editor(slot);
                }
                self.set_status(format!("{name} pasted into patch #{slot}"));
            }
            Action::Rename { slot, name } => {
                let name = name.trim().to_string();
                let Some(patch) = self.bank.as_mut().and_then(|b| b.get_mut(slot)) else {
                    return;
                };
                match patch.set_name(&name) {
                    Ok(()) => {
                        self.patch_out.push((patch.clone(), Mode::Save));
                        if let Some(ed) = self
                            .editor
                            .as_mut()
                            .filter(|e| e.source == Source::Slot(slot))
                        {
                            let _ = ed.patch.set_name(&name);
                            let _ = ed.unedited.set_name(&name);
                        }
                        self.set_status(format!("Patch #{slot} renamed to {name}"));
                    }
                    Err(e) => {
                        self.message("Rename Patch", format!("Could not rename to '{name}': {e}"))
                    }
                }
            }
            Action::MergeWith(other) => {
                let other = self.bank.as_ref().and_then(|b| b.get(other)).cloned();
                if let (Some(other), Some(ed)) = (other, self.editor.as_mut()) {
                    edit::merge_with(&mut ed.patch, &other);
                    self.patch_out.push((ed.patch.clone(), Mode::Edit));
                }
            }
            Action::LeaveEditor { copy, after } => {
                if let Some(ed) = &mut self.editor {
                    ed.asked = Some(ed.patch.clone());
                    if copy {
                        let patch = ed.patch.clone();
                        if let Source::Scratch(ix) = ed.source {
                            ed.unedited = patch.clone();
                            let name = patch.name();
                            match self.scratch.replace(ix, patch) {
                                Ok(()) => {
                                    self.set_status(format!("{name} saved to the Patch Scratchpad"))
                                }
                                Err(e) => self.scratch_save_error(e),
                            }
                        } else {
                            self.add_to_scratchpad(patch);
                        }
                    }
                }
                self.go(after);
            }
            Action::SetPorts { midi_in, midi_out } => {
                if let Err(e) = self.prefs.set_midi_ports(midi_in, midi_out) {
                    self.message("Preferences", format!("Could not save preferences: {e}"));
                }
                self.reconnect = true;
            }
            Action::RefreshPorts => self.open_ports_dialog(),
        }
    }

    /// Check a file name typed by the user and pair it with the library
    /// folder, adding `.syx` if missing.
    fn library_file(&self, input: &str) -> Result<(PathBuf, String), String> {
        let dir = self
            .prefs
            .library()
            .ok_or("Choose a Patch Set Library location first (Patch Set Library tab).")?;
        let mut name = input.trim().to_string();
        if name.contains(['/', '\\']) {
            return Err("The file name can't contain / or \\.".into());
        }
        if !name.to_lowercase().ends_with(".syx") {
            name.push_str(".syx");
        }
        if name.len() <= ".syx".len() {
            return Err("Please enter a file name.".into());
        }
        Ok((dir.to_path_buf(), name))
    }
}

fn save_error(e: ewitool_core::Error) -> String {
    match e {
        ewitool_core::Error::Io(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            "A file with that name already exists, please use a different name.".into()
        }
        e => format!("Could not write the file: {e}"),
    }
}

/// A scrollable single-selection list. Returns the clicked row, and whether
/// it was double-clicked.
fn select_list(
    ui: &mut Ui,
    id: &str,
    items: &[String],
    selected: Option<usize>,
    height: f32,
) -> Option<(usize, bool)> {
    let mut clicked = None;
    theme::card(ui, 1.0)
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(id)
                .auto_shrink(false)
                .max_height(height)
                .show(ui, |ui| {
                    // Always a vertical list, whatever layout the caller uses.
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        if items.is_empty() {
                            ui.weak("(none)");
                        }
                        for (i, item) in items.iter().enumerate() {
                            let r = ui.selectable_label(selected == Some(i), item);
                            // egui counts a click elsewhere just before as part of
                            // a triple-click, so treat that as a double-click too.
                            if r.double_clicked() || r.triple_clicked() {
                                clicked = Some((i, true));
                            } else if r.clicked() {
                                clicked = Some((i, false));
                            }
                        }
                    });
                });
        });
    clicked
}

/// A round colour swatch for Preferences. Returns true when clicked.
fn accent_swatch(ui: &mut Ui, name: &str, colour: Color32, selected: bool) -> bool {
    let size = ui.spacing().interact_size.y;
    let (rect, r) = ui.allocate_exact_size(egui::vec2(size, size), Sense::click());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Button, true, name));
    let painter = ui.painter();
    let centre = rect.center();
    if selected {
        painter.circle_stroke(
            centre,
            size * 0.5,
            Stroke::new(2.0, ui.visuals().text_color()),
        );
    }
    let grow = if r.hovered() { 1.0 } else { 0.0 };
    painter.circle_filled(centre, size * 0.36 + grow, colour);
    r.on_hover_text(name).clicked()
}

fn ok_button(ui: &mut Ui) -> bool {
    ui.add_space(8.0);
    let button = theme::primary_button(ui, "OK");
    ui.add(button).clicked()
}

/// OK/Cancel buttons: `Some(true)` for OK, `Some(false)` for Cancel.
fn ok_cancel(ui: &mut Ui, ok: &str) -> Option<bool> {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let button = theme::primary_button(ui, ok);
        if ui.add(button).clicked() {
            Some(true)
        } else if ui.button("Cancel").clicked() {
            Some(false)
        } else {
            None
        }
    })
    .inner
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.shortcuts(&ctx);
        if !self.theme_applied {
            self.theme_applied = true;
            theme::install(&ctx, self.prefs.look());
            ctx.set_theme(self.prefs.theme());
        }
        if std::mem::take(&mut self.reconnect) {
            self.connect_midi(&ctx);
        }
        self.poll_midi(&ctx);
        #[cfg(target_os = "macos")]
        if let Some(menu) = &mut self.native_menu {
            for cmd in menu.take_commands() {
                self.run_command(&ctx, cmd);
            }
        }

        let p = if ctx.global_style().visuals.dark_mode {
            theme::DARK
        } else {
            theme::LIGHT
        };
        let bar = egui::Frame::new()
            .fill(p.surface)
            .stroke(egui::Stroke::new(1.0, p.border))
            .inner_margin(egui::Margin::symmetric(12, 6));
        if !self.has_native_menu() {
            egui::Panel::top("menu")
                .frame(bar)
                .show(ui, |ui| self.menu_bar(ui));
        }
        egui::Panel::bottom("status")
            .frame(bar)
            .show(ui, |ui| self.status_bar(ui));
        let page = egui::Frame::new()
            .fill(p.bg)
            .inner_margin(egui::Margin::same(14));
        egui::CentralPanel::default().frame(page).show(ui, |ui| {
            ui.horizontal(|ui| {
                self.tab_bar(ui);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.theme_toggle(ui);
                });
            });
            ui.add_space(10.0);
            match self.tab {
                Tab::Scratchpad => self.scratchpad_tab(ui),
                Tab::Library => self.library_tab(ui),
                Tab::CurrentSet => self.current_set_tab(ui),
                Tab::Editor => self.editor_tab(ui),
            }
        });
        // The editor's repeating preview stops when you leave the editor.
        if self.tab != Tab::Editor && self.audio.is_looping() {
            self.audio.stop();
        }
        if self.audio.is_playing() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if self.fetch.is_some() {
            self.show_fetch_progress(&ctx);
        } else if self.bulk_total.is_some() {
            self.show_send_progress(&ctx);
        } else {
            self.show_dialog(&ctx);
        }
        if self.prefs_open {
            self.preferences_window(&ctx);
        }

        self.flush_to_ewi(&ctx);
        #[cfg(target_os = "macos")]
        {
            let state = self.menu_state();
            if let Some(menu) = &mut self.native_menu {
                menu.sync(&state);
            }
        }
    }
}
