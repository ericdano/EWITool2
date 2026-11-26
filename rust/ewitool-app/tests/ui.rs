//! Headless UI tests: drive the real app through every tab.
//!
//! Set `EWITOOL_SCREENSHOTS=<dir>` to also save a PNG of each screen.

use std::path::Path;

use eframe::egui::{self, accesskit::Role};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use ewitool_app::App;
use ewitool_core::patch::user_to_internal;
use ewitool_core::scratchpad::SCRATCHPAD_NAME;
use ewitool_core::{edit, patch_set, Patch, NUM_PATCHES, PATCH_LEN};

const NAMES: [&str; 6] = [
    "Sax Lead",
    "Warm Pad",
    "Flute",
    "Brass Stab",
    "Synth Bass",
    "Oboe",
];

fn make_patch(name: &str, slot: u8, rng: &mut fastrand::Rng) -> Patch {
    let mut blob = [0u8; PATCH_LEN];
    blob[..4].copy_from_slice(&[0xf0, 0x47, 0x64, 0x7f]);
    blob[PATCH_LEN - 1] = 0xf7;
    let mut p = Patch::decode(&blob);
    p.internal_num = user_to_internal(slot).unwrap();
    p.set_name(name).unwrap();
    edit::default_patch(&mut p);
    if slot > 0 {
        edit::random_patch(&mut p, rng);
    }
    p
}

/// A library folder with a 100-patch set and 3 scratchpad patches.
fn sample_library() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let mut rng = fastrand::Rng::with_seed(7);
    let set: Vec<Patch> = (0..NUM_PATCHES as u8)
        .map(|slot| {
            let name = NAMES
                .get(slot as usize)
                .map_or(format!("Patch {slot}"), |n| n.to_string());
            make_patch(&name, slot, &mut rng)
        })
        .collect();
    patch_set::save_syx(&dir.path().join("Demo Set.syx"), &set).unwrap();
    let scratch: Vec<Patch> = ["Clarinet", "Soft Lead", "Choir"]
        .iter()
        .map(|n| make_patch(n, 1, &mut rng))
        .collect();
    std::fs::write(
        dir.path().join(SCRATCHPAD_NAME),
        patch_set::to_syx(&scratch),
    )
    .unwrap();
    dir
}

fn shot(h: &mut Harness<'_, App>, name: &str) {
    if let Some(dir) = std::env::var_os("EWITOOL_SCREENSHOTS") {
        let img = h.render().expect("render");
        img.save(Path::new(&dir).join(format!("{name}.png")))
            .unwrap();
    }
}

fn harness(lib: &Path) -> Harness<'static, App> {
    let lib = lib.to_path_buf();
    Harness::builder()
        .with_size([1280.0, 860.0])
        // Real-time frames, so two quick clicks count as a double-click.
        .with_step_dt(1.0 / 60.0)
        .build_eframe(move |_cc| App::with_library(Some(lib)))
}

#[test]
fn walk_through_every_screen() {
    let lib = sample_library();
    let mut h = harness(lib.path());
    h.run();

    // Scratchpad loads from the library folder.
    h.get_by_label("Clarinet");
    h.get_by_label("Choir");
    shot(&mut h, "1-scratchpad");

    // Library: pick the set and load it.
    h.get_by_label("Patch Sets").click();
    h.run();
    h.get_by_label("Demo Set.syx").click();
    h.run();
    h.get_by_label("Warm Pad");
    shot(&mut h, "2-library");
    h.get_by_label("Load into EWI").click();
    h.run();
    shot(&mut h, "3-confirm-load");
    h.get_by_label("OK").click();
    h.run();

    // Current Patch Set shows all 100 slots.
    h.get_by_label("Sax Lead");
    h.get_by_label("Patch 99");
    shot(&mut h, "4-current-set");

    // Patch actions dialog, then edit.
    // Click selects a patch; Actions… (or a double-click) opens its dialog.
    h.get_by_label("Warm Pad").click();
    h.run();
    h.get_by_label("Actions…").click();
    h.run();
    h.get_by_label("Paste from Patch Library");
    shot(&mut h, "5-patch-actions");
    h.get_by_label("Edit").click();
    h.run();

    // Editor panels are all present.
    for title in [
        "Oscillator 1",
        "Oscillator 2",
        "Primary Osc Filter",
        "Secondary Osc Filter",
        "Primary Noise Filter",
        "Secondary Noise Filter",
        "Formant Filter",
        "Key Trigger",
        "Noise",
        "Chorus",
        "Delay",
        "Reverb",
        "Bite",
        "Pitch Bend",
        "Anti-Alias",
        "Levels",
    ] {
        h.get_by_label(title);
    }
    assert!(h.query_by_label("(edited)").is_none());
    shot(&mut h, "6-editor");

    // Patch > Generate > Default Patch marks the patch as edited.
    h.get_by_label("Patch").click();
    h.run();
    h.get_by_label("Generate").hover();
    h.run();
    h.get_by_label("Default Patch").click();
    h.run();
    h.get_by_label("(edited)");
    shot(&mut h, "7-editor-edited");
}

#[test]
fn copy_from_library_to_scratchpad_persists() {
    let lib = sample_library();
    let mut h = harness(lib.path());
    h.run();
    h.get_by_label("Patch Sets").click();
    h.run();
    h.get_by_label("Demo Set.syx").click();
    h.run();
    h.get_by_label("Flute").click();
    h.run();
    h.get_by_label("Copy to Patch Library").click();
    h.run();

    let saved = patch_set::load_syx(&lib.path().join(SCRATCHPAD_NAME)).unwrap();
    let names: Vec<String> = saved.iter().map(Patch::name).collect();
    assert_eq!(names, ["Clarinet", "Soft Lead", "Choir", "Flute"]);
}

#[test]
fn save_current_set_to_library() {
    let lib = sample_library();
    let mut h = harness(lib.path());
    h.run();
    h.get_by_label("Patch Sets").click();
    h.run();
    h.get_by_label("Demo Set.syx").click();
    h.run();
    h.get_by_label("Load into EWI").click();
    h.run();
    h.get_by_label("OK").click();
    h.run();
    h.get_by_label("Save to Library…").click();
    h.run();
    shot(&mut h, "8-save-set");
    h.get_by_role(Role::TextInput).type_text("Copy");
    h.run();
    h.get_by_label("OK").click();
    h.run();

    let original = patch_set::load_syx(&lib.path().join("Demo Set.syx")).unwrap();
    let copy = patch_set::load_syx(&lib.path().join("Copy.syx")).unwrap();
    assert_eq!(original, copy);
}

/// Open the editor on "Warm Pad" (shared by the tests below).
fn open_editor(h: &mut Harness<'_, App>) {
    h.get_by_label("Patch Sets").click();
    h.run();
    h.get_by_label("Demo Set.syx").click();
    h.run();
    h.get_by_label("Load into EWI").click();
    h.run();
    h.get_by_label("OK").click();
    h.run();
    // Click selects a patch; Actions… (or a double-click) opens its dialog.
    h.get_by_label("Warm Pad").click();
    h.run();
    h.get_by_label("Actions…").click();
    h.run();
    h.get_by_label("Edit").click();
    h.run();
}

#[test]
fn theme_toggle_switches_light_and_dark() {
    let lib = sample_library();
    let mut h = harness(lib.path());
    h.run();
    let dark_at_start = h.ctx.global_style().visuals.dark_mode;
    let toggle = if dark_at_start {
        "Light mode"
    } else {
        "Dark mode"
    };
    h.get_by_label(toggle).click();
    h.run();
    assert_ne!(h.ctx.global_style().visuals.dark_mode, dark_at_start);

    open_editor(&mut h);
    if !h.ctx.global_style().visuals.dark_mode {
        shot(&mut h, "9-editor-light");
    }
}

#[test]
fn editor_reflows_with_the_window() {
    let lib = sample_library();
    let mut h = harness(lib.path());
    h.run();
    open_editor(&mut h);
    let rect = |h: &Harness<'_, App>, label: &str| h.get_by_label(label).rect();

    // Default window: 200% size.
    let normal = rect(&h, "Oscillator 1").width();
    shot(&mut h, "10-editor-default");

    // A shorter window must not shrink anything (it scrolls instead).
    h.set_size(egui::vec2(1280.0, 500.0));
    h.run();
    let short = rect(&h, "Oscillator 1").width();
    assert!(
        (short - normal).abs() < 1.0,
        "height changed the size: {normal} -> {short}"
    );

    // Wide: the two oscillators sit side by side at the same size.
    h.set_size(egui::vec2(2400.0, 1100.0));
    h.run();
    let (o1, o2) = (rect(&h, "Oscillator 1"), rect(&h, "Oscillator 2"));
    assert!(
        (o1.top() - o2.top()).abs() < 1.0,
        "should share a row: {o1:?} {o2:?}"
    );
    assert!((o1.width() - normal).abs() < 1.0);
    shot(&mut h, "11-editor-wide");

    // Tall and narrow: panels stack and nothing is wider than the window.
    h.set_size(egui::vec2(900.0, 1500.0));
    h.run();
    let (o1, o2) = (rect(&h, "Oscillator 1"), rect(&h, "Oscillator 2"));
    assert!(o2.top() > o1.bottom(), "should stack: {o1:?} {o2:?}");
    for title in ["Oscillator 1", "Secondary Osc Filter", "Chorus", "Levels"] {
        assert!(
            rect(&h, title).right() <= 900.0,
            "{title} is off the right edge"
        );
    }
    shot(&mut h, "12-editor-tall");
}

#[test]
fn edit_a_scratchpad_patch_and_save_it_when_leaving() {
    let lib = sample_library();
    let mut h = harness(lib.path());
    h.run();

    // Open a Patch Library patch; no patch set needs to be loaded.
    let pos = h.get_by_label("Clarinet").rect().center();
    double_click(&mut h, pos);
    h.get_by_label("Oscillator 1"); // the editor opened

    // Edit it, then try to leave the editor.
    h.get_by_label("Patch").click();
    h.run();
    h.get_by_label("Generate").hover();
    h.run();
    h.get_by_label("Default Patch").click();
    h.run();
    h.get_by_label("Patch Sets").click();
    h.run();
    h.get_by_label("Unsaved Edits");
    shot(&mut h, "13-unsaved-edits");
    h.get_by_label("Save to Patch Library").click();
    h.run();

    // Left the editor, and the edited patch replaced the original entry.
    h.get_by_label("Patch Sets");
    let saved = patch_set::load_syx(&lib.path().join(SCRATCHPAD_NAME)).unwrap();
    assert_eq!(saved.len(), 3);
    assert_eq!(saved[0].name(), "Clarinet");
    assert_eq!((saved[0].osc1.triangle, saved[0].osc1.level), (127, 96));

    // Going back and leaving again doesn't ask a second time.
    h.get_by_label("Patch Editor").click();
    h.run();
    h.get_by_label("Patch Sets").click();
    h.run();
    assert!(h.query_by_label("Unsaved Edits").is_none());
}

#[test]
fn leaving_edited_ewi_patch_offers_a_copy() {
    let lib = sample_library();
    let mut h = harness(lib.path());
    h.run();
    open_editor(&mut h);
    h.get_by_label("Patch").click();
    h.run();
    h.get_by_label("Generate").hover();
    h.run();
    h.get_by_label("Default Patch").click();
    h.run();

    h.get_by_label("Patch Library").click();
    h.run();
    h.get_by_label("Copy to Patch Library").click();
    h.run();
    let saved = patch_set::load_syx(&lib.path().join(SCRATCHPAD_NAME)).unwrap();
    let names: Vec<String> = saved.iter().map(Patch::name).collect();
    assert_eq!(names, ["Clarinet", "Soft Lead", "Choir", "Warm Pad"]);

    // Cancel keeps you in the editor.
    h.get_by_label("Patch Editor").click();
    h.run();
    h.get_by_label("Patch").click();
    h.run();
    h.get_by_label("Generate").hover();
    h.run();
    h.get_by_label("Random Patch").click();
    h.run();
    h.get_by_label("Current Patch Set").click();
    h.run();
    h.get_by_label("Cancel").click();
    h.run();
    h.get_by_label("Oscillator 1");
}

/// A real double-click: two mouse presses at the same spot in quick
/// succession (the harness's `click()` is an accessibility action, which
/// doesn't register as a double-click).
fn double_click(h: &mut Harness<'_, App>, pos: egui::Pos2) {
    h.hover_at(pos);
    h.step();
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::default(),
            });
            h.step();
        }
    }
    h.run();
}

#[test]
fn double_click_set_offers_to_load_it() {
    let lib = sample_library();
    let mut h = harness(lib.path());
    h.run();
    h.get_by_label("Patch Sets").click();
    h.run();
    let pos = h.get_by_label("Demo Set.syx").rect().center();
    double_click(&mut h, pos);
    h.get_by_label("Load Patch Set");
    shot(&mut h, "14-double-click-load");
    h.get_by_label("OK").click();
    h.run();
    h.get_by_label("Sax Lead");
}

#[test]
fn rescan_finds_new_sets_and_keeps_selection() {
    let lib = sample_library();
    let mut h = harness(lib.path());
    h.run();
    h.get_by_label("Patch Sets").click();
    h.run();
    h.get_by_label("Demo Set.syx").click();
    h.run();
    assert!(h.query_by_label("Another.syx").is_none());

    std::fs::copy(
        lib.path().join("Demo Set.syx"),
        lib.path().join("Another.syx"),
    )
    .unwrap();
    h.get_by_label("Rescan").click();
    h.run();
    h.get_by_label("Another.syx");
    // Demo Set is still selected, so its patches are still listed.
    h.get_by_label("Warm Pad");
}

#[test]
fn preferences_change_the_look_live() {
    let lib = sample_library();
    let mut h = harness(lib.path());
    h.run();
    open_editor(&mut h);
    let accent = |h: &Harness<'_, App>| h.ctx.global_style().visuals.selection.bg_fill;
    let body = |h: &Harness<'_, App>| h.ctx.global_style().text_styles[&egui::TextStyle::Body].size;
    let default_accent = accent(&h);
    assert_eq!(body(&h), 14.0);

    h.get_by_label("File").click();
    h.run();
    h.get_by_label_contains("Preferences").click();
    h.run();
    h.get_by_label("Teal").click();
    h.run();
    assert_eq!(accent(&h), egui::Color32::from_rgb(0x12, 0xA1, 0x8F));
    h.get_by_label("130%").click();
    h.run();
    assert!((body(&h) - 14.0 * 1.3).abs() < 0.01);
    shot(&mut h, "15-preferences");

    h.get_by_label("Reset to Defaults").click();
    h.run();
    assert_eq!(accent(&h), default_accent);
    assert_eq!(body(&h), 14.0);

    h.get_by_label("Orange").click();
    h.run();
    h.get_by_label("Light").click();
    h.run();
    shot(&mut h, "16-preferences-light-orange");
    h.get_by_label("Done").click();
    h.run();
    assert!(
        h.query_by_label("Reset to Defaults").is_none(),
        "window closed"
    );
}

#[test]
fn preview_a_library_patch() {
    let lib = sample_library();
    let mut h = harness(lib.path());
    h.run();
    h.get_by_label("Patch Sets").click();
    h.run();
    h.get_by_label("Demo Set.syx").click();
    h.run();
    h.get_by_label("Flute").click();
    h.run();
    h.get_by_label("Preview").click();
    h.run();
    let (patch, _) = h.state().previewing().expect("a preview was started");
    assert_eq!(patch.name(), "Flute");
}

#[test]
fn editor_preview_plays_until_stopped_or_left() {
    let lib = sample_library();
    let mut h = harness(lib.path());
    h.run();
    open_editor(&mut h);
    h.get_by_label("▶ Play").click();
    h.run();
    let (patch, playing) = h.state().previewing().expect("a preview was started");
    assert_eq!(patch.name(), "Warm Pad");
    assert!(playing);
    shot(&mut h, "9-editor-preview");

    h.get_by_label("■ Stop").click();
    h.run();
    assert!(!h.state().previewing().unwrap().1);

    // Leaving the editor stops a repeating preview.
    h.get_by_label("▶ Play").click();
    h.run();
    h.get_by_label("Patch Library").click();
    h.run();
    assert!(!h.state().previewing().unwrap().1);
}

/// Load the demo set into the Current Patch Set tab.
fn load_demo_set(h: &mut Harness<'_, App>) {
    h.get_by_label("Patch Sets").click();
    h.run();
    h.get_by_label("Demo Set.syx").click();
    h.run();
    h.get_by_label("Load into EWI").click();
    h.run();
    h.get_by_label("OK").click();
    h.run();
}

/// Drag with the mouse from one point to another, holding over the target
/// for `hold_frames` frames (1/60 s each) before letting go.
fn drag(h: &mut Harness<'_, App>, from: egui::Pos2, to: egui::Pos2, hold_frames: usize) {
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    h.hover_at(from);
    h.step();
    h.event(button(from, true));
    h.step();
    for i in 1..=10 {
        let pos = from + (to - from) * (i as f32 / 10.0);
        h.event(egui::Event::PointerMoved(pos));
        h.step();
    }
    for _ in 0..hold_frames {
        h.event(egui::Event::PointerMoved(to));
        h.step();
    }
    h.event(button(to, false));
    h.step();
    h.run();
}

fn centre(h: &Harness<'_, App>, label: &str) -> egui::Pos2 {
    h.get_by_label(label).rect().center()
}

#[test]
fn drag_swaps_and_holding_offers_to_copy() {
    let lib = sample_library();
    let mut h = harness(lib.path());
    h.run();
    load_demo_set(&mut h);

    // A quick drop swaps: Sax Lead (#0) and Warm Pad (#1) change places.
    let (sax, pad) = (centre(&h, "Sax Lead"), centre(&h, "Warm Pad"));
    drag(&mut h, sax, pad, 2);
    assert!(h.query_by_label("Copy or Swap?").is_none());
    assert!(centre(&h, "Warm Pad").y < centre(&h, "Sax Lead").y);

    // Holding over the target offers to copy instead.
    let (flute, brass) = (centre(&h, "Flute"), centre(&h, "Brass Stab"));
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    h.hover_at(flute);
    h.step();
    h.event(button(flute, true));
    h.step();
    for i in 1..=10 {
        h.event(egui::Event::PointerMoved(
            flute + (brass - flute) * (i as f32 / 10.0),
        ));
        h.step();
    }
    h.get_by_label("Release to swap · hold to copy");
    for _ in 0..70 {
        h.event(egui::Event::PointerMoved(brass));
        h.step();
    }
    h.get_by_label("Release to copy or swap");
    shot(&mut h, "15-hold-to-copy");
    h.event(button(brass, false));
    h.step();
    h.run();
    h.get_by_label("Copy or Swap?");
    shot(&mut h, "16-copy-or-swap");
    h.get_by_label("Copy over #3").click();
    h.run();
    assert_eq!(h.query_all_by_label("Flute").count(), 2);
    assert!(h.query_by_label("Brass Stab").is_none());
}

#[test]
fn held_drop_can_still_swap_or_cancel() {
    let lib = sample_library();
    let mut h = harness(lib.path());
    h.run();
    load_demo_set(&mut h);
    let (synth, oboe) = (centre(&h, "Synth Bass"), centre(&h, "Oboe"));
    drag(&mut h, synth, oboe, 70);
    h.get_by_label("Cancel").click();
    h.run();
    assert!(
        centre(&h, "Synth Bass").y < centre(&h, "Oboe").y,
        "unchanged"
    );
    drag(&mut h, synth, oboe, 70);
    h.get_by_label("Swap").click();
    h.run();
    assert!(centre(&h, "Oboe").y < centre(&h, "Synth Bass").y, "swapped");
}

#[test]
fn select_and_delete_empties_the_slot() {
    let lib = sample_library();
    let mut h = harness(lib.path());
    h.run();
    load_demo_set(&mut h);
    assert!(h.query_by_label("<Empty>").is_none());

    // Delete button, after confirming.
    h.get_by_label("Oboe").click();
    h.run();
    h.get_by_label("Delete").click();
    h.run();
    h.get_by_label("Delete Patch");
    shot(&mut h, "17-delete-patch");
    h.get_by_label("OK").click();
    h.run();
    assert!(h.query_by_label("Oboe").is_none());
    assert_eq!(h.query_all_by_label("<Empty>").count(), 1);

    // The Delete key does the same; Cancel keeps the patch.
    h.get_by_label("Synth Bass").click();
    h.run();
    h.key_press(egui::Key::Delete);
    h.run();
    h.get_by_label("Cancel").click();
    h.run();
    h.get_by_label("Synth Bass");
    h.key_press(egui::Key::Delete);
    h.run();
    h.get_by_label("OK").click();
    h.run();
    assert_eq!(h.query_all_by_label("<Empty>").count(), 2);

    // Saving the set leaves the empty slots out.
    h.get_by_label("Save to Library…").click();
    h.run();
    h.get_by_role(Role::TextInput).type_text("Trimmed");
    h.run();
    h.get_by_label("OK").click();
    h.run();
    let saved = patch_set::load_syx(&lib.path().join("Trimmed.syx")).unwrap();
    assert_eq!(saved.len(), NUM_PATCHES - 2);
}

#[test]
fn double_click_opens_patch_actions() {
    let lib = sample_library();
    let mut h = harness(lib.path());
    h.run();
    load_demo_set(&mut h);
    let pos = centre(&h, "Brass Stab");
    double_click(&mut h, pos);
    shot(&mut h, "18-double-click-slot");
    h.get_by_label("Paste from Patch Library");
    h.get_by_label("Patch #3");
}

#[test]
fn about_box_shows_icon_and_coffee_link() {
    let lib = sample_library();
    let mut h = harness(lib.path());
    h.run();
    h.get_by_label("Help").click();
    h.run();
    h.get_by_label("About EWITool2").click();
    h.run();
    h.get_by_label("Support this project: buy me a coffee");
    shot(&mut h, "about");
    h.get_by_label("OK").click();
    h.run();
    assert!(h
        .query_by_label("Support this project: buy me a coffee")
        .is_none());
}
