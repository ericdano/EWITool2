//! How each 0-127 patch value maps to a physical quantity (Hz, seconds,
//! semitones, gain).
//!
//! Akai never documented the EWI4000s sound engine, so this file is where
//! calibration happens: every mapping says whether it is **measured** (checked
//! against the real instrument or factory patches) or a **guess** (a sensible
//! starting point still to be checked against recordings of the EWI).
//! Keep all such numbers here so the DSP code never needs to change when a
//! mapping is corrected.

/// Value of a bipolar parameter (centre 64) as -1.0..=1.0.
pub fn bipolar(v: u8) -> f32 {
    ((f32::from(v) - 64.0) / 63.0).clamp(-1.0, 1.0)
}

/// Value of a unipolar parameter as 0.0..=1.0.
pub fn unit(v: u8) -> f32 {
    f32::from(v.min(127)) / 127.0
}

/// Frequency of a (possibly fractional) MIDI note number.
pub fn note_hz(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

// ---- Oscillators ----

/// Octave selector: 62..=66 is -2..=+2 octaves. **Measured** (editor ranges
/// match the Java editor and the EWI's own display).
pub fn osc_octave_semis(v: u8) -> f32 {
    (f32::from(v) - 64.0) * 12.0
}

/// Semitone selector: 52..=76 is -12..=+12. **Measured** (factory patch
/// "Saw -4" stores 59 on oscillator 2; "Saw+5" stores 71).
pub fn osc_semitone_semis(v: u8) -> f32 {
    f32::from(v) - 64.0
}

/// Fine tune: 14..=114 is -50..=+50 cents. **Measured** (editor range).
pub fn osc_fine_semis(v: u8) -> f32 {
    (f32::from(v) - 64.0) / 100.0
}

/// Beat: a fixed detune in Hz, so the beating between the two oscillators is
/// the same speed on every note. **Guess** at the scale.
pub fn osc_beat_hz(v: u8) -> f32 {
    bipolar(v) * 4.0
}

/// Waveform and oscillator levels. **Guess**: linear.
pub fn level(v: u8) -> f32 {
    unit(v)
}

/// Pulse width as a duty cycle. **Measured** on the EWI (2026-11-23): the
/// recorded spectra of factory patches storing 13/63/95/114 have nulls
/// that put the duty at 45/25/12.5/5%, i.e. 0 is a square and 127 the
/// narrowest pulse. (The "%" in those patch names is slider position.)
pub fn pulse_duty(v: u8) -> f32 {
    (0.5 * (1.0 - unit(v))).max(0.005)
}

/// Rate of LFOs in Hz. **Measured** for the filter LFO (2026-11-23):
/// 0.1 + 0.00527 × value², i.e. 1/8/16/24/32/48/64/96 → 0.1/0.44/1.45/
/// 3.14/5.5/12.2/21.7/48.7 Hz (each within ~1%; 127 ≈ 85 Hz). The PWM and
/// chorus LFOs are assumed to use the same scale.
pub fn lfo_hz(v: u8) -> f32 {
    let v = f32::from(v.min(127));
    0.1 + 0.00527 * v * v
}

/// The LFOs' waveform at phase 0..1: a triangle (**measured** for the
/// filter LFO: the cutoff's 3rd/5th harmonics sit at -20/-29 dB, as a
/// triangle's do). Runs freely; a new note doesn't restart it.
pub fn lfo_wave(phase: f32) -> f32 {
    let p = phase.rem_euclid(1.0);
    if p < 0.5 {
        4.0 * p - 1.0
    } else {
        3.0 - 4.0 * p
    }
}

/// PWM depth as a change of duty cycle. **Guess**.
pub fn pwm_depth(v: u8) -> f32 {
    unit(v) * 0.45
}

/// Pitch sweep at the start of a note, in semitones. Centre 64 = none
/// (**measured**: 83 of 100 factory patches store 64). Range is a **guess**.
pub fn osc_sweep_semis(v: u8) -> f32 {
    bipolar(v) * 12.0
}

/// Decay time constant of a sweep (oscillator or filter), in seconds.
/// **Guess**.
pub fn sweep_seconds(v: u8) -> f32 {
    0.002 + unit(v).powi(2) * 2.0
}

/// How far the pitch sags below the note when breath is under the "attain"
/// level, in semitones. Centre 64 = none (**measured**: 92 of 100 factory
/// patches). Range is a **guess**.
pub fn osc_breath_pitch_semis(v: u8) -> f32 {
    bipolar(v) * 2.0
}

/// Exponent applied to breath (0..1) for a curve parameter; 64 is linear.
/// **Measured** for the filter breath curve (breath 0.5, plain saw):
/// 32/48/64/80/96 behaved as breath^2.1/1.43/1.0/0.40/0.14, interpolated
/// in log2 and extrapolated to the ends. The oscillator breath curve is
/// assumed to work the same way.
pub fn curve_exponent(v: u8) -> f32 {
    const POINTS: [(f32, f32); 7] = [
        (0.0, 2.26),
        (32.0, 1.07),
        (48.0, 0.516),
        (64.0, 0.0),
        (80.0, -1.32),
        (96.0, -2.84),
        (127.0, -5.8),
    ];
    let v = f32::from(v.min(127));
    let log2 = POINTS.windows(2).find(|w| v <= w[1].0).map_or(-5.8, |w| {
        let t = (v - w[0].0) / (w[1].0 - w[0].0);
        w[0].1 + t * (w[1].1 - w[0].1)
    });
    2f32.powf(log2)
}

// ---- Filters ----

/// Filter cutoff as a MIDI note number. **Measured** (2026-11-23, plain saw,
/// key follow 64, no breath mod): values 60/72/84/96/108 gave 2-pole
/// low-pass cutoffs at notes 60.1/72.2/84.4/96.5/107.6.
pub fn filter_cutoff_note(v: u8) -> f32 {
    f32::from(v)
}

/// Filter resonance as a Q factor. **Measured** from the height of the
/// resonant peak on a plain saw (q 5/30/60/90/120 gave Q 0.5/2.8/5.6/8.8/13).
/// The EWI also adds upper harmonics at high q (probably filter
/// saturation), which isn't modelled.
pub fn filter_q(v: u8) -> f32 {
    const POINTS: [(f32, f32); 6] = [
        (5.0, 0.5),
        (30.0, 2.8),
        (60.0, 5.6),
        (90.0, 8.8),
        (120.0, 13.0),
        (127.0, 14.0),
    ];
    let q = f32::from(v).clamp(5.0, 127.0);
    POINTS.windows(2).find(|w| q <= w[1].0).map_or(14.0, |w| {
        let t = (q - w[0].0) / (w[1].0 - w[0].0);
        w[0].1 + t * (w[1].1 - w[0].1)
    })
}

/// Keyboard tracking: semitones of cutoff per semitone of pitch, around
/// [`KEY_FOLLOW_PIVOT`]. **Measured**: 52/64/76/88 gave slopes of
/// -1/0/+1/+2, all meeting the stored cutoff at note 72.
pub fn filter_key_follow(v: u8) -> f32 {
    (f32::from(v) - 64.0) / 12.0
}

/// The note at which key follow leaves the cutoff unchanged. **Measured**.
pub const KEY_FOLLOW_PIVOT: f32 = 72.0;

/// Breath mod scales the cutoff note number: full breath gives the stored
/// cutoff, and the note falls in proportion to breath mod × (1 − curved
/// breath), down to 0 at full breath mod and no breath. **Measured**: with
/// breath mod 64 at breath 0.4 the cutoff fell 30.5% of its value whether
/// the value was 60, 80, 100 or 120; the fall is linear in breath mod
/// (32/64/96/127) and, at curve 64, in breath.
pub fn filter_breath_factor(breath_mod: u8, curved_breath: f32) -> f32 {
    1.0 - unit(breath_mod) * (1.0 - curved_breath.clamp(0.0, 1.0))
}

/// Filter LFO swing either side of the cutoff, in semitones, for an
/// effective depth of 0..127. **Measured**: the cutoff tracker read 0.344
/// semitones peak-to-peak per step (16 → 5.5, 64 → 21.5, 127 → 42), and
/// reads model renders 9% low at every size (its 40 ms window rounds the
/// triangle's peaks), so the true swing is 0.375 per step.
pub fn filter_lfo_semis(depth: f32) -> f32 {
    depth * 0.1875
}

/// Effective filter LFO depth (0..127) after "LFO breath" and "LFO
/// threshold". **Measured** (depths 4/16/32, LFO breath 0/32/64/96/127,
/// threshold 0/32/64/96/127, breath 0.3/0.6/0.9; all within ~2 semitones):
/// - LFO breath 64: depth as stored; threshold has no effect.
/// - Above 64: breath above the threshold pushes the depth towards the
///   maximum: depth + (127 − depth) × amount × (breath − thr)/(1 − thr).
/// - Below 64: the wobble fades as breath rises to the threshold (at once
///   when the threshold is 0): depth × (1 − amount × min(1, breath/thr)).
pub fn filter_lfo_depth(depth: u8, lfo_breath: u8, threshold: u8, breath: f32) -> f32 {
    let depth = f32::from(depth.min(127));
    let amount = bipolar(lfo_breath);
    let thr = unit(threshold);
    if amount >= 0.0 {
        let over = if thr < 1.0 {
            ((breath - thr) / (1.0 - thr)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        depth + (127.0 - depth) * amount * over
    } else {
        let reached = if thr > 0.0 {
            (breath / thr).clamp(0.0, 1.0)
        } else {
            1.0
        };
        depth * (1.0 + amount * reached)
    }
}

/// Filter sweep at the start of a note, in semitones. Centre 64 = none
/// (**measured**: 82 of 100 factory patches). Range is a **guess**.
pub fn filter_sweep_semis(v: u8) -> f32 {
    bipolar(v) * 36.0
}

// ---- Anti-alias filter ----

/// Anti-alias low-pass cutoff as a note number. Every factory patch stores
/// 76 with key follow 12. **Guess**, constrained by measurement: with those
/// settings the recordings show no extra roll-off below ~7 kHz, so it sits
/// above that.
pub fn anti_alias_note(cutoff: u8, key_follow: u8, note: f32) -> f32 {
    f32::from(cutoff) + 48.0 + (note - 60.0) * f32::from(key_follow) / 12.0
}

// ---- Formant filter (Off / Woodwind / Strings) ----

/// Resonant peaks (Hz, Q, gain) added for the formant filter types.
/// **Guess**: generic body resonances.
pub fn formant_peaks(kind: u8) -> &'static [(f32, f32, f32)] {
    match kind {
        1 => &[(550.0, 3.0, 0.8), (1500.0, 4.0, 0.5), (2800.0, 5.0, 0.3)],
        2 => &[(280.0, 2.5, 0.7), (1200.0, 3.0, 0.4), (3200.0, 4.0, 0.4)],
        _ => &[],
    }
}

// ---- Amp ----

/// Overall loudness for a breath of 0..1. **Measured** with the filter off:
/// level follows breath² within 0.3 dB from 0.1 to 1.0.
pub fn amp_breath_gain(breath: f32) -> f32 {
    let b = breath.clamp(0.0, 1.0);
    b * b
}

/// Every oscillator passes through a 2-pole (Butterworth) high-pass that
/// tracks its pitch, at this multiple of its frequency. **Measured**: with
/// the filters off, a saw's harmonics 2+ sit ~5 dB above the fundamental
/// on every note from 36 to 84; this fits within 0.1-0.3 dB. (It may be how
/// the EWI generates its waveforms rather than a separate filter.)
pub const OSC_HIGH_PASS_RATIO: f32 = 1.235;

/// Scale from the summed voice to a comfortable output level.
pub const VOICE_GAIN: f32 = 1.0;

// ---- Effects ----

/// Chorus delay line base time, seconds. **Guess**.
pub fn chorus_delay_s(v: u8) -> f32 {
    0.001 + unit(v) * 0.040
}

/// Chorus modulation depth, seconds. **Guess**.
pub fn chorus_mod_s(v: u8) -> f32 {
    unit(v) * 0.004
}

/// Chorus feedback. Factory patches mostly store 44, so treat as bipolar.
/// **Guess**.
pub fn chorus_feedback(v: u8) -> f32 {
    bipolar(v) * 0.7
}

/// Delay time in seconds. **Guess**.
pub fn delay_s(v: u8) -> f32 {
    0.01 + unit(v) * 1.0
}

/// Delay feedback. **Guess**.
pub fn delay_feedback(v: u8) -> f32 {
    unit(v) * 0.9
}

/// Damping as a one-pole low-pass coefficient (0 = none). **Guess**.
pub fn damping(v: u8) -> f32 {
    unit(v) * 0.8
}

/// Reverb decay: comb feedback for time 10..=50 (editor range). **Guess**.
pub fn reverb_feedback(time: u8) -> f32 {
    let t = ((f32::from(time) - 10.0) / 40.0).clamp(0.0, 1.0);
    0.70 + t * 0.27
}

/// Reverb damping for 54..=74 (editor range). **Guess**.
pub fn reverb_damping(v: u8) -> f32 {
    ((f32::from(v) - 54.0) / 20.0).clamp(0.0, 1.0) * 0.6 + 0.1
}
