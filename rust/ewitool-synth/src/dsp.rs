//! Small DSP building blocks: band-limited oscillator, state-variable filter,
//! one-pole filter, delay line, noise.

use std::f32::consts::PI;

/// PolyBLEP correction for a discontinuity at phase 0 (t in 0..1, dt = step).
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

/// One oscillator producing saw, triangle and pulse from a shared phase.
#[derive(Clone, Debug, Default)]
pub struct Oscillator {
    phase: f32,
    /// Leaky-integrator state for the triangle.
    tri: f32,
}

/// Output of one oscillator step.
pub struct OscOut {
    pub saw: f32,
    pub tri: f32,
    pub pulse: f32,
}

impl Oscillator {
    /// Start a new note at a fixed phase, as a hardware oscillator reset would.
    pub fn reset(&mut self) {
        self.phase = 0.0;
        // The integrated square starts its rise at the bottom of its swing.
        self.tri = -1.0;
    }

    /// Advance one sample at `dt` = frequency / sample rate.
    pub fn next(&mut self, dt: f32, duty: f32) -> OscOut {
        let dt = dt.clamp(1e-6, 0.49);
        let t = self.phase;
        let saw = 2.0 * t - 1.0 - poly_blep(t, dt);

        // Pulse: +1 for t < duty, -1 after; corrected at both edges and
        // shifted so its average is zero whatever the duty.
        let mut pulse = if t < duty { 1.0 } else { -1.0 };
        pulse += poly_blep(t, dt);
        let t2 = (t - duty).rem_euclid(1.0);
        pulse -= poly_blep(t2, dt);
        pulse -= 2.0 * duty - 1.0;

        // Triangle: integrate a 50% square (band-limited), leaky to stay centred.
        let mut sq = if t < 0.5 { 1.0 } else { -1.0 };
        sq += poly_blep(t, dt);
        sq -= poly_blep((t - 0.5).rem_euclid(1.0), dt);
        self.tri = self.tri * (1.0 - 0.1 * dt) + 4.0 * dt * sq;

        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        OscOut {
            saw,
            tri: self.tri,
            pulse,
        }
    }
}

/// Filter response of [`Svf`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterMode {
    LowPass,
    HighPass,
    BandPass,
    Notch,
    Off,
}

impl FilterMode {
    /// The EWI's filter type value (editor order).
    pub fn from_patch(v: u8) -> Self {
        match v {
            0 => Self::LowPass,
            1 => Self::HighPass,
            2 => Self::BandPass,
            3 => Self::Notch,
            _ => Self::Off,
        }
    }
}

/// Topology-preserving-transform state-variable filter (Zavalishin), stable
/// under fast cutoff modulation.
#[derive(Clone, Debug, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
    g: f32,
    k: f32,
}

impl Svf {
    /// Set cutoff (Hz) and Q. Call at control rate.
    pub fn set(&mut self, hz: f32, q: f32, sample_rate: f32) {
        let hz = hz.clamp(10.0, sample_rate * 0.45);
        self.g = (PI * hz / sample_rate).tan();
        self.k = 1.0 / q.max(0.1);
    }

    pub fn process(&mut self, x: f32, mode: FilterMode) -> f32 {
        if mode == FilterMode::Off {
            return x;
        }
        let (g, k) = (self.g, self.k);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = x - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        match mode {
            FilterMode::LowPass => v2,
            FilterMode::BandPass => v1,
            FilterMode::HighPass => x - k * v1 - v2,
            FilterMode::Notch => x - k * v1,
            FilterMode::Off => x,
        }
    }
}

/// One-pole low-pass: `coef` 0 passes everything, near 1 smooths heavily.
#[derive(Clone, Debug, Default)]
pub struct OnePole {
    pub z: f32,
}

impl OnePole {
    pub fn process(&mut self, x: f32, coef: f32) -> f32 {
        self.z = x * (1.0 - coef) + self.z * coef;
        self.z
    }

    /// Coefficient for a cutoff in Hz.
    pub fn coef_for(hz: f32, sample_rate: f32) -> f32 {
        (-2.0 * PI * hz.max(1.0) / sample_rate).exp()
    }
}

/// Circular delay line with fractional (linear-interpolated) reads.
#[derive(Clone, Debug)]
pub struct DelayLine {
    buf: Vec<f32>,
    pos: usize,
}

impl DelayLine {
    pub fn new(max_samples: usize) -> Self {
        Self {
            buf: vec![0.0; max_samples.max(2)],
            pos: 0,
        }
    }

    pub fn write(&mut self, x: f32) {
        self.buf[self.pos] = x;
        self.pos = (self.pos + 1) % self.buf.len();
    }

    /// Value written `delay` samples ago (1.0 = the last one written).
    pub fn read(&self, delay: f32) -> f32 {
        let len = self.buf.len();
        let d = delay.clamp(1.0, (len - 1) as f32);
        let i = d.floor() as usize;
        let frac = d - i as f32;
        let a = self.buf[(self.pos + len - i) % len];
        let b = self.buf[(self.pos + len - i - 1) % len];
        a + (b - a) * frac
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }
}

/// Cheap white noise (xorshift32), -1..1.
#[derive(Clone, Debug)]
pub struct Noise(u32);

impl Default for Noise {
    fn default() -> Self {
        Self(0x1234_5678)
    }
}

impl Noise {
    pub fn sample(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}
