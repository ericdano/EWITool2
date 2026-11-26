//! The 100 patch slots of the EWI (the "Current Patch Set").
//!
//! Replaces `SharedData.ewiPatchList` / `ewiPatchNums`. Slots are indexed by
//! the number the player sees (0-99); each stored patch's `internal_num` is
//! kept in step with its slot.

use crate::patch::{internal_to_user, user_to_internal, Patch, NUM_PATCHES};

/// How an empty slot is shown, and the name of the blank patch stored in
/// the EWI for it (see [`crate::edit::blank_patch`]).
pub const EMPTY_NAME: &str = "<Empty>";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bank {
    slots: Vec<Option<Patch>>,
}

impl Default for Bank {
    fn default() -> Self {
        Self {
            slots: vec![None; NUM_PATCHES],
        }
    }
}

impl Bank {
    /// Build a bank from a patch set, placing each patch in the slot its
    /// internal number belongs to (as the EWI does when the set is sent).
    /// Patches with an invalid or duplicate number fill the remaining slots
    /// in order; anything beyond 100 patches is dropped.
    pub fn from_patches(patches: impl IntoIterator<Item = Patch>) -> Self {
        let mut bank = Self::default();
        let mut leftovers = Vec::new();
        for patch in patches {
            match internal_to_user(patch.internal_num) {
                Some(user) if bank.slots[user as usize].is_none() => bank.set(user as usize, patch),
                _ => leftovers.push(patch),
            }
        }
        for patch in leftovers {
            if let Some(free) = bank.slots.iter().position(Option::is_none) {
                bank.set(free, patch);
            }
        }
        bank
    }

    pub fn get(&self, slot: usize) -> Option<&Patch> {
        self.slots.get(slot)?.as_ref()
    }

    pub fn get_mut(&mut self, slot: usize) -> Option<&mut Patch> {
        self.slots.get_mut(slot)?.as_mut()
    }

    /// Put a patch in a slot, renumbering it to match. Panics if `slot >= 100`.
    pub fn set(&mut self, slot: usize, mut patch: Patch) {
        patch.internal_num = user_to_internal(slot as u8).expect("slot out of range");
        self.slots[slot] = Some(patch);
    }

    /// Swap two slots and renumber both (Java `SharedData.swapPatches`).
    pub fn swap(&mut self, a: usize, b: usize) {
        self.slots.swap(a, b);
        for slot in [a, b] {
            if let Some(p) = &mut self.slots[slot] {
                p.internal_num = user_to_internal(slot as u8).unwrap();
            }
        }
    }

    /// Empty a slot, returning the patch that was there.
    pub fn clear(&mut self, slot: usize) -> Option<Patch> {
        self.slots.get_mut(slot)?.take()
    }

    /// Display name for a slot: the patch name, or [`EMPTY_NAME`].
    pub fn name(&self, slot: usize) -> String {
        self.get(slot)
            .map_or_else(|| EMPTY_NAME.to_string(), Patch::name)
    }

    /// The stored patches in slot order, skipping empty slots.
    pub fn patches(&self) -> Vec<Patch> {
        self.slots.iter().flatten().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
