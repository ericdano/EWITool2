//! Render patches from a `.syx` file to WAV, to compare with recordings of
//! the EWI.
//!
//! cargo run --release -p ewitool-synth --example render -- <file.syx> <out-dir> [note] [breath] [patch numbers...]
//!
//! Patch numbers are positions in the file (0-based); default is all.
//! Prints peak and RMS for each, so silent or clipping patches stand out.

use ewitool_synth::{render_phrase, Phrase};

const SR: u32 = 48000;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: render <file.syx> <out-dir> [note] [breath] [patch numbers...]");
        std::process::exit(2);
    }
    let patches = ewitool_core::patch_set::load_syx(std::path::Path::new(&args[0]))
        .unwrap_or_else(|e| panic!("can't read {}: {e}", args[0]));
    let out = std::path::Path::new(&args[1]);
    std::fs::create_dir_all(out).unwrap();
    let note = args.get(2).map_or(60, |s| s.parse().unwrap());
    let breath = args.get(3).map_or(0.8, |s| s.parse().unwrap());
    let picks: Vec<usize> = args[4.min(args.len())..]
        .iter()
        .map(|s| s.parse().unwrap())
        .collect();
    let phrase = Phrase {
        note,
        breath,
        ..Phrase::default()
    };
    for (i, p) in patches.iter().enumerate() {
        if !picks.is_empty() && !picks.contains(&i) {
            continue;
        }
        let audio = render_phrase(p, &phrase, 1.0, SR as f32);
        let peak = audio.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let rms = (audio.iter().map(|v| v * v).sum::<f32>() / audio.len() as f32).sqrt();
        let name: String = p
            .name()
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let path = out.join(format!("{i:02}-{name}.wav"));
        ewitool_synth::wav::write(&path, &audio, 2, SR).unwrap();
        println!("{i:3} {:<32} peak {peak:5.2}  rms {rms:5.3}", p.name());
    }
}
