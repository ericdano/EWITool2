//! Port of PatchSetIT.java, plus .syx load/save and edge cases.

mod common;

use common::{sample_patch, TempDir};
use ewitool_core::patch_set::{self, LegacyKind, SQS_BODY_START};
use ewitool_core::{Error, Patch, NUM_PATCHES, PATCH_LEN};

/// 100 sample patches named "Patch i" with internal number i, concatenated.
fn sample_bank() -> Vec<u8> {
    (0..NUM_PATCHES)
        .flat_map(|i| {
            let mut p = sample_patch(&format!("Patch {i}"), i as i64);
            p[5] = i as u8;
            p
        })
        .collect()
}

fn legacy_file(body_marker: &[u8], patches: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0u8; 64];
    bytes.extend_from_slice(body_marker);
    bytes.extend_from_slice(patches);
    bytes
}

fn assert_converted(ext: &str, body_marker: &[u8]) {
    let dir = TempDir::new(ext);
    let patches = sample_bank();
    let legacy = dir.path().join(format!("bank.{ext}"));
    std::fs::write(&legacy, legacy_file(body_marker, &patches)).unwrap();

    let (syx_path, count) = patch_set::convert_legacy(&legacy).unwrap();
    assert_eq!(count, NUM_PATCHES);
    assert_eq!(syx_path, dir.path().join("bank.syx"));

    let syx = std::fs::read(&syx_path).unwrap();
    assert_eq!(syx, patches);
    let last = Patch::from_bytes(&syx[99 * PATCH_LEN..]).unwrap();
    assert_eq!(last.name(), "Patch 99");
}

#[test]
fn converts_bnk() {
    assert_converted("bnk", &[b'B', b'O', b'D', b'Y', 0, 0, 0, 0]);
}

#[test]
fn converts_sqs() {
    // The SQS marker's last 6 bytes are the first patch's header, so only
    // the first 8 precede it.
    assert_converted("sqs", &SQS_BODY_START[..8]);
}

#[test]
fn converts_uppercase_extension() {
    assert_converted("BNK", &[b'B', b'O', b'D', b'Y', 0, 0, 0, 0]);
}

#[test]
fn legacy_skips_bad_patches_after_the_first() {
    let mut patches = sample_bank();
    patches[2 * PATCH_LEN - 1] = 0x00; // break patch #1's trailer
    let bytes = legacy_file(b"BODY\0\0\0\0", &patches);
    let out = patch_set::extract_legacy(&bytes, LegacyKind::Bnk).unwrap();
    assert_eq!(out.len(), NUM_PATCHES - 1);
    assert_eq!(out[1].name(), "Patch 2");
}

#[test]
fn legacy_truncated_bank_keeps_whole_patches() {
    let patches = sample_bank();
    // 10 whole patches plus half of the 11th.
    let bytes = legacy_file(b"BODY\0\0\0\0", &patches[..10 * PATCH_LEN + 100]);
    let out = patch_set::extract_legacy(&bytes, LegacyKind::Bnk).unwrap();
    assert_eq!(out.len(), 10);
}

#[test]
fn legacy_errors() {
    assert!(matches!(
        patch_set::extract_legacy(&[0; 500], LegacyKind::Bnk),
        Err(Error::BodyNotFound)
    ));
    let mut patches = sample_bank();
    patches[PATCH_LEN - 1] = 0x00; // break the first patch
    assert!(matches!(
        patch_set::extract_legacy(&legacy_file(b"BODY\0\0\0\0", &patches), LegacyKind::Bnk),
        Err(Error::FirstPatchMissing)
    ));
    assert!(matches!(
        patch_set::extract_legacy(b"BODY\0\0\0\0", LegacyKind::Bnk),
        Err(Error::FirstPatchMissing)
    ));
    assert!(matches!(
        patch_set::convert_legacy(std::path::Path::new("bank.txt")),
        Err(Error::UnknownLegacyType)
    ));
}

#[test]
fn convert_legacy_does_not_overwrite() {
    let dir = TempDir::new("nooverwrite");
    let legacy = dir.path().join("bank.bnk");
    std::fs::write(&legacy, legacy_file(b"BODY\0\0\0\0", &sample_bank())).unwrap();
    std::fs::write(dir.path().join("bank.syx"), b"keep me").unwrap();

    match patch_set::convert_legacy(&legacy) {
        Err(Error::Io(e)) => assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists),
        other => panic!("expected AlreadyExists, got {other:?}"),
    }
    assert_eq!(
        std::fs::read(dir.path().join("bank.syx")).unwrap(),
        b"keep me"
    );
}

#[test]
fn syx_save_then_load_round_trips() {
    let dir = TempDir::new("syx");
    let bank = sample_bank();
    let patches = patch_set::parse_syx(&bank).unwrap();
    assert_eq!(patches.len(), NUM_PATCHES);

    let path = dir.path().join("set.syx");
    patch_set::save_syx(&path, &patches).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bank);
    assert_eq!(patch_set::load_syx(&path).unwrap(), patches);

    // Saving again must not overwrite.
    assert!(matches!(
        patch_set::save_syx(&path, &patches),
        Err(Error::Io(_))
    ));
}

#[test]
fn save_single_patch() {
    let dir = TempDir::new("single");
    let p = Patch::from_bytes(&sample_patch("One", 11)).unwrap();
    let path = dir.path().join("one.syx");
    patch_set::save_patch(&path, &p).unwrap();
    assert_eq!(patch_set::load_syx(&path).unwrap(), vec![p]);
}

#[test]
fn parse_syx_rejects_partial_patch() {
    assert!(patch_set::parse_syx(&[]).unwrap().is_empty());
    assert!(matches!(
        patch_set::parse_syx(&[0; PATCH_LEN + 1]),
        Err(Error::WrongLength { .. })
    ));
}
