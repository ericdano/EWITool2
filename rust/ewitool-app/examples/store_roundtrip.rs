//! Hardware check for sending: read one patch from the EWI, store the
//! identical patch back into the same slot, read it again and compare.
//! Optionally compares the first read against a backup `.syx` first, and
//! refuses to write if they differ.
//!
//! cargo run --example store_roundtrip -- "<MIDI port>" <slot 0-99> [backup.syx]

use std::thread::sleep;
use std::time::Duration;

use ewitool_app::midi::Link;
use ewitool_core::fetch::Fetcher;
use ewitool_core::outgoing::{patch_messages, PATCH_PAUSE};
use ewitool_core::{patch_set, Mode};

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let (Some(port), Some(slot)) = (
        args.get(1),
        args.get(2).and_then(|s| s.parse::<usize>().ok()),
    ) else {
        return Err("usage: store_roundtrip <port> <slot 0-99> [backup.syx]".into());
    };
    let mut link = Link::open(port)?;

    let before = link.fetch(Fetcher::only(slot), |_| {})?;
    let patch = before.get(slot).ok_or("no patch received")?.clone();
    println!("Read #{slot} \"{}\"", patch.name());

    if let Some(backup) = args.get(3) {
        let saved = patch_set::load_syx(std::path::Path::new(backup)).map_err(|e| e.to_string())?;
        let in_backup = saved.iter().find(|p| p.internal_num == patch.internal_num);
        if in_backup != Some(&patch) {
            return Err(format!(
                "#{slot} differs from the backup; not writing anything"
            ));
        }
        println!("Matches the backup");
    }

    for msg in patch_messages(&patch, Mode::Save) {
        link.send(&msg)?;
        sleep(PATCH_PAUSE);
    }
    println!("Stored the identical patch back into #{slot}");
    sleep(Duration::from_millis(500));

    let after = link.fetch(Fetcher::only(slot), |_| {})?;
    let again = after.get(slot).ok_or("no patch received")?;
    if again.to_bytes() == patch.to_bytes() {
        println!("Read #{slot} back: all 206 bytes identical. Sending works.");
        Ok(())
    } else {
        let diffs: Vec<usize> = (0..206)
            .filter(|&i| again.to_bytes()[i] != patch.to_bytes()[i])
            .collect();
        Err(format!(
            "#{slot} changed after storing; differing bytes at {diffs:?}"
        ))
    }
}
