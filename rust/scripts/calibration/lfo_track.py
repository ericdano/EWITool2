"""Measure a filter LFO from calibrate recordings of the L* test patches.

Usage: python3 lfo_track.py <run folder> [ewi|model]

For each take, tracks the 2-pole low-pass cutoff (as a note number) every
5 ms by fitting the harmonic levels of a plain saw, then reports the LFO
rate (strongest periodicity of the cutoff track), its swing (peak-to-peak,
semitones), the mean cutoff, and a shape hint (sine vs triangle vs square).
"""
import math, os, re, sys, wave
import numpy as np

BETA = 1.235  # pitch-tracking high-pass (measured)
H = 12

def load(path):
    w = wave.open(path)
    x = np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16).astype(float) / 32768
    return x, w.getframerate()

def note_hz(n): return 440 * 2 ** ((n - 69) / 12)

def table(f0):
    ks = np.arange(1, H + 1)
    src = -20 * np.log10(ks) + 20 * np.log10(ks**2 / np.sqrt(ks**4 + BETA**4))
    grid = np.arange(30, 130, 0.25)
    fc = note_hz(grid)[:, None]
    r = ks[None, :] * f0 / fc
    lp = -10 * np.log10((1 - r * r) ** 2 + (r / 0.5) ** 2)
    pred = src[None, :] + lp
    return grid, pred - pred[:, :1]

def track(x, sr, f0, start, stop, hop=0.005, win=0.04):
    n = int(win * sr); w = np.hanning(n)
    ks = np.arange(1, H + 1); t = np.arange(n) / sr
    basis = np.exp(-2j * np.pi * np.outer(ks * f0, t)) * w
    grid, pred = table(f0)
    out = []
    for i in range(int(start * sr), int(stop * sr) - n, int(hop * sr)):
        mag = np.abs(basis @ x[i:i + n]) + 1e-9
        h = 20 * np.log10(mag / mag[0])
        ok = h > -60
        err = ((pred[:, ok] - h[ok]) ** 2).mean(axis=1)
        out.append(grid[np.argmin(err)])
    return np.array(out), hop

def fast_rate(x, sr, f0, start, stop, cutoff_note):
    """LFO rate from the wobble of the harmonic nearest the cutoff, sampled
    at the full rate (works for fast LFOs the 5 ms track can't follow)."""
    k = max(2, round(note_hz(cutoff_note) / f0))
    seg = x[int(start * sr): int(stop * sr)]
    t = np.arange(len(seg)) / sr
    y = seg * np.exp(-2j * np.pi * k * f0 * t)
    period = int(sr / f0)
    env = np.abs(np.convolve(y, np.ones(2 * period) / (2 * period), 'valid'))
    env = env[::period // 4 or 1]
    fs = sr / (period // 4 or 1)
    d = env - env.mean()
    spec = np.abs(np.fft.rfft(d * np.hanning(len(d)), 8 * len(d)))
    freqs = np.fft.rfftfreq(8 * len(d), 1 / fs)
    band = (freqs > 0.1) & (freqs < min(200, f0 / 2))
    return freqs[band][np.argmax(spec[band])]

def onset(x, sr):
    env = np.convolve(np.abs(x), np.ones(int(sr * 0.005)) / (sr * 0.005), 'same')
    floor = np.median(np.abs(x[: int(sr * 0.3)])) + 1e-6
    return int(np.argmax(env > floor * 30))

def analyse(path, note):
    x, sr = load(path)
    on = onset(x, sr) / sr
    f0 = note_hz(note)
    # measured pitch is +23.8 cents on this EWI
    if path.endswith('-ewi.wav'):
        f0 *= 2 ** (23.8 / 1200)
    # steady part: from 0.5 s after onset to 0.3 s before the end of hold
    dur = len(x) / sr
    tr, hop = track(x, sr, f0, on + 0.5, dur - 1.3)
    if len(tr) < 20:
        return None
    d = tr - tr.mean()
    spec = np.abs(np.fft.rfft(d * np.hanning(len(d)), 16 * len(d)))
    freqs = np.fft.rfftfreq(16 * len(d), hop)
    lo = freqs > 0.05
    rate = freqs[lo][np.argmax(spec[lo])]
    swing = np.percentile(tr, 98) - np.percentile(tr, 2)
    # The 5 ms track can't follow fast LFOs; check the harmonic wobble.
    if rate > 8 or swing < 1.0:
        rate = fast_rate(x, sr, f0, on + 0.5, dur - 1.3, tr.mean())
    # shape: crest of the track (sine 1.41, triangle 1.73, square 1.0)
    crest = np.abs(d).max() / (d.std() + 1e-9)
    return rate, swing, tr.mean(), crest, tr

def main():
    run = sys.argv[1]; src = sys.argv[2] if len(sys.argv) > 2 else 'ewi'
    for patch in sorted(os.listdir(run)):
        pdir = os.path.join(run, patch)
        if not os.path.isdir(pdir):
            continue
        for f in sorted(os.listdir(pdir)):
            m = re.match(r'n(\d+)-b([\d.]+)-' + src + r'\.wav$', f)
            if not m:
                continue
            res = analyse(os.path.join(pdir, f), int(m.group(1)))
            if res is None:
                print(f'{patch:24} {f}: too short'); continue
            rate, swing, mean, crest, _ = res
            print(f'{patch:24} note {m.group(1)} breath {m.group(2)}: rate {rate:6.2f} Hz  swing {swing:5.1f} st  mean {mean:5.1f}  crest {crest:4.2f}')

if __name__ == '__main__':
    main()
