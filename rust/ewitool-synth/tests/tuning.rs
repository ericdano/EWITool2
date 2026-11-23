//! The measured mappings in `tuning`, checked against the EWI recordings
//! they came from (2026-11-23).

use ewitool_synth::tuning as t;

#[test]
fn filter_lfo_rate() {
    for (v, hz) in [
        (8u8, 0.44f32),
        (16, 1.45),
        (24, 3.15), // recorded 3.14-3.15 Hz
        (32, 5.5),
        (48, 12.2),
        (64, 21.7),
        (96, 48.6),
    ] {
        let got = t::lfo_hz(v);
        assert!(
            (got / hz - 1.0).abs() < 0.03,
            "rate {v}: {got} Hz, recorded {hz}"
        );
    }
}

#[test]
fn lfo_is_a_triangle() {
    assert_eq!(t::lfo_wave(0.0), -1.0);
    assert_eq!(t::lfo_wave(0.25), 0.0);
    assert_eq!(t::lfo_wave(0.5), 1.0);
    assert_eq!(t::lfo_wave(0.75), 0.0);
}

/// Peak-to-peak swing in semitones, as the analysis reports it (it reads
/// 9% low; see `filter_lfo_semis`).
fn swing(depth: u8, lfo_breath: u8, thr: u8, breath: f32) -> f32 {
    2.0 * t::filter_lfo_semis(t::filter_lfo_depth(depth, lfo_breath, thr, breath)) / 1.09
}

#[test]
fn filter_lfo_depth_matches_recordings() {
    // (depth, LFO breath, threshold, breath, recorded swing)
    let takes = [
        (16u8, 64u8, 0u8, 0.9f32, 5.5f32),
        (64, 64, 0, 0.9, 21.5),
        (16, 64, 127, 0.3, 5.5),
        (16, 0, 0, 0.6, 0.0),
        (16, 32, 0, 0.6, 2.5),
        (16, 96, 0, 0.3, 11.0),
        (16, 96, 0, 0.9, 22.2),
        (4, 127, 0, 0.3, 13.8),
        (32, 127, 0, 0.9, 38.5),
        (16, 127, 64, 0.6, 12.2),
        (16, 127, 96, 0.6, 5.5),
        (16, 127, 96, 0.9, 26.0),
        (16, 0, 64, 0.3, 2.2),
        (16, 32, 96, 0.3, 4.5),
        (16, 32, 96, 0.6, 3.5),
    ];
    for (d, lb, thr, b, want) in takes {
        let got = swing(d, lb, thr, b);
        assert!(
            (got - want).abs() < 2.5,
            "depth {d} lfo-breath {lb} thr {thr} breath {b}: {got:.1} vs {want}"
        );
    }
}
