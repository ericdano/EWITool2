//! Shared test helpers.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use ewitool_core::{NAME_LEN, PATCH_LEN};

/// Bit-exact port of `java.util.Random`, so these tests generate the same
/// sample patches as the Java tests for the same seed.
pub struct JavaRandom(u64);

impl JavaRandom {
    const MULT: u64 = 0x5_DEEC_E66D;
    const MASK: u64 = (1 << 48) - 1;

    pub fn new(seed: i64) -> Self {
        Self((seed as u64 ^ Self::MULT) & Self::MASK)
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.0 = (self.0.wrapping_mul(Self::MULT).wrapping_add(0xB)) & Self::MASK;
        (self.0 >> (48 - bits)) as i32
    }

    pub fn next_int(&mut self) -> i32 {
        self.next(32)
    }

    pub fn next_bytes(&mut self, out: &mut [u8]) {
        let mut i = 0;
        while i < out.len() {
            let mut rnd = self.next_int();
            for _ in 0..(out.len() - i).min(4) {
                out[i] = rnd as u8;
                rnd >>= 8;
                i += 1;
            }
        }
    }
}

/// Same as `EWI4000sPatchTest.samplePatch` in the Java tests.
pub fn sample_patch(name: &str, seed: i64) -> [u8; PATCH_LEN] {
    let mut blob = [0u8; PATCH_LEN];
    JavaRandom::new(seed).next_bytes(&mut blob);
    blob[..5].copy_from_slice(&[0xf0, 0x47, 0x64, 0x7f, 0x00]);
    for i in 0..NAME_LEN {
        blob[12 + i] = *name.as_bytes().get(i).unwrap_or(&b' ');
    }
    blob[PATCH_LEN - 1] = 0xf7;
    blob
}

/// A temporary directory removed on drop (avoids a tempfile dependency).
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        // A per-process counter keeps names unique even when tests run in
        // parallel within the same clock tick, and when tags differ only in
        // case (macOS file names are case-insensitive).
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "ewitool-test-{}-{n}-{}",
            std::process::id(),
            tag.to_lowercase()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
