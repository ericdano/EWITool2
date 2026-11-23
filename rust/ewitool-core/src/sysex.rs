//! SysEx messages exchanged with the EWI (port of the constants and parsing
//! in `MidiHandler.java` / `MidiReceiver.java`).
//!
//! Byte slices here are whole messages, including the leading 0xF0 and the
//! trailing 0xF7 (Java's `SysexMessage.getData()` dropped the 0xF0, so its
//! indexes are one lower).

use crate::patch::{Patch, PATCH_LEN};

const NON_REALTIME: u8 = 0x7E;
const ALL_CHANNELS: u8 = 0x7F;
const GENERAL_INFO: u8 = 0x06;
const ID_REQUEST: u8 = 0x01;
const ID_REPLY: u8 = 0x02;
pub const AKAI_ID: u8 = 0x47;
pub const EWI4000S_ID: u8 = 0x64;

/// Universal "identity request": asks any device to say what it is.
pub const DEVICE_ID_REQUEST: [u8; 6] = [
    0xF0,
    NON_REALTIME,
    ALL_CHANNELS,
    GENERAL_INFO,
    ID_REQUEST,
    0xF7,
];

/// What answered an identity request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceId {
    Ewi4000s,
    /// An Akai device that isn't an EWI4000s, with its family code.
    OtherAkai(u8),
    /// A device from another manufacturer, with its manufacturer code.
    NotAkai(u8),
}

/// Parse an identity reply (`F0 7E <device> 06 02 <manufacturer> <family> … F7`).
/// Returns `None` for any other message.
///
/// The Java version also required the device byte to be 0 and the message
/// to be exactly 15 bytes; this accepts any device number and length, so a
/// firmware with a slightly different reply is still recognised.
pub fn parse_device_id(msg: &[u8]) -> Option<DeviceId> {
    match msg {
        [0xF0, NON_REALTIME, _, GENERAL_INFO, ID_REPLY, mfr, family, ..] => {
            Some(match (*mfr, *family) {
                (AKAI_ID, EWI4000S_ID) => DeviceId::Ewi4000s,
                (AKAI_ID, f) => DeviceId::OtherAkai(f),
                (m, _) => DeviceId::NotAkai(m),
            })
        }
        _ => None,
    }
}

const PRESET_DUMP: u8 = 0x00;
const PRESET_DUMP_REQ: u8 = 0x40;
const CHANNEL: u8 = 0x00;

/// Ask the EWI for the patch stored under `internal` (0-99, the device's
/// own numbering; see [`crate::patch::user_to_internal`]).
pub fn patch_request(internal: u8) -> [u8; 7] {
    [
        0xF0,
        AKAI_ID,
        EWI4000S_ID,
        CHANNEL,
        PRESET_DUMP_REQ,
        internal,
        0xF7,
    ]
}

/// Decode a patch dump sent by the EWI in reply to [`patch_request`].
/// Returns `None` for any other message.
pub fn parse_patch_dump(msg: &[u8]) -> Option<Patch> {
    match msg {
        [0xF0, AKAI_ID, EWI4000S_ID, ALL_CHANNELS, PRESET_DUMP, ..]
            if msg.len() == PATCH_LEN && msg[PATCH_LEN - 1] == 0xF7 =>
        {
            Patch::from_bytes(msg).ok()
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_request_bytes() {
        assert_eq!(patch_request(99), [0xF0, 0x47, 0x64, 0x00, 0x40, 99, 0xF7]);
    }

    #[test]
    fn parses_patch_dump() {
        let mut dump = [0u8; PATCH_LEN];
        dump[..6].copy_from_slice(&[0xF0, 0x47, 0x64, 0x7F, 0x00, 12]);
        dump[PATCH_LEN - 1] = 0xF7;
        let patch = parse_patch_dump(&dump).unwrap();
        assert_eq!(patch.internal_num, 12);
        assert_eq!(patch.to_bytes(), dump);

        assert!(parse_patch_dump(&dump[..PATCH_LEN - 1]).is_none());
        let mut edit_dump = dump;
        edit_dump[4] = 0x20;
        assert!(parse_patch_dump(&edit_dump).is_none());
        assert!(parse_patch_dump(&patch_request(12)).is_none());
    }

    #[test]
    fn request_bytes() {
        assert_eq!(DEVICE_ID_REQUEST, [0xF0, 0x7E, 0x7F, 0x06, 0x01, 0xF7]);
    }

    #[test]
    fn parses_ewi_reply() {
        // 15-byte reply as the Java code expects: F0 7E 00 06 02 47 64 + 7 bytes + F7.
        let reply = [
            0xF0, 0x7E, 0x00, 0x06, 0x02, 0x47, 0x64, 0x00, 0x19, 0x00, 0x01, 0x02, 0x03, 0x00,
            0xF7,
        ];
        assert_eq!(parse_device_id(&reply), Some(DeviceId::Ewi4000s));
        // Any device number is accepted.
        let mut other_dev = reply;
        other_dev[2] = 0x10;
        assert_eq!(parse_device_id(&other_dev), Some(DeviceId::Ewi4000s));
    }

    #[test]
    fn parses_other_devices() {
        let akai = [0xF0, 0x7E, 0x00, 0x06, 0x02, 0x47, 0x12, 0x00, 0xF7];
        assert_eq!(parse_device_id(&akai), Some(DeviceId::OtherAkai(0x12)));
        let roland = [0xF0, 0x7E, 0x10, 0x06, 0x02, 0x41, 0x00, 0x00, 0xF7];
        assert_eq!(parse_device_id(&roland), Some(DeviceId::NotAkai(0x41)));
    }

    #[test]
    fn ignores_other_messages() {
        assert_eq!(parse_device_id(&DEVICE_ID_REQUEST), None);
        assert_eq!(parse_device_id(&[0x90, 60, 100]), None);
        assert_eq!(parse_device_id(&[0xF0, 0x7E, 0x00, 0x06, 0x02, 0xF7]), None);
        assert_eq!(parse_device_id(&[]), None);
    }
}
