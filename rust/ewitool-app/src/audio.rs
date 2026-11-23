//! Patch preview: plays patches through `ewitool-synth` on the computer's
//! speakers, for hearing a patch when no EWI is connected.
//!
//! The audio device is opened on the first preview, not at startup, so the
//! app doesn't hold an audio stream (or ask for one) until it's needed.
//! The GUI talks to the audio callback through a small mutex-protected
//! [`Control`]; the callback only `try_lock`s it, so it never waits on the GUI.

use std::sync::{Arc, Mutex};

use ewitool_core::Patch;
use ewitool_synth::{Phrase, Synth};

/// Silence between repeats of a looping preview, seconds.
const LOOP_GAP: f32 = 0.35;

/// What the GUI asks the audio thread to do.
#[derive(Default)]
struct Control {
    patch: Option<Patch>,
    /// Bumped whenever `patch` changes.
    patch_version: u64,
    phrase: Phrase,
    looping: bool,
    /// Bumped to start the phrase from the beginning.
    start: u64,
    /// Bumped to stop.
    stop: u64,
    /// Set by the audio thread: a phrase is sounding.
    playing: bool,
}

/// State owned by the audio callback.
struct Voice {
    synth: Synth,
    patch_version: u64,
    start: u64,
    stop: u64,
    phrase: Phrase,
    looping: bool,
    /// Seconds into the current phrase, or `None` when idle.
    time: Option<f32>,
}

impl Voice {
    fn sync(&mut self, c: &mut Control) {
        if c.patch_version != self.patch_version {
            self.patch_version = c.patch_version;
            if let Some(p) = &c.patch {
                self.synth.set_patch(p);
            }
        }
        if c.start != self.start {
            self.start = c.start;
            self.phrase = c.phrase.clone();
            self.looping = c.looping;
            self.time = Some(0.0);
            self.synth.note_off();
            self.synth.note_on(self.phrase.note);
        } else {
            // Breath and note changes apply to a phrase already playing.
            self.phrase.breath = c.phrase.breath;
            self.looping = c.looping;
        }
        if c.stop != self.stop {
            self.stop = c.stop;
            self.time = None;
            self.synth.note_off();
        }
        c.playing = self.time.is_some();
    }

    fn render(&mut self, out: &mut [f32]) {
        let sr = self.synth.sample_rate();
        // Small chunks so breath follows the phrase smoothly.
        for chunk in out.chunks_mut(64) {
            if let Some(t) = self.time {
                let len = self.phrase.duration();
                if t >= len + LOOP_GAP && self.looping {
                    self.time = Some(0.0);
                    self.synth.note_off();
                    self.synth.note_on(self.phrase.note);
                } else if t >= len {
                    self.synth.note_off();
                    if !self.looping {
                        self.time = None;
                    }
                }
                if let Some(t) = self.time {
                    self.synth.set_breath(self.phrase.breath_at(t));
                    self.time = Some(t + (chunk.len() / 2) as f32 / sr);
                }
            }
            self.synth.render(chunk);
        }
    }
}

/// The open audio output.
struct Output {
    _stream: cpal::Stream,
}

/// Plays patch previews. Created closed; opens the default output device on
/// the first [`Audition::play`].
pub struct Audition {
    control: Arc<Mutex<Control>>,
    output: Option<Output>,
    /// Tests never open a real device.
    enabled: bool,
    /// Why the device couldn't be opened, shown to the user once.
    pub error: Option<String>,
    /// The note and breath used for previews (editable in the GUI).
    pub note: u8,
    pub breath: f32,
}

impl Audition {
    pub fn new() -> Self {
        Self {
            control: Arc::default(),
            output: None,
            enabled: true,
            error: None,
            note: 60,
            breath: 0.8,
        }
    }

    /// An Audition that never opens an audio device (for tests). Requests are
    /// still recorded, so tests can check what would play.
    pub fn silent() -> Self {
        Self {
            enabled: false,
            ..Self::new()
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Control> {
        self.control.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Play `patch` once, or repeatedly until [`Audition::stop`] if
    /// `looping`.
    pub fn play(&mut self, patch: &Patch, looping: bool) {
        self.ensure_open();
        let phrase = Phrase {
            note: self.note,
            breath: self.breath,
            ..Phrase::default()
        };
        let mut c = self.lock();
        c.patch = Some(patch.clone());
        c.patch_version += 1;
        c.phrase = phrase;
        c.looping = looping;
        c.start += 1;
        // Shows as playing straight away; the audio thread confirms it.
        c.playing = true;
    }

    /// Use an edited patch for the sound already playing.
    pub fn update_patch(&mut self, patch: &Patch) {
        let mut c = self.lock();
        if c.patch.as_ref() != Some(patch) {
            c.patch = Some(patch.clone());
            c.patch_version += 1;
        }
    }

    /// Change the breath of the sound already playing (and future ones).
    pub fn set_breath(&mut self, breath: f32) {
        self.breath = breath;
        self.lock().phrase.breath = breath;
    }

    pub fn stop(&mut self) {
        let mut c = self.lock();
        c.stop += 1;
        c.playing = false;
    }

    pub fn is_playing(&self) -> bool {
        self.lock().playing
    }

    /// Whether a looping preview is going.
    pub fn is_looping(&self) -> bool {
        let c = self.lock();
        c.playing && c.looping
    }

    /// The patch most recently sent to play (for tests).
    pub fn current_patch(&self) -> Option<Patch> {
        self.lock().patch.clone()
    }

    fn ensure_open(&mut self) {
        if !self.enabled || self.output.is_some() {
            return;
        }
        match open_output(self.control.clone()) {
            Ok(o) => {
                self.output = Some(o);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }
}

impl Default for Audition {
    fn default() -> Self {
        Self::new()
    }
}

fn open_output(control: Arc<Mutex<Control>>) -> Result<Output, String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("No audio output device found.")?;
    let supported = device
        .default_output_config()
        .map_err(|e| format!("Couldn't read the audio output settings: {e}"))?;
    if supported.sample_format() != cpal::SampleFormat::F32 {
        return Err(format!(
            "The audio output uses {} samples, which preview doesn't support yet.",
            supported.sample_format()
        ));
    }
    let config: cpal::StreamConfig = supported.into();
    let channels = usize::from(config.channels);
    let mut voice = Voice {
        synth: Synth::new(config.sample_rate as f32),
        patch_version: 0,
        start: 0,
        stop: 0,
        phrase: Phrase::default(),
        looping: false,
        time: None,
    };
    let mut stereo = Vec::new();
    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                if let Ok(mut c) = control.try_lock() {
                    voice.sync(&mut c);
                }
                let frames = data.len() / channels.max(1);
                stereo.resize(frames * 2, 0.0);
                voice.render(&mut stereo);
                for (out, s) in data.chunks_mut(channels.max(1)).zip(stereo.chunks(2)) {
                    match out.len() {
                        1 => out[0] = (s[0] + s[1]) * 0.5,
                        _ => {
                            out[0] = s[0];
                            out[1] = s[1];
                            for o in &mut out[2..] {
                                *o = 0.0;
                            }
                        }
                    }
                }
            },
            |e| eprintln!("Audio output error: {e}"),
            None,
        )
        .map_err(|e| format!("Couldn't open the audio output: {e}"))?;
    stream
        .play()
        .map_err(|e| format!("Couldn't start the audio output: {e}"))?;
    Ok(Output { _stream: stream })
}

/// Notes offered for previews, with their names (middle C = C4 = 60).
pub fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B",
    ];
    let octave = i32::from(note) / 12 - 1;
    format!("{}{octave}", NAMES[usize::from(note % 12)])
}

/// Range of preview notes: the EWI4000s' playable range is wider, but this
/// covers its useful middle.
pub const PREVIEW_NOTES: std::ops::RangeInclusive<u8> = 36..=96;
