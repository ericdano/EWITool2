//! A software model of the EWI4000s sound engine, for hearing patches when
//! no EWI is connected.
//!
//! Signal flow (one monophonic voice, like the EWI):
//!
//! ```text
//! osc 1 ─┐                                  ┌─ formant ─ anti-alias ─ amp ─ chorus ─ delay ─ reverb ─ L/R
//! osc 2 ─┴─ osc filter 1 (+2) ─┐            │
//! noise ─── noise filter 1 (+2) ┴── sum ────┘
//! ```
//!
//! Every mapping from a patch value to a physical quantity lives in
//! [`tuning`], which records which mappings are measured and which are still
//! guesses. The model is an approximation: Akai never published how the
//! EWI's DSP works.

pub mod analysis;
pub mod dsp;
pub mod fx;
pub mod tuning;
pub mod wav;

use ewitool_core::patch::{Filter, Osc};
use ewitool_core::Patch;

use dsp::{FilterMode, Noise, OnePole, Oscillator, Svf};
use tuning as t;

/// Controls are recomputed every this many samples.
const CONTROL_BLOCK: usize = 16;

/// A pair of filters routed by the patch's link setting.
#[derive(Default)]
struct FilterPair {
    a: Svf,
    b: Svf,
}

/// Which filters a path goes through (`Misc::osc_filter_link` /
/// `noise_filter_link`, editor labels "1-only", "Linked", "Both").
///
/// **Guess**: "1-only" uses the primary filter alone; "Linked" runs both in
/// series with the secondary copying the primary's settings (a steeper
/// slope); "Both" runs both in series, each with its own settings.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Link {
    OneOnly,
    Linked,
    Both,
}

impl Link {
    fn from_patch(v: u8) -> Self {
        match v {
            1 => Self::Linked,
            2 => Self::Both,
            _ => Self::OneOnly,
        }
    }
}

/// Filter LFO phase, kept per filter.
#[derive(Default, Clone, Copy)]
struct Lfo(f32);

impl Lfo {
    fn advance(&mut self, hz: f32, seconds: f32) -> f32 {
        self.0 = (self.0 + hz * seconds).fract();
        t::lfo_wave(self.0)
    }
}

/// The synthesizer: one voice plus effects. Not thread-safe by itself; the
/// app owns one on its audio thread.
pub struct Synth {
    sr: f32,
    patch: Patch,

    note: f32,
    gate: bool,
    /// Breath target (0..1) and smoothed value.
    breath_target: f32,
    breath: f32,
    /// Seconds since the last note-on (drives sweeps).
    note_time: f32,

    osc: [Oscillator; 2],
    /// Pitch-tracking high-pass on each oscillator.
    osc_hpf: [Svf; 2],
    pwm_lfo: [f32; 2],
    osc_filters: FilterPair,
    noise_filters: FilterPair,
    filter_lfo: [Lfo; 4],
    formants: Vec<Svf>,
    anti_alias: OnePole,
    anti_alias_coef: f32,
    noise: Noise,

    // Per-control-block values.
    osc_dt: [f32; 2],
    duty: [f32; 2],
    osc_gain: [f32; 2],
    noise_gain: f32,
    amp: f32,
    amp_smooth: f32,
    osc_modes: (FilterMode, FilterMode),
    noise_modes: (FilterMode, FilterMode),
    block_left: usize,

    chorus: fx::Chorus,
    delay: fx::Delay,
    reverb: fx::Reverb,
}

impl Synth {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8000.0);
        Self {
            sr,
            patch: Patch::decode(&[0; ewitool_core::PATCH_LEN]),
            note: 60.0,
            gate: false,
            breath_target: 0.0,
            breath: 0.0,
            note_time: 0.0,
            osc: Default::default(),
            osc_hpf: Default::default(),
            pwm_lfo: [0.0; 2],
            osc_filters: FilterPair::default(),
            noise_filters: FilterPair::default(),
            filter_lfo: [Lfo::default(); 4],
            formants: (0..3).map(|_| Svf::default()).collect(),
            anti_alias: OnePole::default(),
            anti_alias_coef: 0.0,
            noise: Noise::default(),
            osc_dt: [0.0; 2],
            duty: [0.5; 2],
            osc_gain: [0.0; 2],
            noise_gain: 0.0,
            amp: 0.0,
            amp_smooth: 0.0,
            osc_modes: (FilterMode::Off, FilterMode::Off),
            noise_modes: (FilterMode::Off, FilterMode::Off),
            block_left: 0,
            chorus: fx::Chorus::new(sr),
            delay: fx::Delay::new(sr),
            reverb: fx::Reverb::new(sr),
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sr
    }

    /// Use `patch` from the next sample on. Filter and effect state is kept,
    /// so changing a parameter while a note sounds doesn't click.
    pub fn set_patch(&mut self, patch: &Patch) {
        self.patch = patch.clone();
        self.block_left = 0;
    }

    /// Start a note (MIDI note number). With single trigger, a note played
    /// while another sounds only changes pitch, like legato on the EWI.
    pub fn note_on(&mut self, note: u8) {
        let legato = self.gate && self.patch.misc.key_trigger == 0;
        self.note = f32::from(note);
        if !legato {
            self.note_time = 0.0;
            for o in &mut self.osc {
                o.reset();
            }
        }
        self.gate = true;
        self.block_left = 0;
    }

    /// Stop the note; breath falls to zero and effects ring out.
    pub fn note_off(&mut self) {
        self.gate = false;
    }

    /// Breath pressure, 0.0..=1.0 (MIDI CC2 / 127).
    pub fn set_breath(&mut self, breath: f32) {
        self.breath_target = breath.clamp(0.0, 1.0);
    }

    pub fn is_note_on(&self) -> bool {
        self.gate
    }

    /// Fill an interleaved stereo buffer.
    pub fn render(&mut self, out: &mut [f32]) {
        for frame in out.as_chunks_mut::<2>().0 {
            let (l, r) = self.next_frame();
            frame[0] = l;
            frame[1] = r;
        }
    }

    fn next_frame(&mut self) -> (f32, f32) {
        if self.block_left == 0 {
            self.update_controls();
            self.block_left = CONTROL_BLOCK;
        }
        self.block_left -= 1;

        let voice = self.voice_sample();
        let p = &self.patch;
        let (mut l, mut r) = if p.misc.chorus_switch == 1 {
            self.chorus.process(voice, &p.chorus)
        } else {
            (voice, voice)
        };
        (l, r) = self.delay.process(l, r, &p.delay);
        (l, r) = self.reverb.process(l, r, &p.reverb);
        (soft_clip(l), soft_clip(r))
    }

    fn voice_sample(&mut self) -> f32 {
        // Smooth the amp per sample so control steps don't zipper.
        self.amp_smooth += (self.amp - self.amp_smooth) * 0.01;
        let mut osc_sum = 0.0;
        for i in 0..2 {
            let o = if i == 0 {
                &self.patch.osc1
            } else {
                &self.patch.osc2
            };
            let s = self.osc[i].next(self.osc_dt[i], self.duty[i]);
            let wave = s.saw * t::level(o.sawtooth)
                + s.tri * t::level(o.triangle)
                + s.pulse * t::level(o.square);
            let wave = self.osc_hpf[i].process(wave, FilterMode::HighPass);
            osc_sum += wave * self.osc_gain[i];
        }
        let link = Link::from_patch(self.patch.misc.osc_filter_link);
        let osc_out = filter_pair(&mut self.osc_filters, osc_sum, self.osc_modes, link);

        let mut noise_out = 0.0;
        if self.noise_gain > 0.0 {
            let n = self.noise.sample() * self.noise_gain;
            let link = Link::from_patch(self.patch.misc.noise_filter_link);
            noise_out = filter_pair(&mut self.noise_filters, n, self.noise_modes, link);
        }

        let mut x = osc_out + noise_out;
        let peaks = t::formant_peaks(self.patch.misc.formant_filter);
        if !peaks.is_empty() {
            let mut add = 0.0;
            for (f, &(_, _, gain)) in self.formants.iter_mut().zip(peaks) {
                add += f.process(x, FilterMode::BandPass) * gain;
            }
            x = x * 0.6 + add;
        }
        if self.patch.anti_alias.switch == 1 {
            x = self.anti_alias.process(x, self.anti_alias_coef);
        }
        x * self.amp_smooth * t::VOICE_GAIN
    }

    /// Recompute pitch, levels and cutoffs from the patch, breath and time.
    fn update_controls(&mut self) {
        let dt = CONTROL_BLOCK as f32 / self.sr;
        let target = if self.gate { self.breath_target } else { 0.0 };
        // ~5 ms breath smoothing, like the EWI's sensor filtering.
        self.breath += (target - self.breath) * (dt / 0.005).min(1.0);
        let b = self.breath;
        self.note_time += dt;
        let time = self.note_time;
        let note = self.note;
        let p = self.patch.clone();

        // Oscillators.
        let xfade = p.misc.osc2_xfade == 1;
        for (i, o) in [&p.osc1, &p.osc2].into_iter().enumerate() {
            let semis = osc_pitch_offset(o, b, time);
            let hz = t::note_hz(note + semis) + t::osc_beat_hz(o.beat);
            self.osc_dt[i] = hz.max(1.0) / self.sr;
            self.osc_hpf[i].set(
                hz * t::OSC_HIGH_PASS_RATIO,
                std::f32::consts::FRAC_1_SQRT_2,
                self.sr,
            );
            self.pwm_lfo[i] = (self.pwm_lfo[i] + t::lfo_hz(o.pwm_freq) * dt).fract();
            let lfo = t::lfo_wave(self.pwm_lfo[i]);
            self.duty[i] =
                (t::pulse_duty(o.pulse_width) + lfo * t::pwm_depth(o.pwm_depth)).clamp(0.01, 0.99);
            let thresh = t::unit(o.breath_threshold);
            let gate = ((b - thresh) / 0.05).clamp(0.0, 1.0);
            let mut gain = t::level(o.level) * gate;
            if xfade {
                gain *= if i == 0 { 1.0 - b } else { b };
            }
            self.osc_gain[i] = gain;
        }

        // Filters.
        let sr = self.sr;
        let link = Link::from_patch(p.misc.osc_filter_link);
        let f2 = if link == Link::Linked {
            &p.osc_filter1
        } else {
            &p.osc_filter2
        };
        set_filter(
            &mut self.osc_filters.a,
            &p.osc_filter1,
            note,
            b,
            time,
            &mut self.filter_lfo[0],
            dt,
            sr,
        );
        set_filter(
            &mut self.osc_filters.b,
            f2,
            note,
            b,
            time,
            &mut self.filter_lfo[1],
            dt,
            sr,
        );
        self.osc_modes = (
            FilterMode::from_patch(p.osc_filter1.mode),
            FilterMode::from_patch(f2.mode),
        );

        let link = Link::from_patch(p.misc.noise_filter_link);
        let f2 = if link == Link::Linked {
            &p.noise_filter1
        } else {
            &p.noise_filter2
        };
        set_filter(
            &mut self.noise_filters.a,
            &p.noise_filter1,
            note,
            b,
            time,
            &mut self.filter_lfo[2],
            dt,
            sr,
        );
        set_filter(
            &mut self.noise_filters.b,
            f2,
            note,
            b,
            time,
            &mut self.filter_lfo[3],
            dt,
            sr,
        );
        self.noise_modes = (
            FilterMode::from_patch(p.noise_filter1.mode),
            FilterMode::from_patch(f2.mode),
        );

        // Noise: breath scales it by the noise "breath" amount. The noise
        // "time" parameter is not modelled yet (meaning unknown).
        let nb = t::unit(p.noise.breath);
        self.noise_gain = t::level(p.noise.level) * (1.0 - nb + nb * b);

        for (f, &(hz, q, _)) in self
            .formants
            .iter_mut()
            .zip(t::formant_peaks(p.misc.formant_filter))
        {
            f.set(hz, q, sr);
        }
        let aa = t::anti_alias_note(p.anti_alias.cutoff, p.anti_alias.key_follow, note);
        self.anti_alias_coef = OnePole::coef_for(t::note_hz(aa), sr);

        self.amp = t::amp_breath_gain(b) * t::level(p.amp.level);
    }
}

/// Limiter so extreme patches can't blast the speakers: untouched up to
/// 0.7, then curving smoothly towards 1.0, so normal levels aren't coloured.
fn soft_clip(x: f32) -> f32 {
    const KNEE: f32 = 0.7;
    let a = x.abs();
    if a <= KNEE {
        x
    } else {
        let over = (a - KNEE) / (1.0 - KNEE);
        x.signum() * (KNEE + (1.0 - KNEE) * over.tanh())
    }
}

/// Pitch offset of one oscillator in semitones: tuning, sweep and the sag
/// below the note while breath is under the "attain" level.
fn osc_pitch_offset(o: &Osc, breath: f32, time: f32) -> f32 {
    let tune = t::osc_octave_semis(o.octave)
        + t::osc_semitone_semis(o.semitone)
        + t::osc_fine_semis(o.fine);
    let sweep = t::osc_sweep_semis(o.sweep_depth) * (-time / t::sweep_seconds(o.sweep_time)).exp();
    let attain = t::unit(o.breath_attain);
    let sag = if attain > 0.0 {
        let reached = (breath / attain)
            .clamp(0.0, 1.0)
            .powf(t::curve_exponent(o.breath_curve));
        -t::osc_breath_pitch_semis(o.breath_depth) * (1.0 - reached)
    } else {
        0.0
    };
    tune + sweep + sag
}

#[allow(clippy::too_many_arguments)]
fn set_filter(
    svf: &mut Svf,
    f: &Filter,
    note: f32,
    breath: f32,
    time: f32,
    lfo: &mut Lfo,
    dt: f32,
    sr: f32,
) {
    let shaped = breath.powf(t::curve_exponent(f.breath_curve));
    // Breath scales the stored cutoff; key follow and sweep add to it.
    // (Measured with key follow and sweep at neutral, so their order relative
    // to the breath scaling is a guess.)
    let mut cutoff = t::filter_cutoff_note(f.freq) * t::filter_breath_factor(f.breath_mod, shaped)
        + (note - t::KEY_FOLLOW_PIVOT) * t::filter_key_follow(f.key_follow)
        + t::filter_sweep_semis(f.sweep_depth) * (-time / t::sweep_seconds(f.sweep_time)).exp();

    let wave = lfo.advance(t::lfo_hz(f.lfo_freq), dt);
    let depth = t::filter_lfo_depth(f.lfo_depth, f.lfo_breath, f.lfo_threshold, breath);
    cutoff += wave * t::filter_lfo_semis(depth);
    svf.set(t::note_hz(cutoff), t::filter_q(f.q), sr);
}

fn filter_pair(fp: &mut FilterPair, x: f32, modes: (FilterMode, FilterMode), link: Link) -> f32 {
    let y = fp.a.process(x, modes.0);
    match link {
        Link::OneOnly => y,
        Link::Linked | Link::Both => fp.b.process(y, modes.1),
    }
}

/// A scripted way of playing a note, for previews and tests.
#[derive(Clone, Debug)]
pub struct Phrase {
    pub note: u8,
    /// Peak breath, 0..1.
    pub breath: f32,
    /// Seconds to swell from no breath to the peak.
    pub attack: f32,
    /// Seconds held at the peak.
    pub hold: f32,
    /// Seconds to fall back to no breath.
    pub release: f32,
}

impl Default for Phrase {
    fn default() -> Self {
        Self {
            note: 60,
            breath: 0.8,
            attack: 0.12,
            hold: 1.5,
            release: 0.15,
        }
    }
}

impl Phrase {
    /// Seconds the note lasts (not counting effects ringing out).
    pub fn duration(&self) -> f32 {
        self.attack + self.hold + self.release
    }

    /// Breath at `t` seconds after the start.
    pub fn breath_at(&self, t: f32) -> f32 {
        if t < self.attack {
            self.breath * t / self.attack.max(1e-3)
        } else if t < self.attack + self.hold {
            self.breath
        } else {
            let r = (t - self.attack - self.hold) / self.release.max(1e-3);
            self.breath * (1.0 - r).max(0.0)
        }
    }
}

/// Render `phrase` played with `patch`, plus `tail` seconds of effects, as
/// interleaved stereo.
pub fn render_phrase(patch: &Patch, phrase: &Phrase, tail: f32, sample_rate: f32) -> Vec<f32> {
    let mut synth = Synth::new(sample_rate);
    synth.set_patch(patch);
    synth.note_on(phrase.note);
    let frames = ((phrase.duration() + tail) * sample_rate) as usize;
    let mut out = vec![0.0; frames * 2];
    for (i, chunk) in out.chunks_mut(2 * CONTROL_BLOCK).enumerate() {
        let t = (i * CONTROL_BLOCK) as f32 / sample_rate;
        if t >= phrase.duration() && synth.is_note_on() {
            synth.note_off();
        }
        synth.set_breath(phrase.breath_at(t));
        synth.render(chunk);
    }
    out
}
