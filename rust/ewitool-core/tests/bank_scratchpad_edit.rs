//! Tests for the patch bank, scratchpad persistence and edit operations.

mod common;

use common::{sample_patch, TempDir};
use ewitool_core::edit;
use ewitool_core::patch::user_to_internal;
use ewitool_core::scratchpad::SCRATCHPAD_NAME;
use ewitool_core::{patch_set, Bank, Error, Patch, ScratchPad, NUM_PATCHES};

fn patch(name: &str, seed: i64) -> Patch {
    Patch::from_bytes(&sample_patch(name, seed)).unwrap()
}

/// A patch with the given internal number.
fn numbered(name: &str, internal: u8) -> Patch {
    let mut p = patch(name, internal as i64);
    p.internal_num = internal;
    p
}

// ---- Bank ----

#[test]
fn bank_places_patches_by_internal_number() {
    // Internal 99 is the patch the player sees as #0.
    let bank = Bank::from_patches([
        numbered("Zero", 99),
        numbered("One", 0),
        numbered("Fifty", 49),
    ]);
    assert_eq!(bank.name(0), "Zero");
    assert_eq!(bank.name(1), "One");
    assert_eq!(bank.name(50), "Fifty");
    assert_eq!(bank.name(2), "<Empty>");
    assert_eq!(bank.len(), 3);
}

#[test]
fn clearing_a_slot_leaves_it_empty() {
    let mut bank = Bank::from_patches([numbered("Zero", 99), numbered("One", 0)]);
    let removed = bank.clear(1).unwrap();
    assert_eq!(removed.name(), "One");
    assert!(bank.get(1).is_none());
    assert_eq!(bank.name(1), ewitool_core::bank::EMPTY_NAME);
    assert_eq!(bank.len(), 1);
    assert!(bank.clear(1).is_none(), "already empty");
    assert!(bank.clear(500).is_none(), "out of range");
}

#[test]
fn blank_patch_is_a_valid_default_sound_named_empty() {
    let template = patch("Lead", 7);
    let blank = edit::blank_patch(&template);
    assert_eq!(blank.name(), ewitool_core::bank::EMPTY_NAME);
    // Same framing (header, NRPN prefixes, number) as the template ...
    assert_eq!(blank.header, template.header);
    assert_eq!(blank.osc1.nrpn, template.osc1.nrpn);
    assert_eq!(blank.internal_num, template.internal_num);
    // ... with the default sound.
    let mut expected = template.clone();
    edit::default_patch(&mut expected);
    expected.set_name(ewitool_core::bank::EMPTY_NAME).unwrap();
    assert_eq!(blank, expected);
    // And it survives the sysex round trip.
    assert_eq!(Patch::from_bytes(&blank.to_bytes()).unwrap(), blank);
}

#[test]
fn bank_round_trips_a_saved_set() {
    let set: Vec<Patch> = (0..NUM_PATCHES as u8)
        .map(|i| numbered(&format!("P{i}"), user_to_internal(i).unwrap()))
        .collect();
    let bank = Bank::from_patches(set.clone());
    assert_eq!(bank.len(), NUM_PATCHES);
    assert_eq!(bank.patches(), set);
}

#[test]
fn bank_puts_duplicate_and_invalid_numbers_in_free_slots() {
    let bank = Bank::from_patches([numbered("A", 0), numbered("Dup", 0), numbered("Bad", 200)]);
    assert_eq!(bank.name(1), "A");
    assert_eq!(bank.name(0), "Dup");
    assert_eq!(bank.name(2), "Bad");
    for slot in 0..3 {
        assert_eq!(
            bank.get(slot).unwrap().internal_num,
            user_to_internal(slot as u8).unwrap()
        );
    }
}

#[test]
fn bank_swap_renumbers() {
    let mut bank = Bank::from_patches([numbered("Zero", 99), numbered("Five", 4)]);
    bank.swap(0, 5);
    assert_eq!(bank.name(0), "Five");
    assert_eq!(bank.get(0).unwrap().internal_num, 99);
    assert_eq!(bank.name(5), "Zero");
    assert_eq!(bank.get(5).unwrap().internal_num, 4);
    // Swapping with an empty slot moves the patch.
    bank.swap(5, 7);
    assert_eq!(bank.name(5), "<Empty>");
    assert_eq!(bank.get(7).unwrap().internal_num, 6);
}

#[test]
fn bank_set_renumbers() {
    let mut bank = Bank::default();
    bank.set(0, numbered("X", 12));
    assert_eq!(bank.get(0).unwrap().internal_num, 99);
}

// ---- ScratchPad ----

#[test]
fn scratchpad_persists_every_change() {
    let dir = TempDir::new("scratch");
    let mut sp = ScratchPad::load(Some(dir.path())).unwrap();
    assert!(sp.is_empty());
    sp.add(patch("One", 1)).unwrap();
    sp.add(patch("Two", 2)).unwrap();
    sp.rename(0, "Uno").unwrap();
    sp.remove(1).unwrap();

    let reloaded = ScratchPad::load(Some(dir.path())).unwrap();
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded.patches()[0].name(), "Uno");
    let on_disk = std::fs::read(dir.path().join(SCRATCHPAD_NAME)).unwrap();
    assert_eq!(patch_set::parse_syx(&on_disk).unwrap(), reloaded.patches());

    let mut sp = reloaded;
    sp.clear().unwrap();
    assert!(ScratchPad::load(Some(dir.path())).unwrap().is_empty());
}

#[test]
fn scratchpad_replace_persists() {
    let dir = TempDir::new("scratch-replace");
    let mut sp = ScratchPad::load(Some(dir.path())).unwrap();
    sp.add(patch("Before", 1)).unwrap();
    let mut edited = sp.patches()[0].clone();
    edited.reverb.level = 99;
    sp.replace(0, edited.clone()).unwrap();
    assert_eq!(
        ScratchPad::load(Some(dir.path())).unwrap().patches(),
        [edited]
    );
}

#[test]
fn scratchpad_rejects_bad_rename() {
    let dir = TempDir::new("scratch-rename");
    let mut sp = ScratchPad::load(Some(dir.path())).unwrap();
    sp.add(patch("Keep", 1)).unwrap();
    assert!(matches!(sp.rename(0, ""), Err(Error::InvalidName(_))));
    assert_eq!(
        ScratchPad::load(Some(dir.path())).unwrap().patches()[0].name(),
        "Keep"
    );
}

#[test]
fn scratchpad_in_memory_works_without_a_folder() {
    let mut sp = ScratchPad::in_memory();
    sp.add(patch("Mem", 1)).unwrap();
    assert_eq!(sp.len(), 1);
    assert!(!sp.is_persistent());
}

#[test]
fn import_into_another_folder() {
    let src = TempDir::new("import-src");
    let lib = TempDir::new("import-lib");
    let mut bytes = b"BODY\0\0\0\0".to_vec();
    for i in 0..3 {
        bytes.extend_from_slice(&sample_patch(&format!("P{i}"), i));
    }
    let legacy = src.path().join("old.BNK");
    std::fs::write(&legacy, bytes).unwrap();
    let (syx, n) = patch_set::convert_legacy_to(&legacy, lib.path()).unwrap();
    assert_eq!(n, 3);
    assert_eq!(syx, lib.path().join("old.syx"));
    assert_eq!(patch_set::load_syx(&syx).unwrap().len(), 3);
}

// ---- Edit operations ----

#[test]
fn default_patch_is_plain_triangle() {
    let mut p = patch("Default", 1);
    let before = p.clone();
    edit::default_patch(&mut p);
    assert_eq!(p.osc1.triangle, 127);
    assert_eq!(p.osc1.level, 96);
    assert_eq!(p.osc2.level, 0);
    assert_eq!(p.osc_filter1.mode, edit::FILTER_OFF);
    assert_eq!(p.noise_filter2.lfo_threshold, 0);
    assert_eq!(p.amp.level, 96);
    // Identity bytes are untouched.
    assert_eq!(p.name(), before.name());
    assert_eq!(p.header, before.header);
    assert_eq!(p.osc1.nrpn, before.osc1.nrpn);
    assert_eq!(p.trailer, before.trailer);
}

#[test]
fn random_patch_stays_in_range_and_reaches_switch_tops() {
    let mut rng = fastrand::Rng::with_seed(1);
    let mut xfade_on = false;
    let mut chorus_on = false;
    for _ in 0..200 {
        let mut p = patch("Rand", 2);
        edit::random_patch(&mut p, &mut rng);
        assert!(p.osc1.level >= 48 && p.osc1.level <= 127);
        assert!((62..=66).contains(&p.osc2.octave));
        assert!(p.osc_filter1.q >= 5 && p.osc_filter1.q <= 127);
        assert!(p.osc_filter1.mode <= edit::FILTER_OFF);
        assert!(p.reverb.level <= 12);
        assert!(p.misc.formant_filter <= 2);
        xfade_on |= p.misc.osc2_xfade == 1;
        chorus_on |= p.misc.chorus_switch == 1;
    }
    assert!(xfade_on && chorus_on, "switches never turned on");
}

#[test]
fn randomise_by_10pct_stays_close() {
    let mut rng = fastrand::Rng::with_seed(3);
    let mut p = patch("Near", 3);
    edit::default_patch(&mut p);
    let before = p.clone();
    edit::randomise_by_10pct(&mut p, &mut rng);
    for (a, b) in [
        (before.osc1.level, p.osc1.level),
        (before.osc1.triangle, p.osc1.triangle),
        (before.osc_filter1.q, p.osc_filter1.q),
        (before.reverb.dry, p.reverb.dry),
    ] {
        assert!((i32::from(a) - i32::from(b)).abs() <= 12, "{a} -> {b}");
    }
    assert!(p.osc_filter1.q >= 5);
}

#[test]
fn merge_averages() {
    let mut a = patch("A", 1);
    let mut b = patch("B", 2);
    a.osc2.octave = 62;
    b.osc2.octave = 66;
    a.reverb.time = 10;
    b.reverb.time = 30;
    a.osc_filter1.q = 20;
    b.osc_filter1.q = 40;
    let switch_before = a.misc.chorus_switch;
    edit::merge_with(&mut a, &b);
    assert_eq!(a.osc2.octave, 64);
    assert_eq!(a.reverb.time, 20);
    assert_eq!(a.osc_filter1.q, 30);
    assert_eq!(a.misc.chorus_switch, switch_before);
    assert_eq!(a.name(), "A");
}

#[test]
fn process_operations() {
    let mut p = patch("Proc", 4);
    edit::make_dry(&mut p);
    assert_eq!((p.misc.chorus_switch, p.chorus.dry_level), (0, 127));
    assert_eq!(
        (p.delay.dry, p.delay.level, p.reverb.dry, p.reverb.level),
        (127, 0, 127, 0)
    );

    p.osc1.level = 50;
    p.osc2.level = 100;
    p.amp.level = 20;
    p.amp.octave_level = 0;
    edit::maximise_volume(&mut p);
    assert_eq!(p.osc2.level, 127);
    assert_eq!(p.osc1.level, 63);
    assert_eq!(p.amp.level, 25);
    assert_eq!(p.amp.octave_level, 0);

    edit::remove_noise(&mut p);
    assert_eq!((p.noise.time, p.noise.breath, p.noise.level), (0, 0, 0));
    assert_eq!(p.noise_filter1.mode, edit::FILTER_OFF);
    assert_eq!(p.noise_filter2.mode, edit::FILTER_OFF);
}

#[test]
fn maximise_volume_handles_silence() {
    let mut p = patch("Silent", 5);
    p.osc1.level = 0;
    p.osc2.level = 0;
    p.amp.level = 0;
    p.amp.octave_level = 0;
    edit::maximise_volume(&mut p);
    assert_eq!(p.osc1.level, 0);
}
