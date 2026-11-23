//! The EWI's effects: stereo chorus, delay and reverb, in that order.

use ewitool_core::patch::{Chorus as ChorusP, Delay as DelayP, Reverb as ReverbP};

use crate::dsp::{DelayLine, OnePole};
use crate::tuning as t;

/// Two modulated delay taps, one per side.
pub struct Chorus {
    line: DelayLine,
    lfo: f32,
    sr: f32,
    fb: f32,
}

impl Chorus {
    pub fn new(sr: f32) -> Self {
        Self {
            line: DelayLine::new((sr * 0.06) as usize),
            lfo: 0.0,
            sr,
            fb: 0.0,
        }
    }

    pub fn process(&mut self, x: f32, p: &ChorusP) -> (f32, f32) {
        let sr = self.sr;
        self.lfo = (self.lfo + t::lfo_hz(p.lfo_freq) / sr).fract();
        let s = (self.lfo * std::f32::consts::TAU).sin();
        let d1 = (t::chorus_delay_s(p.delay1) + t::chorus_mod_s(p.mod_lev1) * (1.0 + s)) * sr;
        let d2 = (t::chorus_delay_s(p.delay2) + t::chorus_mod_s(p.mod_lev2) * (1.0 - s)) * sr;
        let w1 = self.line.read(d1);
        let w2 = self.line.read(d2);
        self.fb = (w1 + w2) * 0.5 * t::chorus_feedback(p.feedback);
        self.line.write(x + self.fb);
        let dry = t::level(p.dry_level) * x;
        (
            dry + t::level(p.wet_lev1) * w1,
            dry + t::level(p.wet_lev2) * w2,
        )
    }
}

/// Mono-fed delay with damped feedback, returned to both sides.
pub struct Delay {
    line: DelayLine,
    damp: OnePole,
    sr: f32,
}

impl Delay {
    pub fn new(sr: f32) -> Self {
        Self {
            line: DelayLine::new((sr * 1.1) as usize),
            damp: OnePole::default(),
            sr,
        }
    }

    pub fn process(&mut self, l: f32, r: f32, p: &DelayP) -> (f32, f32) {
        let wet = self.line.read(t::delay_s(p.time) * self.sr);
        let fb = self.damp.process(wet, t::damping(p.damp)) * t::delay_feedback(p.feedback);
        self.line.write((l + r) * 0.5 + fb);
        let (dry, lev) = (t::level(p.dry), t::level(p.level));
        (l * dry + wet * lev, r * dry + wet * lev)
    }
}

/// A small Freeverb-style reverb: parallel damped combs into allpasses.
pub struct Reverb {
    combs: Vec<(DelayLine, OnePole, usize)>,
    allpasses: Vec<(DelayLine, usize)>,
}

const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
/// Right channel offset, as in Freeverb.
const SPREAD: usize = 23;

impl Reverb {
    pub fn new(sr: f32) -> Self {
        let scale = sr / 44100.0;
        let len = |n: usize| ((n as f32) * scale) as usize;
        let mut combs = Vec::new();
        let mut allpasses = Vec::new();
        for side in [0, SPREAD] {
            for n in COMBS {
                let l = len(n + side);
                combs.push((DelayLine::new(l + 1), OnePole::default(), l));
            }
            for n in ALLPASSES {
                let l = len(n + side);
                allpasses.push((DelayLine::new(l + 1), l));
            }
        }
        Self { combs, allpasses }
    }

    pub fn process(&mut self, l: f32, r: f32, p: &ReverbP) -> (f32, f32) {
        let fb = t::reverb_feedback(p.time);
        let damp = t::reverb_damping(p.damp);
        // Density blends how much the allpass diffusion is used.
        let diffusion = 0.3 + t::unit(p.density) * 0.4;
        let input = (l + r) * 0.015;
        let mut out = [0.0f32; 2];
        let n_combs = COMBS.len();
        let n_aps = ALLPASSES.len();
        for (side, o) in out.iter_mut().enumerate() {
            let mut acc = 0.0;
            for (line, lp, len) in &mut self.combs[side * n_combs..(side + 1) * n_combs] {
                let y = line.read(*len as f32);
                let filtered = lp.process(y, damp);
                line.write(input + filtered * fb);
                acc += y;
            }
            for (line, len) in &mut self.allpasses[side * n_aps..(side + 1) * n_aps] {
                let buf = line.read(*len as f32);
                line.write(acc + buf * diffusion);
                acc = buf - acc * diffusion;
            }
            *o = acc;
        }
        let (dry, lev) = (t::level(p.dry), t::level(p.level));
        (l * dry + out[0] * lev, r * dry + out[1] * lev)
    }
}
