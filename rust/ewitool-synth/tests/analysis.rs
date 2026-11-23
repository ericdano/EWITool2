use ewitool_synth::analysis::{cents, envelope_db, features, onset, pitch};

const SR: f32 = 48000.0;

fn saw(hz: f32, seconds: f32) -> Vec<f32> {
    // Additive (band-limited) sawtooth: harmonic k at 1/k.
    (0..(SR * seconds) as usize)
        .map(|i| {
            let t = i as f32 / SR;
            (1..=30)
                .filter(|k| *k as f32 * hz < SR / 2.0)
                .map(|k| (std::f32::consts::TAU * k as f32 * hz * t).sin() / k as f32)
                .sum::<f32>()
                * 0.5
        })
        .collect()
}

#[test]
fn pitch_is_found_within_a_cent() {
    for hz in [110.0, 261.63, 440.0, 987.77] {
        let f = pitch(&saw(hz, 0.3), SR, hz * 1.05).unwrap();
        assert!(cents(f, hz).abs() < 1.0, "{hz}: {f}");
    }
}

#[test]
fn saw_harmonics_fall_at_6_db_per_octave() {
    let f = features(&saw(220.0, 0.5), SR, 220.0);
    for (k, want) in [(2, -6.02), (4, -12.04), (8, -18.06)] {
        let got = f.harmonics_db[k - 1];
        assert!((got - want).abs() < 0.5, "harmonic {k}: {got}");
    }
    assert!(f.centroid_hz > 220.0 * 1.5);
}

#[test]
fn onset_and_envelope() {
    let mut x = vec![0.0001f32; (SR * 0.5) as usize];
    x.extend(saw(330.0, 0.5));
    let at = onset(&x, SR, 0.2, 20.0).unwrap();
    assert!((at as f32 - SR * 0.5).abs() < SR * 0.005, "{at}");
    let env = envelope_db(&x, SR, 0.1);
    assert_eq!(env.len(), 10);
    assert!(env[2] < -60.0 && env[7] > -20.0);
}
