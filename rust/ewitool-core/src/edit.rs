//! Whole-patch operations from the editor's Patch menu (Generate / Process).
//!
//! Ported from `PatchEditorTab.java`, with these deliberate fixes:
//! - Random values include the top of their range. Java's `randBetween`
//!   never returned `max`, so cross-fade, multi key-trigger and chorus were
//!   never switched on by "Random Patch".
//! - "Random Patch" randomises Osc 2's octave and "Merge" mixes Osc 2's
//!   octave/semitone; Java changed Osc 1's twice instead.
//! - "Default Patch" resets the secondary noise filter's LFO threshold
//!   (Java reset the primary one twice).

use crate::patch::{Filter, Osc, Patch};

/// Filter mode value meaning "Off".
pub const FILTER_OFF: u8 = 4;

fn rand_between(rng: &mut fastrand::Rng, min: u8, max: u8) -> u8 {
    rng.u8(min..=max)
}

/// Current value moved randomly by up to 10% of `max`, kept within bounds.
fn rand_near(rng: &mut fastrand::Rng, min: u8, max: u8, cur: u8) -> u8 {
    let step = i32::from(max) / 10;
    let (min, max, cur) = (i32::from(min), i32::from(max), i32::from(cur));
    let lo = (cur - step).clamp(min, max);
    let hi = (cur + step).clamp(min, max);
    rng.i32(lo.min(hi)..=hi.max(lo)) as u8
}

/// Average of two values (Java `mixInts(a, 50, b)`).
fn mix(a: u8, b: u8) -> u8 {
    a / 2 + b / 2
}

/// A plain triangle wave at 75% volume, no filtering or effects.
pub fn default_patch(p: &mut Patch) {
    for (osc, level, triangle) in [(&mut p.osc1, 96, 127), (&mut p.osc2, 0, 0)] {
        *osc = Osc {
            nrpn: osc.nrpn,
            filler1: osc.filler1,
            octave: 64,
            semitone: 64,
            fine: 64,
            beat: 64,
            sawtooth: 0,
            triangle,
            square: 0,
            pulse_width: 64,
            pwm_freq: 0,
            pwm_depth: 0,
            sweep_depth: 64,
            sweep_time: 0,
            breath_depth: 0,
            breath_attain: 0,
            breath_curve: 64,
            breath_threshold: 0,
            level,
        };
    }
    p.misc.osc2_xfade = 0;
    p.misc.formant_filter = 0;
    p.misc.key_trigger = 1;
    p.misc.osc_filter_link = 0;

    for f in [
        &mut p.osc_filter1,
        &mut p.osc_filter2,
        &mut p.noise_filter1,
        &mut p.noise_filter2,
    ] {
        *f = Filter {
            nrpn: f.nrpn,
            mode: FILTER_OFF,
            freq: 0,
            q: 5,
            key_follow: 64,
            breath_mod: 0,
            lfo_freq: 0,
            lfo_depth: 0,
            lfo_breath: 64,
            lfo_threshold: 0,
            sweep_depth: 64,
            sweep_time: 0,
            breath_curve: 64,
        };
    }

    p.noise.time = 0;
    p.noise.breath = 0;
    p.noise.level = 0;

    let c = &mut p.chorus;
    (c.delay1, c.mod_lev1, c.wet_lev1) = (0, 0, 0);
    (c.delay2, c.mod_lev2, c.wet_lev2) = (0, 0, 0);
    (c.dry_level, c.feedback, c.lfo_freq) = (127, 0, 0);
    p.misc.chorus_switch = 0;

    let d = &mut p.delay;
    (d.time, d.damp, d.feedback, d.dry, d.level) = (0, 0, 0, 127, 0);
    let r = &mut p.reverb;
    (r.damp, r.density, r.dry, r.level, r.time) = (0, 0, 127, 0, 0);

    p.amp.bite_tremolo = 0;
    p.misc.bite_vibrato = 127;
    p.amp.level = 96; // 75%
    p.amp.octave_level = 36; // seems to be the device default
}

/// The patch stored in an EWI slot that EWITool2 shows as empty (the EWI
/// can't leave a slot empty): the default sound, named
/// [`crate::bank::EMPTY_NAME`]. `template` supplies the sysex framing and
/// filler bytes; its sound and name are replaced.
pub fn blank_patch(template: &Patch) -> Patch {
    let mut p = template.clone();
    default_patch(&mut p);
    p.set_name(crate::bank::EMPTY_NAME)
        .expect("EMPTY_NAME is a valid patch name");
    p
}

fn randomise_osc(rng: &mut fastrand::Rng, o: &mut Osc, min_level: u8) {
    for v in [
        &mut o.fine,
        &mut o.beat,
        &mut o.sawtooth,
        &mut o.triangle,
        &mut o.square,
        &mut o.pulse_width,
        &mut o.pwm_depth,
        &mut o.pwm_freq,
        &mut o.sweep_depth,
        &mut o.sweep_time,
        &mut o.breath_attain,
        &mut o.breath_depth,
        &mut o.breath_curve,
        &mut o.breath_threshold,
    ] {
        *v = rand_between(rng, 0, 127);
    }
    o.level = rand_between(rng, min_level, 127);
}

fn randomise_filter(rng: &mut fastrand::Rng, f: &mut Filter) {
    f.mode = rand_between(rng, 0, FILTER_OFF);
    for v in filter_values(f) {
        *v = rand_between(rng, 0, 127);
    }
    f.q = rand_between(rng, 5, 127);
}

/// The continuous (0-127) filter parameters, excluding mode and Q.
fn filter_values(f: &mut Filter) -> [&mut u8; 10] {
    [
        &mut f.breath_curve,
        &mut f.breath_mod,
        &mut f.freq,
        &mut f.key_follow,
        &mut f.lfo_breath,
        &mut f.lfo_depth,
        &mut f.lfo_freq,
        &mut f.lfo_threshold,
        &mut f.sweep_depth,
        &mut f.sweep_time,
    ]
}

/// A fully random patch (noise filters are left alone, as in Java).
pub fn random_patch(p: &mut Patch, rng: &mut fastrand::Rng) {
    p.osc1.octave = 64;
    p.osc1.semitone = 64;
    randomise_osc(rng, &mut p.osc1, 48);
    p.misc.osc2_xfade = rand_between(rng, 0, 1);
    p.osc2.octave = rand_between(rng, 62, 66);
    p.osc2.semitone = 64; // keep sane for now
    randomise_osc(rng, &mut p.osc2, 0);

    p.misc.formant_filter = rand_between(rng, 0, 2);
    p.misc.key_trigger = rand_between(rng, 0, 1);
    p.misc.osc_filter_link = rand_between(rng, 0, 2);
    randomise_filter(rng, &mut p.osc_filter1);
    randomise_filter(rng, &mut p.osc_filter2);

    p.noise.breath = rand_between(rng, 0, 127);
    p.noise.level = rand_between(rng, 0, 127);
    p.noise.time = rand_between(rng, 0, 127);

    p.misc.chorus_switch = rand_between(rng, 0, 1);
    for v in chorus_values(p) {
        *v = rand_between(rng, 0, 127);
    }
    let d = &mut p.delay;
    d.time = rand_between(rng, 0, 127);
    d.damp = rand_between(rng, 0, 127);
    d.feedback = rand_between(rng, 0, 127);
    d.dry = rand_between(rng, 64, 127);
    d.level = rand_between(rng, 0, 127);
    let r = &mut p.reverb;
    r.damp = rand_between(rng, 0, 127);
    r.density = rand_between(rng, 0, 127);
    r.dry = rand_between(rng, 64, 127);
    r.level = rand_between(rng, 0, 12);
    r.time = rand_between(rng, 0, 127);
}

fn chorus_values(p: &mut Patch) -> [&mut u8; 9] {
    let c = &mut p.chorus;
    [
        &mut c.delay1,
        &mut c.mod_lev1,
        &mut c.wet_lev1,
        &mut c.delay2,
        &mut c.mod_lev2,
        &mut c.wet_lev2,
        &mut c.dry_level,
        &mut c.feedback,
        &mut c.lfo_freq,
    ]
}

fn delay_reverb_values(p: &mut Patch) -> [&mut u8; 10] {
    let (d, r) = (&mut p.delay, &mut p.reverb);
    [
        &mut d.time,
        &mut d.damp,
        &mut d.feedback,
        &mut d.dry,
        &mut d.level,
        &mut r.damp,
        &mut r.density,
        &mut r.dry,
        &mut r.level,
        &mut r.time,
    ]
}

fn osc_values(o: &mut Osc) -> [&mut u8; 15] {
    [
        &mut o.fine,
        &mut o.beat,
        &mut o.sawtooth,
        &mut o.triangle,
        &mut o.square,
        &mut o.pulse_width,
        &mut o.pwm_depth,
        &mut o.pwm_freq,
        &mut o.sweep_depth,
        &mut o.sweep_time,
        &mut o.breath_attain,
        &mut o.breath_depth,
        &mut o.breath_curve,
        &mut o.breath_threshold,
        &mut o.level,
    ]
}

/// Nudge most parameters randomly by up to 10%.
pub fn randomise_by_10pct(p: &mut Patch, rng: &mut fastrand::Rng) {
    for o in [&mut p.osc1, &mut p.osc2] {
        for v in osc_values(o) {
            *v = rand_near(rng, 0, 127, *v);
        }
    }
    for f in [&mut p.osc_filter1, &mut p.osc_filter2] {
        for v in filter_values(f) {
            *v = rand_near(rng, 0, 127, *v);
        }
        f.q = rand_near(rng, 5, 127, f.q);
    }
    for v in chorus_values(p) {
        *v = rand_near(rng, 0, 127, *v);
    }
    for v in delay_reverb_values(p) {
        *v = rand_near(rng, 0, 127, *v);
    }
}

/// Average this patch's parameters with another's (switches and filter
/// modes are kept from this patch, as in Java).
pub fn merge_with(p: &mut Patch, other: &Patch) {
    let mut o = other.clone();
    for (a, b) in [(&mut p.osc1, &mut o.osc1), (&mut p.osc2, &mut o.osc2)] {
        a.octave = mix(a.octave, b.octave);
        a.semitone = mix(a.semitone, b.semitone);
        for (x, y) in osc_values(a).into_iter().zip(osc_values(b)) {
            *x = mix(*x, *y);
        }
    }
    for (a, b) in [
        (&mut p.osc_filter1, &mut o.osc_filter1),
        (&mut p.osc_filter2, &mut o.osc_filter2),
    ] {
        for (x, y) in filter_values(a).into_iter().zip(filter_values(b)) {
            *x = mix(*x, *y);
        }
        a.q = mix(a.q, b.q);
    }
    p.noise.breath = mix(p.noise.breath, o.noise.breath);
    p.noise.level = mix(p.noise.level, o.noise.level);
    p.noise.time = mix(p.noise.time, o.noise.time);
    for (x, y) in chorus_values(p).into_iter().zip(chorus_values(&mut o)) {
        *x = mix(*x, *y);
    }
    for (x, y) in delay_reverb_values(p)
        .into_iter()
        .zip(delay_reverb_values(&mut o))
    {
        *x = mix(*x, *y);
    }
}

/// Remove chorus, delay and reverb, leaving only the dry signal.
pub fn make_dry(p: &mut Patch) {
    p.misc.chorus_switch = 0;
    p.chorus.dry_level = 127;
    p.delay.dry = 127;
    p.delay.level = 0;
    p.reverb.dry = 127;
    p.reverb.level = 0;
}

/// Scale the oscillator, master and octave levels so the loudest is 127.
pub fn maximise_volume(p: &mut Patch) {
    let max = [p.osc1.level, p.osc2.level, p.amp.level, p.amp.octave_level]
        .into_iter()
        .max()
        .unwrap_or(0);
    if max == 0 {
        return;
    }
    let factor = 127.0 / f64::from(max);
    for v in [
        &mut p.osc1.level,
        &mut p.osc2.level,
        &mut p.amp.level,
        &mut p.amp.octave_level,
    ] {
        *v = (f64::from(*v) * factor).min(127.0) as u8;
    }
}

/// Silence the noise generator and switch off both noise filters.
pub fn remove_noise(p: &mut Patch) {
    p.noise.time = 0;
    p.noise.breath = 0;
    p.noise.level = 0;
    p.noise_filter1.mode = FILTER_OFF;
    p.noise_filter2.mode = FILTER_OFF;
}
