//! Reconnect pacing for the gateway.
//!
//! Two things in a reconnect loop get accounts into trouble, and neither is the
//! websocket itself:
//!
//! * **Identify storms.** Every fresh handshake costs an IDENTIFY. Discord's
//!   documented ceiling is 1000 a day, but a client that keeps reconnecting —
//!   a revoked token, a captive portal, a bad clock — can spend that in minutes
//!   and then be locked out of the gateway entirely. This module spends at most
//!   `IDENTIFY_LIMIT` an hour, which is Yapfast's published choice too.
//! * **Synchronised retries.** A raw `2, 4, 8…` ladder means every client on the
//!   same expired token retries on the same second. Each delay is therefore
//!   scaled by a random factor in `[0.5, 1.0)`, so a fleet spreads out.
//!
//! A connection that stays up for `STABLE_SESSION_SECS` resets the ladder; one
//! that authenticates and immediately dies does not, or a crash loop would sit
//! at two seconds forever.
//!
//! Nothing here talks to the network, and the clock is always passed in, so all
//! of it is unit-tested.

use std::collections::VecDeque;
use std::sync::OnceLock;

/// How many fresh identifies are allowed inside the window.
pub const IDENTIFY_LIMIT: u32 = 10;
/// The window those identifies are counted over.
pub const IDENTIFY_WINDOW_SECS: u64 = 3600;
/// How long a connection must stay up before the backoff ladder resets.
pub const STABLE_SESSION_SECS: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentifyAllow {
    Allowed,
    /// Wait this long before trying again; the budget refills on its own.
    Wait {
        retry_after: u64,
    },
}

/// A sliding window over the times an IDENTIFY was spent.
#[derive(Debug, Default, Clone)]
pub struct IdentifyBudget {
    stamps: VecDeque<f64>,
}

impl IdentifyBudget {
    pub fn new() -> Self {
        Self::default()
    }

    /// Spend one identify. Returns `Wait` when the window is full — the caller
    /// must not send anything in that case.
    pub fn try_take(&mut self, now: f64) -> IdentifyAllow {
        self.prune(now);

        if self.stamps.len() < IDENTIFY_LIMIT as usize {
            self.stamps.push_back(now);
            return IdentifyAllow::Allowed;
        }

        let oldest = self.stamps.front().copied().unwrap_or(now);
        let free_at = oldest + IDENTIFY_WINDOW_SECS as f64;
        IdentifyAllow::Wait {
            retry_after: (free_at - now).ceil().max(1.0) as u64,
        }
    }

    /// How many identifies are spent inside the window, as of `now`.
    pub fn used(&mut self, now: f64) -> usize {
        self.prune(now);
        self.stamps.len()
    }

    pub fn reset(&mut self) {
        self.stamps.clear();
    }

    fn prune(&mut self, now: f64) {
        while let Some(front) = self.stamps.front().copied() {
            // A clock that moved backwards keeps entries; that is the safe side.
            if now - front >= IDENTIFY_WINDOW_SECS as f64 {
                self.stamps.pop_front();
            } else {
                break;
            }
        }
    }
}

/// Apply the jitter factor to one backoff step.
///
/// `roll` is a unit random value; anything outside `[0, 1]` is clamped so a
/// broken RNG cannot produce a zero or an inflated delay.
pub fn jittered_secs(base: u64, roll: f64) -> u64 {
    let factor = 0.5 + 0.5 * roll.clamp(0.0, 1.0);
    ((base as f64) * factor).round().max(1.0) as u64
}

/// Whether a session that has been up for this long should reset the ladder.
pub fn should_reset_backoff(healthy_for_secs: f64) -> bool {
    healthy_for_secs >= STABLE_SESSION_SECS as f64
}

/// Seconds since the process started, so the budget never reads the wall clock.
pub fn monotonic_secs() -> f64 {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_secs_f64()
}

/// One random value in `[0, 1)`; a failed RNG lands mid-range, not at zero.
pub fn random_unit() -> f64 {
    let mut bytes = [0u8; 8];
    if getrandom::fill(&mut bytes).is_err() {
        return 0.5;
    }
    let value = u64::from_le_bytes(bytes);
    value as f64 / (u64::MAX as f64 + 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_identifies_are_allowed_and_the_eleventh_waits() {
        let mut budget = IdentifyBudget::new();

        for index in 0..IDENTIFY_LIMIT {
            assert_eq!(
                budget.try_take(index as f64),
                IdentifyAllow::Allowed,
                "identify {index} should be allowed"
            );
        }

        // The stamps were spent at 0..=9, so asking at t=10 finds the oldest one
        // still inside the window, with 3590 s left on it.
        match budget.try_take(IDENTIFY_LIMIT as f64) {
            IdentifyAllow::Wait { retry_after } => {
                assert_eq!(retry_after, IDENTIFY_WINDOW_SECS - IDENTIFY_LIMIT as u64);
            }
            IdentifyAllow::Allowed => panic!("the eleventh identify must wait"),
        }
    }

    #[test]
    fn the_wait_counts_from_the_oldest_stamp() {
        let mut budget = IdentifyBudget::new();
        for _ in 0..IDENTIFY_LIMIT {
            budget.try_take(0.0);
        }

        match budget.try_take(1_000.0) {
            IdentifyAllow::Wait { retry_after } => {
                assert_eq!(retry_after, IDENTIFY_WINDOW_SECS - 1_000)
            }
            IdentifyAllow::Allowed => panic!("budget is full"),
        }
    }

    #[test]
    fn a_stamp_leaving_the_window_frees_exactly_one_identify() {
        let mut budget = IdentifyBudget::new();
        // One per second, so the window holds distinct ages.
        for index in 0..IDENTIFY_LIMIT {
            budget.try_take(index as f64);
        }

        // At t=3600 the stamp taken at 0 is exactly one window old and drops out,
        // freeing one slot; the other nine stay inside the window.
        let window = IDENTIFY_WINDOW_SECS as f64;
        assert_eq!(budget.try_take(window), IdentifyAllow::Allowed);
        assert_eq!(budget.used(window), IDENTIFY_LIMIT as usize);
    }

    #[test]
    fn used_tracks_only_the_entries_inside_the_window() {
        let mut budget = IdentifyBudget::new();
        for _ in 0..4 {
            budget.try_take(0.0);
        }
        assert_eq!(budget.used(10.0), 4);

        // Well past the window, everything has expired.
        assert_eq!(budget.used(IDENTIFY_WINDOW_SECS as f64 + 1.0), 0);
    }

    #[test]
    fn reset_forgets_every_spent_identify() {
        let mut budget = IdentifyBudget::new();
        for _ in 0..IDENTIFY_LIMIT {
            budget.try_take(0.0);
        }
        budget.reset();
        assert_eq!(budget.try_take(1.0), IdentifyAllow::Allowed);
        assert_eq!(budget.used(1.0), 1);
    }

    #[test]
    fn jitter_stays_between_half_and_all_of_the_step() {
        assert_eq!(jittered_secs(64, 0.0), 32);
        assert_eq!(jittered_secs(64, 0.5), 48);
        assert_eq!(jittered_secs(64, 1.0), 64);

        // Out-of-range rolls are clamped, not obeyed.
        assert_eq!(jittered_secs(64, 2.0), 64);
        assert_eq!(jittered_secs(64, -1.0), 32);
    }

    #[test]
    fn jitter_never_produces_a_zero_delay() {
        assert_eq!(jittered_secs(1, 0.0), 1);
        assert_eq!(jittered_secs(0, 0.0), 1);
    }

    #[test]
    fn only_a_session_that_stayed_up_resets_the_ladder() {
        assert!(!should_reset_backoff(2.0));
        assert!(!should_reset_backoff(STABLE_SESSION_SECS as f64 - 0.1));
        assert!(should_reset_backoff(STABLE_SESSION_SECS as f64));
        assert!(should_reset_backoff(600.0));
    }

    #[test]
    fn random_unit_stays_in_range() {
        for _ in 0..64 {
            let roll = random_unit();
            assert!((0.0..1.0).contains(&roll), "roll {roll} out of range");
        }
    }
}
