//! Hardware calibration for the patch preview synth: play notes on the real
//! EWI over MIDI, record its audio output, render the same notes with the
//! model, and compare.
//!
//! cargo run --release --example calibrate -- list
//!     MIDI ports and audio inputs (read-only).
//! cargo run --release --example calibrate -- probe --midi "<port>" --audio "<input>"
//!     Plays one note on whatever patch the EWI has loaded (sends no patch)
//!     and reports whether the audio input heard it.
//! cargo run --release --example calibrate -- run --midi "<port>" --audio "<input>" \
//!     --syx <file.syx> --out <dir> [--patches 0,24] [--notes 48,60,72] [--breath 0.4,0.9]
//!     Sends each patch to the EWI's edit buffer (never stores to a slot),
//!     records every note/breath combination, renders the model the same
//!     way, and writes WAVs plus results.csv to <dir>.
//!
//! cargo run --release --example calibrate -- compare --syx <file.syx> --out <dir> [--hold S]
//!     Re-renders the model against recordings already in <dir> (after
//!     changing the model) and prints the comparison plus an overall score.
//!     --hold must match the run that made the recordings.
//!
//! cargo run --release --example calibrate -- model --syx <file.syx> --out <dir> [--patches ..] [--notes ..] [--breath ..] [--hold S]
//!     Writes only the model's takes (nNN-bB.BB-model.wav), in the same
//!     layout as `run`, without touching any hardware.
//!
//! Common options: --channel N (audio input channel, 1-based, default 1),
//! --midi-channel N (1-16, default 1), --cc N (breath controller, default 2),
//! --hold S (seconds held at full breath, default 2).

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ewitool_app::midi::{self, Link};
use ewitool_core::outgoing::{patch_messages, PATCH_PAUSE};
use ewitool_core::{patch_set, Mode, Patch};
use ewitool_synth::analysis::{self, Features};
use ewitool_synth::{render_phrase, tuning, wav, Phrase};

/// Silence recorded before each note, for the noise floor.
const PRE_ROLL: f32 = 0.4;
/// Recorded after the note ends (release and effects).
const TAIL: f32 = 1.0;

type Opts = HashMap<String, String>;

fn parse_opts(args: &[String]) -> Result<Opts, String> {
    let mut opts = HashMap::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let key = a
            .strip_prefix("--")
            .ok_or_else(|| format!("unexpected argument {a:?}"))?;
        let v = it.next().ok_or_else(|| format!("--{key} needs a value"))?;
        opts.insert(key.to_string(), v.clone());
    }
    Ok(opts)
}

fn list_of<T: std::str::FromStr>(opts: &Opts, key: &str, default: &str) -> Result<Vec<T>, String> {
    opts.get(key)
        .map_or(default, String::as_str)
        .split(',')
        .map(|s| {
            s.trim()
                .parse()
                .map_err(|_| format!("bad --{key} value {s:?}"))
        })
        .collect()
}

fn num<T: std::str::FromStr>(opts: &Opts, key: &str, default: T) -> Result<T, String> {
    match opts.get(key) {
        Some(v) => v.parse().map_err(|_| format!("bad --{key} value {v:?}")),
        None => Ok(default),
    }
}

fn need<'a>(opts: &'a Opts, key: &str) -> Result<&'a str, String> {
    opts.get(key)
        .map(String::as_str)
        .ok_or_else(|| format!("--{key} is required"))
}

// ---- audio input ----

fn input_devices() -> Result<Vec<(String, cpal::Device)>, String> {
    let host = cpal::default_host();
    let devices = host.input_devices().map_err(|e| e.to_string())?;
    Ok(devices
        .filter_map(|d| {
            let name = d.description().ok()?.to_string();
            Some((name, d))
        })
        .collect())
}

/// Records one channel of an input device for as long as it lives.
struct Recorder {
    _stream: cpal::Stream,
    buf: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
}

impl Recorder {
    fn open(name: &str, channel: usize) -> Result<Self, String> {
        let needle = name.to_lowercase();
        let (full, device) = input_devices()?
            .into_iter()
            .find(|(n, _)| n.to_lowercase().contains(&needle))
            .ok_or_else(|| format!("no audio input matching {name:?} (try `list`)"))?;
        let supported = device.default_input_config().map_err(|e| e.to_string())?;
        if supported.sample_format() != cpal::SampleFormat::F32 {
            return Err(format!(
                "{full} records {} samples; only f32 is supported",
                supported.sample_format()
            ));
        }
        let config: cpal::StreamConfig = supported.into();
        let channels = usize::from(config.channels);
        if channel == 0 || channel > channels {
            return Err(format!(
                "{full} has {channels} channels; --channel must be 1-{channels}"
            ));
        }
        let sample_rate = config.sample_rate;
        let buf = Arc::new(Mutex::new(Vec::new()));
        let sink = buf.clone();
        let stream = device
            .build_input_stream(
                config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if let Ok(mut b) = sink.lock() {
                        b.extend(data.iter().skip(channel - 1).step_by(channels));
                    }
                },
                |e| eprintln!("audio input error: {e}"),
                None,
            )
            .map_err(|e| format!("can't open {full}: {e}"))?;
        stream.play().map_err(|e| e.to_string())?;
        println!("Recording from \"{full}\" channel {channel} at {sample_rate} Hz");
        Ok(Self {
            _stream: stream,
            buf,
            sample_rate,
        })
    }

    fn clear(&self) {
        self.buf.lock().unwrap().clear();
    }

    fn take(&self) -> Vec<f32> {
        std::mem::take(&mut *self.buf.lock().unwrap())
    }
}

// ---- MIDI performance ----

/// Plays notes; always leaves the EWI silent, even on an early return.
struct Player {
    link: Link,
    channel: u8,
    cc: u8,
}

impl Player {
    fn breath(&mut self, b: f32) -> Result<(), String> {
        let v = (b.clamp(0.0, 1.0) * 127.0).round() as u8;
        self.link.send(&[0xB0 | self.channel, self.cc, v])
    }

    /// Play `phrase` in real time.
    fn play(&mut self, phrase: &Phrase) -> Result<(), String> {
        self.breath(0.0)?;
        self.link.send(&[0x90 | self.channel, phrase.note, 100])?;
        let start = Instant::now();
        let mut last = u8::MAX;
        loop {
            let t = start.elapsed().as_secs_f32();
            if t >= phrase.duration() {
                break;
            }
            let v = (phrase.breath_at(t) * 127.0).round() as u8;
            if v != last {
                self.link.send(&[0xB0 | self.channel, self.cc, v])?;
                last = v;
            }
            sleep(Duration::from_millis(4));
        }
        self.silence();
        Ok(())
    }

    fn silence(&mut self) {
        let _ = self.breath(0.0);
        // All notes off.
        let _ = self.link.send(&[0xB0 | self.channel, 123, 0]);
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.silence();
    }
}

fn open_player(opts: &Opts) -> Result<Player, String> {
    let port = need(opts, "midi")?;
    let channel: u8 = num(opts, "midi-channel", 1)?;
    if !(1..=16).contains(&channel) {
        return Err("--midi-channel must be 1-16".into());
    }
    Ok(Player {
        link: Link::open(port)?,
        channel: channel - 1,
        cc: num(opts, "cc", 2)?,
    })
}

/// Record one phrase: pre-roll, the note, then the tail.
fn take(player: &mut Player, rec: &Recorder, phrase: &Phrase) -> Result<Vec<f32>, String> {
    rec.clear();
    sleep(Duration::from_secs_f32(PRE_ROLL));
    player.play(phrase)?;
    sleep(Duration::from_secs_f32(TAIL));
    Ok(rec.take())
}

// ---- analysis ----

struct Take {
    features: Features,
    /// Seconds to rise from 20 dB to 3 dB below the steady level.
    attack_s: f32,
}

/// Measure a recording whose note starts after a stretch of silence.
fn measure(x: &[f32], sr: f32, phrase: &Phrase) -> Option<Take> {
    let start = analysis::onset(x, sr, PRE_ROLL * 0.8, 15.0)?;
    let sec = |s: f32| start + (s * sr) as usize;
    // Steady part: after the breath has reached its peak and settled.
    let (a, b) = (
        sec(phrase.attack + 0.4),
        sec(phrase.attack + phrase.hold - 0.1),
    );
    if b > x.len() || a >= b {
        return None;
    }
    let steady = &x[a..b];
    let features = analysis::features(steady, sr, tuning::note_hz(f32::from(phrase.note)));
    // Rise time from 20 dB below the steady level to 3 dB below it, so the
    // recording's noise floor doesn't change the answer.
    // Level in sliding 10 ms windows every 2 ms (shorter windows ripple
    // with the waveform).
    let from = start.saturating_sub((0.1 * sr) as usize);
    let (win, hop) = ((0.010 * sr) as usize, (0.002 * sr) as usize);
    let env: Vec<f32> = (from..x.len().saturating_sub(win))
        .step_by(hop)
        .map(|i| analysis::db(analysis::rms(&x[i..i + win])))
        .collect();
    let reach = |below: f32| env.iter().position(|&d| d >= features.rms_db - below);
    let attack_s = match (reach(20.0), reach(3.0)) {
        (Some(a), Some(b)) => b.saturating_sub(a) as f32 * 0.002,
        _ => 0.0,
    };
    Some(Take { features, attack_s })
}

fn model_audio(patch: &Patch, phrase: &Phrase, sr: f32) -> Vec<f32> {
    let mut x = vec![0.0; (PRE_ROLL * sr) as usize];
    // A tiny noise floor so onset detection behaves as on a recording.
    let mut n = 1u32;
    for v in &mut x {
        n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *v = (n as f32 / u32::MAX as f32 - 0.5) * 1e-5;
    }
    x.extend(analysis::mono(&render_phrase(patch, phrase, TAIL, sr)));
    x
}

fn safe_name(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ---- commands ----

fn list() -> Result<(), String> {
    let (ins, outs) = midi::list_ports()?;
    println!("MIDI In ports:");
    for p in ins {
        println!("  {p}");
    }
    println!("MIDI Out ports:");
    for p in outs {
        println!("  {p}");
    }
    println!("Audio inputs:");
    for (name, d) in input_devices()? {
        let conf = d
            .default_input_config()
            .map(|c| {
                format!(
                    "{} ch, {} Hz, {}",
                    c.channels(),
                    c.sample_rate(),
                    c.sample_format()
                )
            })
            .unwrap_or_else(|e| e.to_string());
        println!("  {name}  ({conf})");
    }
    Ok(())
}

fn probe(opts: &Opts) -> Result<(), String> {
    let rec = Recorder::open(need(opts, "audio")?, num(opts, "channel", 1)?)?;
    let mut player = open_player(opts)?;
    let phrase = Phrase {
        note: 60,
        breath: 0.8,
        attack: 0.1,
        hold: 1.0,
        release: 0.1,
    };
    println!("Playing middle C on the EWI's current patch (no patch is sent)…");
    let x = take(&mut player, &rec, &phrase)?;
    let sr = rec.sample_rate as f32;
    let floor_n = ((PRE_ROLL * 0.8 * sr) as usize).min(x.len());
    let floor = analysis::db(analysis::rms(&x[..floor_n]));
    let loudest = analysis::envelope_db(&x, sr, 0.05)
        .into_iter()
        .fold(f32::MIN, f32::max);
    println!("Noise floor {floor:.1} dBFS, loudest {loudest:.1} dBFS");
    if loudest - floor < 15.0 {
        return Err(
            "Didn't hear the note. Check the audio cable and input level, that the \
                    terminal app is allowed to use the microphone (System Settings ▸ Privacy \
                    & Security ▸ Microphone), and that the EWI is on the chosen MIDI channel."
                .into(),
        );
    }
    if loudest > -1.0 {
        println!("Warning: the recording is clipping; turn the input gain down.");
    }
    match measure(&x, sr, &phrase) {
        Some(t) => println!(
            "Heard it: {:.1} Hz ({:+.1} cents from C4), attack {:.0} ms. Ready to calibrate.",
            t.features.f0,
            analysis::cents(t.features.f0, tuning::note_hz(60.0)),
            t.attack_s * 1000.0
        ),
        None => println!("Heard sound, but couldn't measure a steady note."),
    }
    Ok(())
}

fn run(opts: &Opts) -> Result<(), String> {
    let syx = PathBuf::from(need(opts, "syx")?);
    let out = PathBuf::from(need(opts, "out")?);
    let patches = patch_set::load_syx(&syx).map_err(|e| e.to_string())?;
    let picks: Vec<usize> = match opts.get("patches") {
        Some(_) => list_of(opts, "patches", "")?,
        None => (0..patches.len()).collect(),
    };
    let notes: Vec<u8> = list_of(opts, "notes", "48,60,72")?;
    let breaths: Vec<f32> = list_of(opts, "breath", "0.4,0.9")?;
    let hold: f32 = num(opts, "hold", 2.0)?;

    let rec = Recorder::open(need(opts, "audio")?, num(opts, "channel", 1)?)?;
    let sr = rec.sample_rate as f32;
    let mut player = open_player(opts)?;
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let csv_path = out.join("results.csv");
    let new_csv = !csv_path.exists();
    let mut csv = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&csv_path)
        .map_err(|e| e.to_string())?;
    if new_csv {
        let harmonics: Vec<String> = (1..=analysis::HARMONICS).map(|k| format!("h{k}")).collect();
        writeln!(
            csv,
            "file,patch,name,note,breath,source,f0,cents,rms_db,centroid_hz,attack_ms,{}",
            harmonics.join(",")
        )
        .map_err(|e| e.to_string())?;
    }

    println!(
        "{} patches × {} notes × {} breath levels. Patches go to the EWI's edit buffer only.",
        picks.len(),
        notes.len(),
        breaths.len()
    );
    for &i in &picks {
        let patch = patches
            .get(i)
            .ok_or_else(|| format!("{} has no patch #{i}", syx.display()))?;
        println!("\n#{i} {}", patch.name());
        for msg in patch_messages(patch, Mode::Edit) {
            player.link.send(&msg)?;
            sleep(PATCH_PAUSE);
        }
        sleep(Duration::from_millis(300));
        let dir = out.join(format!("{i:02}-{}", safe_name(&patch.name())));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        for &note in &notes {
            for &breath in &breaths {
                let phrase = Phrase {
                    note,
                    breath,
                    attack: 0.15,
                    hold,
                    release: 0.15,
                };
                let ewi = take(&mut player, &rec, &phrase)?;
                let model = model_audio(patch, &phrase, sr);
                let stem = format!("n{note}-b{breath:.2}");
                for (src, x) in [("ewi", &ewi), ("model", &model)] {
                    let path = dir.join(format!("{stem}-{src}.wav"));
                    wav::write(&path, x, 1, rec.sample_rate).map_err(|e| e.to_string())?;
                }
                let (e, m) = (measure(&ewi, sr, &phrase), measure(&model, sr, &phrase));
                print_comparison(note, breath, e.as_ref(), m.as_ref());
                for (src, t) in [("ewi", &e), ("model", &m)] {
                    if let Some(t) = t {
                        write_row(&mut csv, &syx, i, patch, &phrase, src, t)?;
                    }
                }
            }
        }
    }
    println!("\nWrote recordings and {}", csv_path.display());
    Ok(())
}

/// Model takes only, laid out like `run`'s output. No hardware.
fn model_only(opts: &Opts) -> Result<(), String> {
    let syx = PathBuf::from(need(opts, "syx")?);
    let out = PathBuf::from(need(opts, "out")?);
    let patches = patch_set::load_syx(&syx).map_err(|e| e.to_string())?;
    let picks: Vec<usize> = match opts.get("patches") {
        Some(_) => list_of(opts, "patches", "")?,
        None => (0..patches.len()).collect(),
    };
    let notes: Vec<u8> = list_of(opts, "notes", "48,60,72")?;
    let breaths: Vec<f32> = list_of(opts, "breath", "0.4,0.9")?;
    let hold: f32 = num(opts, "hold", 2.0)?;
    let rate = 44100;
    for i in picks {
        let patch = patches.get(i).ok_or("no such patch")?;
        let dir = out.join(format!("{i:02}-{}", safe_name(&patch.name())));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for &note in &notes {
            for &breath in &breaths {
                let phrase = Phrase {
                    note,
                    breath,
                    attack: 0.15,
                    hold,
                    release: 0.15,
                };
                let x = model_audio(patch, &phrase, rate as f32);
                let path = dir.join(format!("n{note}-b{breath:.2}-model.wav"));
                wav::write(&path, &x, 1, rate).map_err(|e| e.to_string())?;
            }
        }
    }
    println!("Wrote model takes to {}", out.display());
    Ok(())
}

/// Re-render the model for recordings made earlier by `run`.
fn compare(opts: &Opts) -> Result<(), String> {
    let syx = PathBuf::from(need(opts, "syx")?);
    let out = PathBuf::from(need(opts, "out")?);
    let hold: f32 = num(opts, "hold", 2.0)?;
    let patches = patch_set::load_syx(&syx).map_err(|e| e.to_string())?;
    let mut err = Score::default();
    for (i, patch) in patches.iter().enumerate() {
        let dir = out.join(format!("{i:02}-{}", safe_name(&patch.name())));
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut stems: Vec<String> = entries
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter_map(|n| n.strip_suffix("-ewi.wav").map(str::to_string))
            .collect();
        stems.sort();
        println!("\n#{i} {}", patch.name());
        for stem in stems {
            // n60-b0.80
            let parse = || -> Option<(u8, f32)> {
                let (n, b) = stem.strip_prefix('n')?.split_once("-b")?;
                Some((n.parse().ok()?, b.parse().ok()?))
            };
            let Some((note, breath)) = parse() else {
                continue;
            };
            let (ewi, _, rate) =
                wav::read(&dir.join(format!("{stem}-ewi.wav"))).map_err(|e| e.to_string())?;
            let sr = rate as f32;
            let phrase = Phrase {
                note,
                breath,
                attack: 0.15,
                hold,
                release: 0.15,
            };
            let model = model_audio(patch, &phrase, sr);
            let (e, m) = (measure(&ewi, sr, &phrase), measure(&model, sr, &phrase));
            print_comparison(note, breath, e.as_ref(), m.as_ref());
            if let (Some(e), Some(m)) = (e, m) {
                err.add(&e, &m);
            }
        }
    }
    err.print();
    Ok(())
}

/// Overall model error across takes.
#[derive(Default)]
struct Score {
    n: usize,
    /// Sum of squared harmonic-level errors (harmonics 2-8, dB).
    harm: f32,
    harm_n: usize,
    /// Sum of squared brightness errors, in semitones.
    bright: f32,
}

impl Score {
    fn add(&mut self, e: &Take, m: &Take) {
        self.n += 1;
        for k in 1..8 {
            let (a, b) = (e.features.harmonics_db[k], m.features.harmonics_db[k]);
            // Ignore harmonics lost in the noise.
            if a > -50.0 {
                self.harm += (a - b).powi(2);
                self.harm_n += 1;
            }
        }
        let st = 12.0 * (e.features.centroid_hz / m.features.centroid_hz).log2();
        self.bright += st * st;
    }

    fn print(&self) {
        if self.n == 0 {
            println!("No recordings found.");
            return;
        }
        println!(
            "\nOverall: harmonic levels off by {:.1} dB RMS; brightness off by {:.1} semitones RMS ({} takes)",
            (self.harm / self.harm_n.max(1) as f32).sqrt(),
            (self.bright / self.n as f32).sqrt(),
            self.n
        );
    }
}

/// Compare the model with itself, delayed and with a noise floor like a
/// recording; every pair of numbers should match. Touches no hardware.
fn selftest(opts: &Opts) -> Result<(), String> {
    let syx = PathBuf::from(need(opts, "syx")?);
    let patches = patch_set::load_syx(&syx).map_err(|e| e.to_string())?;
    let picks: Vec<usize> = list_of(opts, "patches", "0")?;
    let sr = 44100.0;
    for i in picks {
        let patch = patches.get(i).ok_or("no such patch")?;
        println!("#{i} {}", patch.name());
        for note in [48u8, 60, 72] {
            let phrase = Phrase {
                note,
                breath: 0.8,
                attack: 0.15,
                hold: 2.0,
                release: 0.15,
            };
            let model = model_audio(patch, &phrase, sr);
            // Fake recording: 37 ms later, at a different gain, with hiss.
            let mut fake = vec![0.0; (0.037 * sr) as usize];
            fake.extend(model.iter().map(|v| v * 0.5));
            let mut n = 7u32;
            for v in &mut fake {
                n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *v += (n as f32 / u32::MAX as f32 - 0.5) * 2e-4;
            }
            let (e, m) = (measure(&fake, sr, &phrase), measure(&model, sr, &phrase));
            print_comparison(note, 0.8, e.as_ref(), m.as_ref());
        }
    }
    Ok(())
}

fn print_comparison(note: u8, breath: f32, ewi: Option<&Take>, model: Option<&Take>) {
    let label = format!("  note {note:3} breath {breath:.2}:");
    let (Some(e), Some(m)) = (ewi, model) else {
        let which = if ewi.is_none() { "EWI" } else { "model" };
        println!("{label} no steady note measured from the {which}");
        return;
    };
    let (ef, mf) = (&e.features, &m.features);
    let nominal = tuning::note_hz(f32::from(note));
    println!(
        "{label} pitch {:+6.1}¢ vs {:+6.1}¢ | bright {:6.0} vs {:6.0} Hz | attack {:4.0} vs {:4.0} ms | h2 {:+5.1} vs {:+5.1} dB | h3 {:+5.1} vs {:+5.1} dB",
        analysis::cents(ef.f0, nominal),
        analysis::cents(mf.f0, nominal),
        ef.centroid_hz,
        mf.centroid_hz,
        e.attack_s * 1000.0,
        m.attack_s * 1000.0,
        ef.harmonics_db[1],
        mf.harmonics_db[1],
        ef.harmonics_db[2],
        mf.harmonics_db[2],
    );
}

fn write_row(
    csv: &mut std::fs::File,
    syx: &Path,
    i: usize,
    patch: &Patch,
    phrase: &Phrase,
    src: &str,
    t: &Take,
) -> Result<(), String> {
    let f = &t.features;
    let nominal = tuning::note_hz(f32::from(phrase.note));
    let h: Vec<String> = f.harmonics_db.iter().map(|d| format!("{d:.2}")).collect();
    writeln!(
        csv,
        "{},{i},{},{},{:.2},{src},{:.3},{:.2},{:.2},{:.1},{:.1},{}",
        syx.display().to_string().replace(',', "_"),
        patch.name().replace(',', "_"),
        phrase.note,
        phrase.breath,
        f.f0,
        analysis::cents(f.f0, nominal),
        f.rms_db,
        f.centroid_hz,
        t.attack_s * 1000.0,
        h.join(",")
    )
    .map_err(|e| e.to_string())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("list") => list(),
        Some("model") => parse_opts(&args[1..]).and_then(|o| model_only(&o)),
        Some("compare") => parse_opts(&args[1..]).and_then(|o| compare(&o)),
        Some("selftest") => parse_opts(&args[1..]).and_then(|o| selftest(&o)),
        Some("probe") => parse_opts(&args[1..]).and_then(|o| probe(&o)),
        Some("run") => parse_opts(&args[1..]).and_then(|o| run(&o)),
        _ => Err("usage: calibrate list | probe --midi P --audio A | run --midi P --audio A --syx F --out D [...]\n\
                  (see the top of examples/calibrate.rs)"
            .into()),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
