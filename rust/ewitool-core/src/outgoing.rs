//! Messages sent to the EWI and the paced queue they go through (port of
//! `MidiHandler.sendLiveControl` / `sendPatch` and `MidiSender.java`).
//!
//! Timing follows the Java version: no pause after controller messages,
//! 250 ms after each patch so the EWI has time to store it.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::patch::{Mode, Patch};

/// Pause after sending a patch (Java `MIDI_MESSAGE_LONG_PAUSE_MS`).
pub const PATCH_PAUSE: Duration = Duration::from_millis(250);

const CONTROL_CHANGE: u8 = 0xB0; // channel 1
const CC_DATA_ENTRY: u8 = 0x06;
const CC_NRPN_LSB: u8 = 0x62;
const CC_NRPN_MSB: u8 = 0x63;
const NRPN_NULL: u8 = 0x7F;

/// The controller messages that set one patch parameter live: select the
/// NRPN, send the value, then deselect so stray data entry does nothing.
pub fn live_control(nrpn: u8, index: u8, value: u8) -> Vec<Vec<u8>> {
    [
        (CC_NRPN_LSB, index),
        (CC_NRPN_MSB, nrpn),
        (CC_DATA_ENTRY, value),
        (CC_NRPN_LSB, NRPN_NULL),
        (CC_NRPN_MSB, NRPN_NULL),
    ]
    .into_iter()
    .map(|(cc, v)| vec![CONTROL_CHANGE, cc, v & 0x7F])
    .collect()
}

/// The SysEx messages that send a patch. In `Save` mode the EWI stores it
/// in its slot. In `Edit` mode it is sent a second time numbered as patch 0,
/// which is what loads it into the EWI's edit buffer (as the Java version
/// does).
pub fn patch_messages(patch: &Patch, mode: Mode) -> Vec<Vec<u8>> {
    let mut p = patch.clone();
    p.set_mode(mode);
    let mut msgs = vec![p.to_bytes().to_vec()];
    if mode == Mode::Edit {
        p.internal_num = 0;
        msgs.push(p.to_bytes().to_vec());
    }
    msgs
}

#[derive(Debug)]
struct Item {
    msgs: Vec<Vec<u8>>,
    pause: Duration,
    /// (nrpn, index) for live controls, so repeated changes can be merged.
    live: Option<(u8, u8)>,
}

/// Messages waiting to go to the EWI, sent in order with the right pauses.
#[derive(Debug, Default)]
pub struct SendQueue {
    items: VecDeque<Item>,
    ready_at: Option<Instant>,
}

impl SendQueue {
    /// Queue a parameter change. If a change to the same parameter is still
    /// waiting, its value is updated instead, so dragging a slider while a
    /// patch is being sent can't build up a backlog.
    pub fn push_live(&mut self, nrpn: u8, index: u8, value: u8) {
        let msgs = live_control(nrpn, index, value);
        if let Some(item) = self
            .items
            .iter_mut()
            .find(|i| i.live == Some((nrpn, index)))
        {
            item.msgs = msgs;
        } else {
            self.items.push_back(Item {
                msgs,
                pause: Duration::ZERO,
                live: Some((nrpn, index)),
            });
        }
    }

    pub fn push_patch(&mut self, patch: &Patch, mode: Mode) {
        for msg in patch_messages(patch, mode) {
            self.items.push_back(Item {
                msgs: vec![msg],
                pause: PATCH_PAUSE,
                live: None,
            });
        }
    }

    /// The next messages to send, if their turn has come at `now`.
    pub fn next(&mut self, now: Instant) -> Option<Vec<Vec<u8>>> {
        if self.ready_at.is_some_and(|t| now < t) {
            return None;
        }
        let item = self.items.pop_front()?;
        self.ready_at = Some(now + item.pause);
        Some(item.msgs)
    }

    /// When `next` will next have something to send, if anything is queued.
    pub fn wake_at(&self) -> Option<Instant> {
        if self.items.is_empty() {
            None
        } else {
            Some(self.ready_at.unwrap_or_else(Instant::now))
        }
    }

    /// Patch messages still waiting.
    pub fn pending_patches(&self) -> usize {
        self.items.iter().filter(|i| i.live.is_none()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::PATCH_LEN;

    fn patch(internal: u8) -> Patch {
        let mut blob = [0u8; PATCH_LEN];
        blob[..6].copy_from_slice(&[0xF0, 0x47, 0x64, 0x7F, 0x00, internal]);
        blob[PATCH_LEN - 1] = 0xF7;
        Patch::decode(&blob)
    }

    #[test]
    fn live_control_bytes() {
        assert_eq!(
            live_control(64, 17, 100),
            vec![
                vec![0xB0, 0x62, 17],
                vec![0xB0, 0x63, 64],
                vec![0xB0, 0x06, 100],
                vec![0xB0, 0x62, 0x7F],
                vec![0xB0, 0x63, 0x7F],
            ]
        );
    }

    #[test]
    fn save_sends_once_with_save_flag() {
        let msgs = patch_messages(&patch(12), Mode::Save);
        assert_eq!(msgs.len(), 1);
        assert_eq!((msgs[0][4], msgs[0][5]), (0x00, 12));
        assert_eq!(msgs[0].len(), PATCH_LEN);
    }

    #[test]
    fn edit_sends_twice_second_as_patch_zero() {
        let mut p = patch(12);
        p.mode = 0x00;
        let msgs = patch_messages(&p, Mode::Edit);
        assert_eq!(msgs.len(), 2);
        assert_eq!((msgs[0][4], msgs[0][5]), (0x20, 12));
        assert_eq!((msgs[1][4], msgs[1][5]), (0x20, 0));
        // The caller's patch is untouched.
        assert_eq!((p.mode, p.internal_num), (0x00, 12));
    }

    #[test]
    fn queue_pauses_after_patches_only() {
        let mut q = SendQueue::default();
        let t = Instant::now();
        q.push_live(64, 1, 10);
        q.push_patch(&patch(3), Mode::Save);
        q.push_live(64, 2, 20);
        assert_eq!(q.next(t).unwrap().len(), 5); // live control: no pause
        assert_eq!(q.next(t).unwrap()[0].len(), PATCH_LEN); // patch
        assert!(
            q.next(t + Duration::from_millis(100)).is_none(),
            "must pause after a patch"
        );
        assert_eq!(q.next(t + PATCH_PAUSE).unwrap().len(), 5);
        assert!(q.is_empty());
        assert!(q.next(t + PATCH_PAUSE * 2).is_none());
    }

    #[test]
    fn repeated_live_changes_are_merged() {
        let mut q = SendQueue::default();
        q.push_patch(&patch(3), Mode::Save);
        for v in 0..50 {
            q.push_live(65, 8, v);
        }
        q.push_live(65, 9, 1);
        let t = Instant::now();
        q.next(t); // the patch
        let first = q.next(t + PATCH_PAUSE).unwrap();
        assert_eq!(first[2], vec![0xB0, 0x06, 49], "latest value wins");
        assert_eq!(q.next(t + PATCH_PAUSE).unwrap()[0], vec![0xB0, 0x62, 9]);
        assert!(q.is_empty());
    }

    #[test]
    fn counts_pending_patches() {
        let mut q = SendQueue::default();
        q.push_patch(&patch(1), Mode::Save);
        q.push_patch(&patch(2), Mode::Edit); // two messages
        q.push_live(64, 1, 1);
        assert_eq!(q.pending_patches(), 3);
        q.clear();
        assert!(q.is_empty());
        assert_eq!(q.wake_at(), None);
    }
}
