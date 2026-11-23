use ewitool_core::{Patch, PATCH_LEN};
use ewitool_synth::analysis::features;
use ewitool_synth::{render_phrase, Phrase, Synth};

const SR: f32 = 48000.0;

/// A plain patch: oscillator 1 only, filters off, no effects, every bipolar
/// value at its centre.
fn plain() -> Patch {
    let mut p = Patch::decode(&[0; PATCH_LEN]);
    for o in [&mut p.osc1, &mut p.osc2] {
        o.octave = 64;
        o.semitone = 64;
        o.fine = 64;
        o.beat = 64;
        o.sweep_depth = 64;
        o.breath_depth = 64;
        o.breath_curve = 64;
        o.pulse_width = 64;
    }
    p.osc1.sawtooth = 127;
    p.osc1.level = 127;
    for f in [
        &mut p.osc_filter1,
        &mut p.osc_filter2,
        &mut p.noise_filter1,
        &mut p.noise_filter2,
    ] {
        f.mode = 4;
        f.sweep_depth = 64;
        f.lfo_breath = 64;
        f.breath_curve = 64;
        f.key_follow = 64;
        f.freq = 100;
        f.q = 5;
    }
    p.amp.level = 127;
    p.delay.dry = 127;
    p.reverb.dry = 127;
    p.reverb.time = 20;
    p.reverb.damp = 60;
    p
}

/// Left channel of a held note, skipping the attack.
fn hold(p: &Patch, note: u8, breath: f32, seconds: f32) -> Vec<f32> {
    let mut s = Synth::new(SR);
    s.set_patch(p);
    s.note_on(note);
    s.set_breath(breath);
    let mut warm = vec![0.0; (SR * 0.2) as usize * 2];
    s.render(&mut warm);
    let mut out = vec![0.0; (SR * seconds) as usize * 2];
    s.render(&mut out);
    out.iter().step_by(2).copied().collect()
}

/// Pitch from rising zero crossings (good for a bare sawtooth).
fn pitch(x: &[f32]) -> f32 {
    let mean = x.iter().sum::<f32>() / x.len() as f32;
    let crossings: Vec<usize> = x
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[0] - mean < 0.0 && w[1] - mean >= 0.0)
        .map(|(i, _)| i)
        .collect();
    let (first, last) = (crossings[0], *crossings.last().unwrap());
    SR * (crossings.len() - 1) as f32 / (last - first) as f32
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

/// Share of energy in sample-to-sample changes: higher = brighter.
fn brightness(x: &[f32]) -> f32 {
    let d: Vec<f32> = x.windows(2).map(|w| w[1] - w[0]).collect();
    rms(&d) / rms(x)
}

#[test]
fn a440_plays_at_440_hz() {
    let f = pitch(&hold(&plain(), 69, 0.8, 0.5));
    assert!((f - 440.0).abs() < 1.0, "{f}");
}

#[test]
fn octave_semitone_and_fine_tune() {
    let mut p = plain();
    p.osc1.octave = 65;
    let f = pitch(&hold(&p, 57, 0.8, 0.5));
    assert!((f - 440.0).abs() < 1.0, "octave: {f}");

    let mut p = plain();
    p.osc1.semitone = 71; // +7
    let f = pitch(&hold(&p, 62, 0.8, 0.5));
    assert!((f - 440.0).abs() < 1.0, "semitone: {f}");

    let mut p = plain();
    p.osc1.fine = 114; // +50 cents
    let f = pitch(&hold(&p, 69, 0.8, 0.5));
    let want = 440.0 * 2f32.powf(0.5 / 12.0);
    assert!((f - want).abs() < 1.5, "fine: {f} want {want}");
}

/// Pulse width checked the way it was measured on the EWI: a pulse of duty
/// 1/n has no energy at harmonic n.
#[test]
fn pulse_width_sets_duty_cycle() {
    // 0 is a square (no 2nd harmonic), 63 a quarter (no 4th), 95 an eighth
    // (no 8th).
    for (pw, null) in [(0u8, 2usize), (63, 4), (95, 8)] {
        let mut p = plain();
        p.osc1.sawtooth = 0;
        p.osc1.square = 127;
        p.osc1.pulse_width = pw;
        // Moderate breath keeps it clear of the output limiter.
        let f = features(&hold(&p, 45, 0.5, 0.5), SR, 110.0);
        let at_null = f.harmonics_db[null - 1];
        let before = f.harmonics_db[null - 2];
        assert!(at_null < before - 20.0, "pw {pw}: {:?}", f.harmonics_db);
    }
}

#[test]
fn no_breath_is_silent_and_more_breath_is_louder() {
    let p = plain();
    assert!(rms(&hold(&p, 60, 0.0, 0.3)) < 1e-4);
    let soft = rms(&hold(&p, 60, 0.3, 0.3));
    let loud = rms(&hold(&p, 60, 0.9, 0.3));
    assert!(soft > 0.01 && loud > soft * 1.5, "{soft} {loud}");
}

#[test]
fn low_pass_cutoff_darkens() {
    let mut p = plain();
    p.osc_filter1.mode = 0;
    p.osc_filter1.freq = 120;
    let open = brightness(&hold(&p, 48, 1.0, 0.3));
    p.osc_filter1.freq = 70;
    let closed = brightness(&hold(&p, 48, 1.0, 0.3));
    assert!(closed < open * 0.5, "open {open}, closed {closed}");
}

#[test]
fn breath_opens_the_filter() {
    let mut p = plain();
    p.osc_filter1.mode = 0;
    p.osc_filter1.freq = 110;
    p.osc_filter1.breath_mod = 100;
    let soft = brightness(&hold(&p, 48, 0.2, 0.3));
    let hard = brightness(&hold(&p, 48, 1.0, 0.3));
    assert!(hard > soft * 1.5, "soft {soft}, hard {hard}");
}

#[test]
fn phrase_renders_and_rings_out() {
    let mut p = plain();
    p.reverb.level = 127;
    let phrase = Phrase::default();
    let out = render_phrase(&p, &phrase, 1.0, SR);
    let frames = out.len() / 2;
    assert_eq!(frames, ((phrase.duration() + 1.0) * SR) as usize);
    // Reverb tail after the note has ended.
    let end = (phrase.duration() * SR) as usize * 2;
    let tail = rms(&out[end + 4800..end + 9600]);
    assert!(tail > 1e-4, "{tail}");
}

#[test]
fn random_patches_stay_finite_and_bounded() {
    let mut rng = 12345u32;
    for _ in 0..40 {
        let mut bytes = [0u8; PATCH_LEN];
        for b in &mut bytes {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            *b = (rng % 128) as u8;
        }
        let p = Patch::decode(&bytes);
        let out = render_phrase(&p, &Phrase::default(), 0.3, 22050.0);
        assert!(out.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
    }
}
