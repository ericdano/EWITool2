//! Measurements for comparing the model with recordings of the real EWI:
//! pitch, loudness over time, brightness and harmonic levels.

use std::f32::consts::PI;

/// Steady-state features of a held note.
#[derive(Clone, Debug)]
pub struct Features {
    /// Fundamental frequency, Hz.
    pub f0: f32,
    /// Level, dB relative to full scale.
    pub rms_db: f32,
    /// Spectral centroid (a measure of brightness), Hz.
    pub centroid_hz: f32,
    /// Level of harmonics 1..=N in dB relative to the fundamental.
    pub harmonics_db: Vec<f32>,
}

/// Number of harmonics measured.
pub const HARMONICS: usize = 16;

pub fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

pub fn db(v: f32) -> f32 {
    20.0 * v.max(1e-9).log10()
}

/// Mix interleaved stereo to mono.
pub fn mono(stereo: &[f32]) -> Vec<f32> {
    stereo
        .chunks(2)
        .map(|c| c.iter().sum::<f32>() / c.len() as f32)
        .collect()
}

/// RMS level in dB for each `hop_s`-second block.
pub fn envelope_db(x: &[f32], sr: f32, hop_s: f32) -> Vec<f32> {
    let hop = ((sr * hop_s) as usize).max(1);
    x.chunks(hop).map(|c| db(rms(c))).collect()
}

/// Index of the first sample where the level rises `above_db` over the
/// noise floor measured in the first `floor_s` seconds. Used to line up a
/// recording with the moment the note started.
pub fn onset(x: &[f32], sr: f32, floor_s: f32, above_db: f32) -> Option<usize> {
    let hop = (sr * 0.002) as usize;
    let floor_len = ((sr * floor_s) as usize).min(x.len());
    let floor = db(rms(&x[..floor_len]));
    x.chunks(hop)
        .position(|c| db(rms(c)) > floor + above_db)
        .map(|i| i * hop)
}

/// Fundamental near `expected_hz` (within a fifth either way), by
/// normalized autocorrelation with parabolic interpolation.
pub fn pitch(x: &[f32], sr: f32, expected_hz: f32) -> Option<f32> {
    let min_lag = (sr / (expected_hz * 1.5)).floor().max(2.0) as usize;
    let max_lag = (sr / (expected_hz / 1.5)).ceil() as usize;
    if x.len() < max_lag * 3 {
        return None;
    }
    // Room for the largest lag scored (max_lag + 1).
    let corr = |lag: usize| -> f32 {
        let n = x.len() - lag;
        let (mut xy, mut xx, mut yy) = (0.0f64, 0.0f64, 0.0f64);
        for i in 0..n {
            let (a, b) = (f64::from(x[i]), f64::from(x[i + lag]));
            xy += a * b;
            xx += a * a;
            yy += b * b;
        }
        (xy / (xx * yy).sqrt().max(1e-12)) as f32
    };
    // Peak of three scores at lag-1, lag, lag+1, as a fractional lag.
    let refine = |lag: usize| -> f32 {
        let (a, b, c) = (corr(lag - 1), corr(lag), corr(lag + 1));
        let denom = a - 2.0 * b + c;
        let shift = if denom.abs() > 1e-9 {
            0.5 * (a - c) / denom
        } else {
            0.0
        };
        lag as f32 + shift.clamp(-0.5, 0.5)
    };
    let (best, score) = (min_lag..=max_lag)
        .map(|l| (l, corr(l)))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    if score < 0.5 {
        return None;
    }
    // Measure across several periods for sub-sample precision: the error
    // of the interpolated peak is divided by the number of periods.
    let period = refine(best);
    let periods = ((x.len() as f32 / 3.0) / period).floor().clamp(1.0, 40.0);
    let guess = (period * periods).round() as usize;
    // The single-period error is multiplied too, so search around the guess.
    let multi = (guess.saturating_sub(3).max(2)..=guess + 3)
        .max_by(|&a, &b| corr(a).total_cmp(&corr(b)))
        .unwrap_or(guess);
    let lag = if periods > 1.0 && multi + 1 < x.len() {
        refine(multi) / periods
    } else {
        period
    };
    Some(sr / lag)
}

/// Magnitude of the Hann-windowed signal at `hz` (Goertzel).
fn magnitude_at(x: &[f32], sr: f32, hz: f32) -> f32 {
    let w = 2.0 * PI * hz / sr;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f32, 0.0f32);
    let n = x.len() as f32;
    for (i, &v) in x.iter().enumerate() {
        let win = 0.5 - 0.5 * (2.0 * PI * i as f32 / n).cos();
        let s = v * win + coeff * s1 - s2;
        s2 = s1;
        s1 = s;
    }
    (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(0.0).sqrt() * 2.0 / n
}

/// Measure a steady stretch of a held note.
pub fn features(x: &[f32], sr: f32, expected_hz: f32) -> Features {
    let f0 = pitch(x, sr, expected_hz).unwrap_or(expected_hz);
    let mags: Vec<f32> = (1..=HARMONICS)
        .map(|k| {
            let hz = f0 * k as f32;
            if hz < sr * 0.48 {
                magnitude_at(x, sr, hz)
            } else {
                0.0
            }
        })
        .collect();
    let fund = mags[0].max(1e-9);
    let harmonics_db = mags.iter().map(|m| db(m / fund)).collect();
    let total: f32 = mags.iter().map(|m| m * m).sum();
    let centroid_hz = mags
        .iter()
        .enumerate()
        .map(|(k, m)| (k + 1) as f32 * f0 * m * m)
        .sum::<f32>()
        / total.max(1e-12);
    Features {
        f0,
        rms_db: db(rms(x)),
        centroid_hz,
        harmonics_db,
    }
}

/// Difference in cents between two frequencies.
pub fn cents(a: f32, b: f32) -> f32 {
    1200.0 * (a / b).log2()
}
