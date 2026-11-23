//! One EWI4000s patch: a 206-byte sysex message, decoded into typed fields.
//!
//! Port of `EWI4000sPatch.java`. The Java class kept the raw blob and the decoded
//! fields side by side and had to keep them in sync by hand; here the typed
//! [`Patch`] is the only copy, and [`Patch::to_bytes`] produces the blob on demand.

use crate::error::Error;

/// Number of patches the EWI holds.
pub const NUM_PATCHES: usize = 100;
/// Length of one patch sysex message, including the leading 0xF0 and trailing 0xF7.
pub const PATCH_LEN: usize = 206;
/// Length of the space-padded patch name.
pub const NAME_LEN: usize = 32;

pub const SYSEX_HEADER: u8 = 0xF0;
pub const SYSEX_TRAILER: u8 = 0xF7;

/// Whether a patch sent to the EWI is stored or only loaded into the edit buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Store into the patch's slot (Java `EWI_SAVE`).
    Save = 0x00,
    /// Load into the edit buffer only (Java `EWI_EDIT`).
    Edit = 0x20,
}

/// Patch numbering: the player sees patches 0-99, but the device stores the
/// patch shown as #0 under internal number 99, and patch #n under n-1.
pub fn user_to_internal(user: u8) -> Option<u8> {
    match user {
        0 => Some(99),
        1..=99 => Some(user - 1),
        _ => None,
    }
}

/// Inverse of [`user_to_internal`].
pub fn internal_to_user(internal: u8) -> Option<u8> {
    match internal {
        99 => Some(0),
        0..=98 => Some(internal + 1),
        _ => None,
    }
}

/// The 3-byte NRPN prefix that starts each parameter section.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Nrpn {
    pub lsb: u8,
    pub msb: u8,
    pub offset: u8,
}

impl Nrpn {
    fn read(b: &[u8]) -> Self {
        Self {
            lsb: b[0],
            msb: b[1],
            offset: b[2],
        }
    }

    fn write(&self, b: &mut [u8]) {
        b[..3].copy_from_slice(&[self.lsb, self.msb, self.offset]);
    }
}

/// Defines a parameter section: an [`Nrpn`] followed by one byte per field.
/// **Fields must be listed in byte order**; their position is their offset.
macro_rules! section {
    ($(#[$meta:meta])* $name:ident { $($(#[$fmeta:meta])* $field:ident),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub struct $name {
            pub nrpn: Nrpn,
            $($(#[$fmeta])* pub $field: u8,)+
        }

        impl $name {
            /// Length in bytes, including the NRPN prefix.
            pub const LEN: usize = 3 + [$(stringify!($field)),+].len();

            fn read(b: &[u8]) -> Self {
                let mut bytes = b[3..Self::LEN].iter().copied();
                Self {
                    nrpn: Nrpn::read(b),
                    $($field: bytes.next().unwrap(),)+
                }
            }

            fn write(&self, b: &mut [u8]) {
                self.nrpn.write(b);
                b[3..Self::LEN].copy_from_slice(&[$(self.$field),+]);
            }
        }
    };
}

section! {
    /// Oscillator (NRPN 64 / 65, 18 values).
    Osc {
        /// 64 +/- 2
        octave,
        /// 64 +/- 12
        semitone,
        /// -50 to +50 cents
        fine,
        beat,
        /// Unused (firmware 2.3); kept so round trips are exact.
        filler1,
        sawtooth,
        triangle,
        square,
        pulse_width,
        pwm_freq,
        pwm_depth,
        sweep_depth,
        sweep_time,
        breath_depth,
        breath_attain,
        breath_curve,
        breath_threshold,
        level,
    }
}

section! {
    /// Filter (NRPN 72-75, 12 values).
    Filter {
        mode,
        freq,
        q,
        key_follow,
        breath_mod,
        lfo_freq,
        lfo_depth,
        lfo_breath,
        lfo_threshold,
        sweep_depth,
        sweep_time,
        breath_curve,
    }
}

section! {
    /// Anti-alias filter (NRPN 79, 3 values).
    AntiAlias { switch, cutoff, key_follow }
}

section! {
    /// Noise generator (NRPN 80, 3 values).
    Noise { time, breath, level }
}

section! {
    /// Miscellaneous settings (NRPN 81, 10 values).
    Misc {
        bend_range,
        bend_step_mode,
        bite_vibrato,
        osc_filter_link,
        noise_filter_link,
        formant_filter,
        osc2_xfade,
        key_trigger,
        /// Unused; kept so round trips are exact.
        filler10,
        chorus_switch,
    }
}

section! {
    /// Amplifier (NRPN 88, 3 values).
    Amp { bite_tremolo, level, octave_level }
}

section! {
    /// Chorus (NRPN 112, 9 values).
    Chorus {
        delay1,
        mod_lev1,
        wet_lev1,
        delay2,
        mod_lev2,
        wet_lev2,
        feedback,
        lfo_freq,
        dry_level,
    }
}

section! {
    /// Delay (NRPN 113, 5 values).
    Delay { time, feedback, damp, level, dry }
}

section! {
    /// Reverb (NRPN 114, 5 values).
    Reverb { dry, level, density, time, damp }
}

// Byte offsets of each part of the blob. These must match the device format.
mod off {
    pub const HEADER: usize = 0;
    pub const MODE: usize = 4;
    pub const INTERNAL_NUM: usize = 5;
    pub const FILLERS: usize = 6;
    pub const NAME: usize = 12;
    pub const OSC1: usize = 44;
    pub const OSC2: usize = 65;
    pub const OSC_FILTER1: usize = 86;
    pub const OSC_FILTER2: usize = 101;
    pub const NOISE_FILTER1: usize = 116;
    pub const NOISE_FILTER2: usize = 131;
    pub const ANTI_ALIAS: usize = 146;
    pub const NOISE: usize = 152;
    pub const MISC: usize = 158;
    pub const AMP: usize = 171;
    pub const CHORUS: usize = 177;
    pub const DELAY: usize = 189;
    pub const REVERB: usize = 197;
    pub const TRAILER: usize = 205;
}

// Compile-time proof that the sections tile the blob with no gaps or overlaps.
const _: () = {
    use off::*;
    assert!(HEADER + 4 == MODE);
    assert!(MODE + 1 == INTERNAL_NUM);
    assert!(INTERNAL_NUM + 1 == FILLERS);
    assert!(FILLERS + 6 == NAME);
    assert!(NAME + NAME_LEN == OSC1);
    assert!(OSC1 + Osc::LEN == OSC2);
    assert!(OSC2 + Osc::LEN == OSC_FILTER1);
    assert!(OSC_FILTER1 + Filter::LEN == OSC_FILTER2);
    assert!(OSC_FILTER2 + Filter::LEN == NOISE_FILTER1);
    assert!(NOISE_FILTER1 + Filter::LEN == NOISE_FILTER2);
    assert!(NOISE_FILTER2 + Filter::LEN == ANTI_ALIAS);
    assert!(ANTI_ALIAS + AntiAlias::LEN == NOISE);
    assert!(NOISE + Noise::LEN == MISC);
    assert!(MISC + Misc::LEN == AMP);
    assert!(AMP + Amp::LEN == CHORUS);
    assert!(CHORUS + Chorus::LEN == DELAY);
    assert!(DELAY + Delay::LEN == REVERB);
    assert!(REVERB + Reverb::LEN == TRAILER);
    assert!(TRAILER + 1 == PATCH_LEN);
};

/// A decoded EWI4000s patch.
///
/// Every byte of the blob is kept (including unused filler bytes), so
/// `Patch::from_bytes(b)?.to_bytes() == b` for any 206-byte input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Patch {
    /// Sysex header: F0 47 64 7F for a valid patch.
    pub header: [u8; 4],
    /// Raw mode byte: 0x00 store, 0x20 edit. Set with [`Patch::set_mode`].
    pub mode: u8,
    /// Device-internal patch number (see [`user_to_internal`]).
    pub internal_num: u8,
    pub fillers: [u8; 6],
    /// Space-padded name bytes. Use [`Patch::name`] / [`Patch::set_name`].
    pub name_bytes: [u8; NAME_LEN],
    pub osc1: Osc,
    pub osc2: Osc,
    pub osc_filter1: Filter,
    pub osc_filter2: Filter,
    pub noise_filter1: Filter,
    pub noise_filter2: Filter,
    pub anti_alias: AntiAlias,
    pub noise: Noise,
    pub misc: Misc,
    pub amp: Amp,
    pub chorus: Chorus,
    pub delay: Delay,
    pub reverb: Reverb,
    /// Sysex trailer: 0xF7 for a valid patch.
    pub trailer: u8,
}

impl Patch {
    /// Decode a 206-byte patch blob.
    pub fn from_bytes(b: &[u8]) -> Result<Self, Error> {
        let b: &[u8; PATCH_LEN] = b.try_into().map_err(|_| Error::WrongLength {
            expected: PATCH_LEN,
            actual: b.len(),
        })?;
        Ok(Self::decode(b))
    }

    /// Decode a fixed-size patch blob (cannot fail).
    pub fn decode(b: &[u8; PATCH_LEN]) -> Self {
        use off::*;
        Self {
            header: b[HEADER..MODE].try_into().unwrap(),
            mode: b[MODE],
            internal_num: b[INTERNAL_NUM],
            fillers: b[FILLERS..NAME].try_into().unwrap(),
            name_bytes: b[NAME..OSC1].try_into().unwrap(),
            osc1: Osc::read(&b[OSC1..]),
            osc2: Osc::read(&b[OSC2..]),
            osc_filter1: Filter::read(&b[OSC_FILTER1..]),
            osc_filter2: Filter::read(&b[OSC_FILTER2..]),
            noise_filter1: Filter::read(&b[NOISE_FILTER1..]),
            noise_filter2: Filter::read(&b[NOISE_FILTER2..]),
            anti_alias: AntiAlias::read(&b[ANTI_ALIAS..]),
            noise: Noise::read(&b[NOISE..]),
            misc: Misc::read(&b[MISC..]),
            amp: Amp::read(&b[AMP..]),
            chorus: Chorus::read(&b[CHORUS..]),
            delay: Delay::read(&b[DELAY..]),
            reverb: Reverb::read(&b[REVERB..]),
            trailer: b[TRAILER],
        }
    }

    /// Encode back to the 206-byte sysex blob.
    pub fn to_bytes(&self) -> [u8; PATCH_LEN] {
        use off::*;
        let mut b = [0u8; PATCH_LEN];
        b[HEADER..MODE].copy_from_slice(&self.header);
        b[MODE] = self.mode;
        b[INTERNAL_NUM] = self.internal_num;
        b[FILLERS..NAME].copy_from_slice(&self.fillers);
        b[NAME..OSC1].copy_from_slice(&self.name_bytes);
        self.osc1.write(&mut b[OSC1..]);
        self.osc2.write(&mut b[OSC2..]);
        self.osc_filter1.write(&mut b[OSC_FILTER1..]);
        self.osc_filter2.write(&mut b[OSC_FILTER2..]);
        self.noise_filter1.write(&mut b[NOISE_FILTER1..]);
        self.noise_filter2.write(&mut b[NOISE_FILTER2..]);
        self.anti_alias.write(&mut b[ANTI_ALIAS..]);
        self.noise.write(&mut b[NOISE..]);
        self.misc.write(&mut b[MISC..]);
        self.amp.write(&mut b[AMP..]);
        self.chorus.write(&mut b[CHORUS..]);
        self.delay.write(&mut b[DELAY..]);
        self.reverb.write(&mut b[REVERB..]);
        b[TRAILER] = self.trailer;
        b
    }

    /// Cheap sanity check used when importing legacy banks: ends in 0xF7.
    pub fn has_sysex_trailer(&self) -> bool {
        self.trailer == SYSEX_TRAILER
    }

    /// The patch name with padding removed (same trimming as Java's `String.trim`).
    pub fn name(&self) -> String {
        // Bytes map 1:1 to chars (Latin-1), matching Java's (char) cast for 0-127.
        let s: String = self.name_bytes.iter().map(|&b| b as char).collect();
        s.trim_matches(|c: char| c <= ' ').to_string()
    }

    /// Set the name: 1-32 printable ASCII characters, space-padded on the right.
    pub fn set_name(&mut self, name: &str) -> Result<(), Error> {
        if name.is_empty() || name.len() > NAME_LEN {
            return Err(Error::InvalidName(format!(
                "name must be 1-{NAME_LEN} characters long"
            )));
        }
        if !name.bytes().all(|b| (0x20..=0x7E).contains(&b)) {
            return Err(Error::InvalidName(
                "name may only contain printable ASCII characters".into(),
            ));
        }
        self.name_bytes = [b' '; NAME_LEN];
        self.name_bytes[..name.len()].copy_from_slice(name.as_bytes());
        Ok(())
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode as u8;
    }

    /// The number the player sees (0-99), if the internal number is valid.
    pub fn user_num(&self) -> Option<u8> {
        internal_to_user(self.internal_num)
    }

    /// Parse a patch from 412 hex digits (ASCII whitespace is ignored).
    pub fn from_hex(hex: &str) -> Result<Self, Error> {
        let digits: Vec<u8> = hex.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
        if !digits.len().is_multiple_of(2) {
            return Err(Error::BadHex);
        }
        let bytes = digits
            .chunks(2)
            .map(|pair| {
                let s = std::str::from_utf8(pair).map_err(|_| Error::BadHex)?;
                u8::from_str_radix(s, 16).map_err(|_| Error::BadHex)
            })
            .collect::<Result<Vec<u8>, Error>>()?;
        Self::from_bytes(&bytes)
    }

    /// Lowercase hex, optionally with a space after every byte (as the Java version).
    pub fn to_hex(&self, spaced: bool) -> String {
        let sep = if spaced { " " } else { "" };
        self.to_bytes()
            .iter()
            .map(|b| format!("{b:02x}{sep}"))
            .collect()
    }
}
