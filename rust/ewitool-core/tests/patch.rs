//! Port of EWI4000sPatchTest.java, plus a field-offset table check.

mod common;

use common::{sample_patch, JavaRandom};
use ewitool_core::patch::{internal_to_user, user_to_internal};
use ewitool_core::{Error, Mode, Patch, PATCH_LEN};

#[test]
fn java_random_matches_jdk() {
    // Well-known first outputs of java.util.Random.
    assert_eq!(JavaRandom::new(42).next_int(), -1170105035);
    assert_eq!(JavaRandom::new(0).next_int(), -1155484576);
}

#[test]
fn decode_then_encode_round_trips() {
    for seed in 0..500 {
        let original = sample_patch("Round Trip", seed);
        let patch = Patch::from_bytes(&original).unwrap();
        assert_eq!(patch.to_bytes(), original, "seed {seed}");
    }
}

#[test]
fn round_trips_arbitrary_bytes() {
    // Even garbage (wrong header/trailer) must survive unchanged.
    let mut blob = [0u8; PATCH_LEN];
    JavaRandom::new(7).next_bytes(&mut blob);
    assert_eq!(Patch::decode(&blob).to_bytes(), blob);
}

/// Each field must come from the same byte the Java `decodeBlob()` reads.
/// Fill byte i with value i, then check every field against Java's offsets.
#[test]
fn fields_match_java_offsets() {
    let blob: [u8; PATCH_LEN] = std::array::from_fn(|i| i as u8);
    let p = Patch::decode(&blob);

    assert_eq!(p.header, [0, 1, 2, 3]);
    assert_eq!(p.mode, 4);
    assert_eq!(p.internal_num, 5);
    assert_eq!(p.fillers, [6, 7, 8, 9, 10, 11]);
    assert_eq!(p.name_bytes[0], 12);
    assert_eq!(p.name_bytes[31], 43);

    // Osc: patchBlob[44..65] and [65..86]; field k at base + k.
    for (osc, base) in [(&p.osc1, 44u8), (&p.osc2, 65)] {
        let fields = [
            osc.nrpn.lsb,
            osc.nrpn.msb,
            osc.nrpn.offset,
            osc.octave,
            osc.semitone,
            osc.fine,
            osc.beat,
            osc.filler1,
            osc.sawtooth,
            osc.triangle,
            osc.square,
            osc.pulse_width,
            osc.pwm_freq,
            osc.pwm_depth,
            osc.sweep_depth,
            osc.sweep_time,
            osc.breath_depth,
            osc.breath_attain,
            osc.breath_curve,
            osc.breath_threshold,
            osc.level,
        ];
        let expected: Vec<u8> = (base..base + 21).collect();
        assert_eq!(fields.to_vec(), expected);
    }

    // Filters: [86..101], [101..116], [116..131], [131..146].
    for (f, base) in [
        (&p.osc_filter1, 86u8),
        (&p.osc_filter2, 101),
        (&p.noise_filter1, 116),
        (&p.noise_filter2, 131),
    ] {
        let fields = [
            f.nrpn.lsb,
            f.nrpn.msb,
            f.nrpn.offset,
            f.mode,
            f.freq,
            f.q,
            f.key_follow,
            f.breath_mod,
            f.lfo_freq,
            f.lfo_depth,
            f.lfo_breath,
            f.lfo_threshold,
            f.sweep_depth,
            f.sweep_time,
            f.breath_curve,
        ];
        let expected: Vec<u8> = (base..base + 15).collect();
        assert_eq!(fields.to_vec(), expected);
    }

    assert_eq!(p.anti_alias.nrpn.lsb, 146);
    assert_eq!(
        [
            p.anti_alias.switch,
            p.anti_alias.cutoff,
            p.anti_alias.key_follow
        ],
        [149, 150, 151]
    );
    assert_eq!(p.noise.nrpn.lsb, 152);
    assert_eq!(
        [p.noise.time, p.noise.breath, p.noise.level],
        [155, 156, 157]
    );
    assert_eq!(p.misc.nrpn.lsb, 158);
    let m = &p.misc;
    assert_eq!(
        [
            m.bend_range,
            m.bend_step_mode,
            m.bite_vibrato,
            m.osc_filter_link,
            m.noise_filter_link,
            m.formant_filter,
            m.osc2_xfade,
            m.key_trigger,
            m.filler10,
            m.chorus_switch,
        ],
        [161, 162, 163, 164, 165, 166, 167, 168, 169, 170]
    );
    assert_eq!(p.amp.nrpn.lsb, 171);
    assert_eq!(
        [p.amp.bite_tremolo, p.amp.level, p.amp.octave_level],
        [174, 175, 176]
    );
    let c = &p.chorus;
    assert_eq!(c.nrpn.lsb, 177);
    assert_eq!(
        [
            c.delay1,
            c.mod_lev1,
            c.wet_lev1,
            c.delay2,
            c.mod_lev2,
            c.wet_lev2,
            c.feedback,
            c.lfo_freq,
            c.dry_level,
        ],
        [180, 181, 182, 183, 184, 185, 186, 187, 188]
    );
    let d = &p.delay;
    assert_eq!(d.nrpn.lsb, 189);
    assert_eq!(
        [d.time, d.feedback, d.damp, d.level, d.dry],
        [192, 193, 194, 195, 196]
    );
    let r = &p.reverb;
    assert_eq!(r.nrpn.lsb, 197);
    assert_eq!(
        [r.dry, r.level, r.density, r.time, r.damp],
        [200, 201, 202, 203, 204]
    );
    assert_eq!(p.trailer, 205);
}

#[test]
fn editing_a_field_changes_only_its_byte() {
    let original = sample_patch("Edit", 9);
    let mut p = Patch::from_bytes(&original).unwrap();
    p.reverb.level = original[201].wrapping_add(1);
    let out = p.to_bytes();
    for i in 0..PATCH_LEN {
        if i == 201 {
            assert_ne!(out[i], original[i]);
        } else {
            assert_eq!(out[i], original[i], "byte {i} changed");
        }
    }
}

#[test]
fn decodes_name_and_header_fields() {
    let mut blob = sample_patch("Sax Lead", 1);
    blob[5] = 17;
    let p = Patch::from_bytes(&blob).unwrap();
    assert_eq!(p.name(), "Sax Lead");
    assert_eq!(p.internal_num, 17);
    assert_eq!(p.user_num(), Some(18));
}

#[test]
fn set_name_pads_and_writes_blob() {
    let mut p = Patch::from_bytes(&sample_patch("Old", 2)).unwrap();
    p.set_name("New").unwrap();
    assert_eq!(p.name(), "New");
    let blob = p.to_bytes();
    assert_eq!(&blob[12..44], format!("New{}", " ".repeat(29)).as_bytes());
}

#[test]
fn set_name_rejects_empty_too_long_and_non_ascii() {
    let mut p = Patch::from_bytes(&sample_patch("Keep", 3)).unwrap();
    assert!(matches!(p.set_name(""), Err(Error::InvalidName(_))));
    assert!(matches!(
        p.set_name(&"x".repeat(33)),
        Err(Error::InvalidName(_))
    ));
    assert!(matches!(p.set_name("Café"), Err(Error::InvalidName(_))));
    assert_eq!(p.name(), "Keep");
    assert!(p.set_name(&"x".repeat(32)).is_ok());
}

#[test]
fn set_internal_num_and_mode_update_blob() {
    let mut p = Patch::from_bytes(&sample_patch("P", 4)).unwrap();
    p.internal_num = 99;
    p.set_mode(Mode::Edit);
    let blob = p.to_bytes();
    assert_eq!(blob[5], 99);
    assert_eq!(blob[4], 0x20);
}

#[test]
fn patch_numbering() {
    assert_eq!(user_to_internal(0), Some(99));
    assert_eq!(user_to_internal(1), Some(0));
    assert_eq!(user_to_internal(99), Some(98));
    assert_eq!(user_to_internal(100), None);
    for user in 0..100 {
        assert_eq!(
            internal_to_user(user_to_internal(user).unwrap()),
            Some(user)
        );
    }
    assert_eq!(internal_to_user(100), None);
}

#[test]
fn hex_round_trips() {
    let blob = sample_patch("Hex", 5);
    let p = Patch::from_bytes(&blob).unwrap();
    let hex = p.to_hex(false);
    let expected: String = blob.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, expected);
    assert_eq!(Patch::from_hex(&hex).unwrap().to_bytes(), blob);
    assert_eq!(Patch::from_hex(&p.to_hex(true)).unwrap().to_bytes(), blob);
}

#[test]
fn hex_with_spaces() {
    let hex = Patch::from_bytes(&sample_patch("S", 6))
        .unwrap()
        .to_hex(true);
    assert!(hex.starts_with("f0 47 64 7f "));
    assert_eq!(hex.len(), PATCH_LEN * 3);
}

#[test]
fn rejects_wrong_length_and_bad_hex() {
    assert!(matches!(
        Patch::from_bytes(&[0; 10]),
        Err(Error::WrongLength { actual: 10, .. })
    ));
    assert!(matches!(
        Patch::from_bytes(&[0; 300]),
        Err(Error::WrongLength { .. })
    ));
    assert!(matches!(Patch::from_hex("zz"), Err(Error::BadHex)));
    assert!(matches!(Patch::from_hex("abc"), Err(Error::BadHex)));
}
