//! Core EWI4000s logic for EWITool2: the patch format, library files, the
//! 100-slot patch bank, the scratchpad and whole-patch edit operations.
//! No GUI and no MIDI, so everything here is testable headless.

pub mod bank;
pub mod edit;
pub mod error;
pub mod fetch;
pub mod outgoing;
pub mod patch;
pub mod patch_set;
pub mod scratchpad;
pub mod sysex;

pub use bank::Bank;
pub use error::Error;
pub use patch::{Mode, Patch, NAME_LEN, NUM_PATCHES, PATCH_LEN};
pub use scratchpad::ScratchPad;
