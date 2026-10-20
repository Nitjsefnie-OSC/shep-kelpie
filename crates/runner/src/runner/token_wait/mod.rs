//! A worker's turn waiting for a token GitHub is not giving just now
//!
//! A covered repo's worker never runs as the maintainer, so a mint that fails
//! for GitHub being out of reach, busy or rate limited holds its turn back
//! instead of parking the work item on a ruling. The turn is tried again
//! after a pause, so a down GitHub is not asked at every pass.

use std::cell::Cell;

use super::Runner;
use crate::ports::{ForgeError, Timestamp};

/// Seconds the turn waits before the token is asked for again
const RETRY_AFTER: u64 = 30;

/// Whether a turn waits for its token, in memory only
#[derive(Debug, Default)]
pub(super) struct TokenWait {
    // The last try failed for a reason that passes
    busy: Cell<bool>,
    // Until when no try is made
    until: Cell<u64>,
    // Whether the wait has been told in the log
    told: Cell<bool>,
}

impl TokenWait {
    /// Whether no try is made yet
    pub(super) fn holds(&self, now: Timestamp) -> bool {
        now.0 < self.until.get()
    }

    /// A try begins: it has not failed busy yet
    pub(super) fn begins(&self) {
        self.busy.set(false);
    }

    /// The try failed for a reason that passes, and no try is made before
    /// `hold` ends, if the forge is held
    pub(super) fn busy_at(&self, now: Timestamp, hold: Option<Timestamp>) {
        self.busy.set(true);
        let after = Timestamp(now.0.saturating_add(RETRY_AFTER));
        self.until.set(hold.map_or(after, |hold| hold.max(after)).0);
    }

    /// Whether the try that just failed did so for a reason that passes
    pub(super) fn is_busy(&self) -> bool {
        self.busy.get()
    }

    /// Whether the wait is new, so the log should say it
    pub(super) fn first_told(&self) -> bool {
        !self.told.replace(true)
    }

    /// The try worked
    pub(super) fn ends(&self) {
        self.told.set(false);
    }
}

impl Runner {
    /// Whether the token's `error` is one that passes, as a busy GitHub or a
    /// held or rate-limited forge is, and the wait for it begins if so
    ///
    /// A rate-limited answer set the forge's hold, which the wait follows.
    pub(super) fn token_error_passes(&self, error: &ForgeError) -> bool {
        let passes = matches!(
            error,
            ForgeError::AppBusy(_) | ForgeError::Limited { .. } | ForgeError::Held(_)
        );
        if passes {
            let now = self.ports.clock.now();
            self.token_wait.busy_at(now, self.forge_hold.until(now));
        }
        passes
    }
}

#[cfg(test)]
mod tests;
