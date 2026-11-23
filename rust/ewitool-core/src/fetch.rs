//! Fetching all 100 patches from the EWI (port of the "Fetch All Patches"
//! loop in `UiMain.java` and `MidiHandler.requestPatch`).
//!
//! [`Fetcher`] does no I/O itself, so it can be tested without hardware.
//! The caller sends what [`Fetcher::poll`] asks for and passes every
//! received SysEx message to [`Fetcher::receive`].
//!
//! Timing follows the Java version: requests at least 100 ms apart and up
//! to 3 s to wait for each reply. Java retried a patch forever; this gives
//! up after [`MAX_ATTEMPTS`] tries.

use std::time::{Duration, Instant};

use crate::bank::Bank;
use crate::patch::{user_to_internal, NUM_PATCHES};
use crate::sysex;

/// Minimum gap between requests (Java `MIDI_MESSAGE_SHORT_PAUSE_MS`).
pub const MIN_GAP: Duration = Duration::from_millis(100);
/// How long to wait for each patch (Java `MIDI_TIMEOUT_MS`).
pub const TIMEOUT: Duration = Duration::from_secs(3);
/// Requests per patch before giving up.
pub const MAX_ATTEMPTS: u32 = 3;

/// What the caller should do next.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// Send these bytes to the EWI now.
    Send(Vec<u8>),
    /// Nothing to do yet; call `poll` again soon.
    Wait,
    /// All patches received; take them with [`Fetcher::into_bank`].
    Done,
    /// Gave up on a patch.
    Failed(String),
}

#[derive(Debug)]
pub struct Fetcher {
    bank: Bank,
    /// Next slot to fetch (the number the player sees).
    slot: usize,
    first: usize,
    /// One past the last slot to fetch.
    end: usize,
    /// When the current slot's last request was sent.
    sent_at: Option<Instant>,
    /// When any request was last sent, for pacing.
    last_send: Option<Instant>,
    attempts: u32,
}

impl Default for Fetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Fetcher {
    /// Fetch all 100 patches.
    pub fn new() -> Self {
        Self::range(0, NUM_PATCHES)
    }

    /// Fetch just the patch in `slot` (the number the player sees).
    pub fn only(slot: usize) -> Self {
        assert!(slot < NUM_PATCHES, "slot out of range");
        Self::range(slot, slot + 1)
    }

    fn range(first: usize, end: usize) -> Self {
        Self {
            bank: Bank::default(),
            slot: first,
            first,
            end,
            sent_at: None,
            last_send: None,
            attempts: 0,
        }
    }

    /// Patches received so far.
    pub fn received(&self) -> usize {
        self.slot - self.first
    }

    pub fn is_done(&self) -> bool {
        self.slot >= self.end
    }

    /// Handle a received message. Returns true if it was the patch being
    /// waited for; anything else (other replies, stale patches) is ignored.
    pub fn receive(&mut self, msg: &[u8]) -> bool {
        if self.is_done() || self.sent_at.is_none() {
            return false;
        }
        let Some(patch) = sysex::parse_patch_dump(msg) else {
            return false;
        };
        if Some(patch.internal_num) != user_to_internal(self.slot as u8) {
            return false;
        }
        self.bank.set(self.slot, patch);
        self.slot += 1;
        self.sent_at = None;
        self.attempts = 0;
        true
    }

    pub fn poll(&mut self, now: Instant) -> Step {
        if self.is_done() {
            return Step::Done;
        }
        if let Some(sent) = self.sent_at {
            if now.duration_since(sent) < TIMEOUT {
                return Step::Wait;
            }
            if self.attempts >= MAX_ATTEMPTS {
                return Step::Failed(format!(
                    "The EWI didn't send patch #{} after {MAX_ATTEMPTS} tries.",
                    self.slot
                ));
            }
        }
        if self
            .last_send
            .is_some_and(|t| now.duration_since(t) < MIN_GAP)
        {
            return Step::Wait;
        }
        self.sent_at = Some(now);
        self.last_send = Some(now);
        self.attempts += 1;
        let internal = user_to_internal(self.slot as u8).unwrap();
        Step::Send(sysex::patch_request(internal).to_vec())
    }

    /// The fetched patches (complete once `poll` returns `Done`).
    pub fn into_bank(self) -> Bank {
        self.bank
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::PATCH_LEN;

    /// What the EWI sends back for a request: a dump of that patch.
    fn reply_to(request: &[u8]) -> Vec<u8> {
        let internal = request[5];
        let mut dump = [0u8; PATCH_LEN];
        dump[..6].copy_from_slice(&[0xF0, 0x47, 0x64, 0x7F, 0x00, internal]);
        dump[12..20].copy_from_slice(format!("Int {internal:<4}").as_bytes());
        dump[PATCH_LEN - 1] = 0xF7;
        dump.to_vec()
    }

    fn expect_send(step: Step) -> Vec<u8> {
        match step {
            Step::Send(bytes) => bytes,
            other => panic!("expected Send, got {other:?}"),
        }
    }

    #[test]
    fn fetches_all_patches_in_player_order() {
        let mut f = Fetcher::new();
        let mut now = Instant::now();
        let mut requested = Vec::new();
        while !f.is_done() {
            let req = expect_send(f.poll(now));
            requested.push(req[5]);
            assert!(f.receive(&reply_to(&req)));
            now += MIN_GAP;
        }
        assert_eq!(f.poll(now), Step::Done);
        // Player's #0 is internal 99, then internal 0, 1, ...
        assert_eq!(requested[0], 99);
        assert_eq!(requested[1], 0);
        assert_eq!(requested[99], 98);
        let bank = f.into_bank();
        assert_eq!(bank.len(), 100);
        assert_eq!(bank.name(0), "Int 99");
        assert_eq!(bank.name(1), "Int 0");
    }

    #[test]
    fn fetches_one_patch() {
        let mut f = Fetcher::only(5);
        let req = expect_send(f.poll(Instant::now()));
        assert_eq!(req[5], 4);
        assert!(f.receive(&reply_to(&req)));
        assert_eq!(f.poll(Instant::now()), Step::Done);
        let bank = f.into_bank();
        assert_eq!(bank.len(), 1);
        assert_eq!(bank.name(5), "Int 4");
    }

    #[test]
    fn paces_requests() {
        let mut f = Fetcher::new();
        let t0 = Instant::now();
        let req = expect_send(f.poll(t0));
        f.receive(&reply_to(&req));
        // Reply came quickly; the next request must still wait for MIN_GAP.
        assert_eq!(f.poll(t0 + Duration::from_millis(20)), Step::Wait);
        expect_send(f.poll(t0 + MIN_GAP));
    }

    #[test]
    fn ignores_wrong_and_unrelated_messages() {
        let mut f = Fetcher::new();
        let now = Instant::now();
        // Nothing requested yet.
        assert!(!f.receive(&reply_to(&[0, 0, 0, 0, 0, 99])));
        let req = expect_send(f.poll(now));
        assert!(!f.receive(&reply_to(&[0, 0, 0, 0, 0, 5]))); // wrong patch
        assert!(!f.receive(&sysex::DEVICE_ID_REQUEST)); // not a patch
        assert_eq!(f.received(), 0);
        assert!(f.receive(&reply_to(&req)));
        assert_eq!(f.received(), 1);
    }

    #[test]
    fn retries_then_gives_up() {
        let mut f = Fetcher::new();
        let mut now = Instant::now();
        let first = expect_send(f.poll(now));
        assert_eq!(f.poll(now + Duration::from_secs(1)), Step::Wait);
        for _ in 1..MAX_ATTEMPTS {
            now += TIMEOUT;
            assert_eq!(
                expect_send(f.poll(now)),
                first,
                "retry asks for the same patch"
            );
        }
        now += TIMEOUT;
        assert!(matches!(f.poll(now), Step::Failed(msg) if msg.contains("#0")));
    }

    #[test]
    fn retry_succeeds_after_timeout() {
        let mut f = Fetcher::new();
        let now = Instant::now();
        let req = expect_send(f.poll(now));
        let again = expect_send(f.poll(now + TIMEOUT));
        assert_eq!(req, again);
        assert!(f.receive(&reply_to(&again)));
        assert_eq!(f.received(), 1);
    }
}
