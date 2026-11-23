//! Patch library files: native `.syx` files (concatenated 206-byte patches)
//! and import of legacy `.bnk` / `.sqs` soundbanks.
//!
//! Port of `PatchSet.java` plus the `.syx` loading loops from
//! `PatchSetsTab.java` and `ScratchPad.java`. Nothing here shows UI; callers
//! decide how to report errors.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::patch::{Patch, NUM_PATCHES, PATCH_LEN};

/// Marker before the patch data in a .bnk file.
pub const BNK_BODY_START: &[u8] = b"BODY";
/// Marker before the patch data in a .sqs file. SQS files contain several
/// false "BODY"s; this longer sequence finds the real one. Its last 6 bytes
/// are the first patch's header.
pub const SQS_BODY_START: &[u8] = &[
    b'B', b'O', b'D', b'Y', 0x00, 0x00, 0x50, 0x78, 0xf0, 0x47, 0x64, 0x7f, 0x00, 0x00,
];
/// Patch data starts this many bytes after the marker.
const BODY_SKIP: usize = 8;

/// Decode a `.syx` file's contents. An empty file is an empty set; any other
/// length must be a whole number of patches.
pub fn parse_syx(bytes: &[u8]) -> Result<Vec<Patch>, Error> {
    if !bytes.len().is_multiple_of(PATCH_LEN) {
        return Err(Error::WrongLength {
            expected: PATCH_LEN,
            actual: bytes.len(),
        });
    }
    Ok(bytes
        .as_chunks::<PATCH_LEN>()
        .0
        .iter()
        .map(Patch::decode)
        .collect())
}

/// Encode patches as a `.syx` file's contents.
pub fn to_syx(patches: &[Patch]) -> Vec<u8> {
    patches.iter().flat_map(|p| p.to_bytes()).collect()
}

pub fn load_syx(path: &Path) -> Result<Vec<Patch>, Error> {
    parse_syx(&std::fs::read(path)?)
}

/// Write patches to a new `.syx` file. Fails with an `AlreadyExists` I/O
/// error rather than overwriting.
pub fn save_syx(path: &Path, patches: &[Patch]) -> Result<(), Error> {
    write_new_file(path, &to_syx(patches))
}

/// Export one patch to a new `.syx` file (Java `EWI4000sPatch.save`).
pub fn save_patch(path: &Path, patch: &Patch) -> Result<(), Error> {
    write_new_file(path, &patch.to_bytes())
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let mut f = OpenOptions::new().write(true).create_new(true).open(path)?;
    f.write_all(bytes)?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyKind {
    Bnk,
    Sqs,
}

impl LegacyKind {
    /// Detect from the file extension (case-insensitive).
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "bnk" => Some(Self::Bnk),
            "sqs" => Some(Self::Sqs),
            _ => None,
        }
    }

    fn body_marker(self) -> &'static [u8] {
        match self {
            Self::Bnk => BNK_BODY_START,
            Self::Sqs => SQS_BODY_START,
        }
    }
}

/// Pull the patches out of a legacy soundbank's contents.
///
/// Reads up to 100 patches after the BODY marker. Patches not ending in 0xF7
/// are skipped (as in Java); a bad first patch is an error. Unlike the Java
/// version, a truncated bank yields the patches that are present instead of
/// throwing.
pub fn extract_legacy(bytes: &[u8], kind: LegacyKind) -> Result<Vec<Patch>, Error> {
    let marker = kind.body_marker();
    let start = bytes
        .windows(marker.len())
        .position(|w| w == marker)
        .ok_or(Error::BodyNotFound)?
        + BODY_SKIP;

    let body = bytes.get(start..).unwrap_or_default();
    let mut patches = Vec::new();
    for (i, chunk) in body
        .as_chunks::<PATCH_LEN>()
        .0
        .iter()
        .take(NUM_PATCHES)
        .enumerate()
    {
        let patch = Patch::decode(chunk);
        if patch.has_sysex_trailer() {
            patches.push(patch);
        } else if i == 0 {
            return Err(Error::FirstPatchMissing);
        }
    }
    if patches.is_empty() {
        return Err(Error::FirstPatchMissing);
    }
    Ok(patches)
}

/// Import a legacy `.bnk` / `.sqs` file, writing a `.syx` file with the same
/// name next to it. Returns the new file's path and the number of patches
/// written. Never overwrites an existing `.syx`.
pub fn convert_legacy(legacy: &Path) -> Result<(PathBuf, usize), Error> {
    convert_legacy_to(legacy, legacy.parent().unwrap_or(Path::new("")))
}

/// Like [`convert_legacy`], but writes the `.syx` file into `out_dir`.
pub fn convert_legacy_to(legacy: &Path, out_dir: &Path) -> Result<(PathBuf, usize), Error> {
    let kind = LegacyKind::from_path(legacy).ok_or(Error::UnknownLegacyType)?;
    let patches = extract_legacy(&std::fs::read(legacy)?, kind)?;
    let name = legacy.file_name().ok_or(Error::UnknownLegacyType)?;
    let syx = out_dir.join(name).with_extension("syx");
    save_syx(&syx, &patches)?;
    Ok((syx, patches.len()))
}
