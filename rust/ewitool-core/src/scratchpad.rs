//! The Scratchpad: a persistent working list of patches, saved as
//! `SCRATCHPAD.BIN` (concatenated patches) in the library folder.
//!
//! Port of `ScratchPad.java`. Every change is written to disk immediately.
//! Without a library folder the scratchpad still works, but only in memory.

use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::patch::Patch;
use crate::patch_set;

pub const SCRATCHPAD_NAME: &str = "SCRATCHPAD.BIN";

#[derive(Debug, Default)]
pub struct ScratchPad {
    dir: Option<PathBuf>,
    patches: Vec<Patch>,
}

impl ScratchPad {
    /// Load the scratchpad from `dir` (missing file = empty scratchpad).
    pub fn load(dir: Option<&Path>) -> Result<Self, Error> {
        let mut sp = Self {
            dir: dir.map(Path::to_path_buf),
            patches: Vec::new(),
        };
        if let Some(path) = sp.path() {
            match std::fs::read(&path) {
                Ok(bytes) => sp.patches = patch_set::parse_syx(&bytes)?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(sp)
    }

    /// An in-memory scratchpad that is never saved.
    pub fn in_memory() -> Self {
        Self::default()
    }

    pub fn path(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(SCRATCHPAD_NAME))
    }

    pub fn is_persistent(&self) -> bool {
        self.dir.is_some()
    }

    pub fn patches(&self) -> &[Patch] {
        &self.patches
    }

    pub fn len(&self) -> usize {
        self.patches.len()
    }

    pub fn is_empty(&self) -> bool {
        self.patches.is_empty()
    }

    pub fn add(&mut self, patch: Patch) -> Result<(), Error> {
        self.patches.push(patch);
        self.store()
    }

    /// Replace the patch at `ix` (e.g. after editing it).
    pub fn replace(&mut self, ix: usize, patch: Patch) -> Result<(), Error> {
        if let Some(p) = self.patches.get_mut(ix) {
            *p = patch;
        }
        self.store()
    }

    pub fn remove(&mut self, ix: usize) -> Result<(), Error> {
        if ix < self.patches.len() {
            self.patches.remove(ix);
        }
        self.store()
    }

    /// Rename a patch. On an invalid name nothing changes.
    pub fn rename(&mut self, ix: usize, name: &str) -> Result<(), Error> {
        if let Some(p) = self.patches.get_mut(ix) {
            p.set_name(name)?;
        }
        self.store()
    }

    pub fn clear(&mut self) -> Result<(), Error> {
        self.patches.clear();
        self.store()
    }

    /// Write to disk, replacing the old file only once the new one is complete.
    fn store(&self) -> Result<(), Error> {
        let Some(path) = self.path() else {
            return Ok(());
        };
        let tmp = path.with_extension("BIN.tmp");
        std::fs::write(&tmp, patch_set::to_syx(&self.patches))?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }
}
