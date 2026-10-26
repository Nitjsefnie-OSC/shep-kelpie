//! How long a runner's loop sleeps

use std::time::Duration;

use crate::ports::Timestamp;
use crate::runner::{BOARD_POLL, READ_EVERY, Runner};

// How long the loop sleeps after a pass with nothing left to do, unless woken
pub(super) fn wait_after_pass(runner: &Runner) -> Duration {
    let look = if runner.awaits_reply() {
        Duration::from_secs(READ_EVERY)
    } else {
        BOARD_POLL
    };
    let soonest = [runner.next_ceiling(), runner.next_look()];
    (soonest.into_iter().flatten()).fold(look, Duration::min)
}

/// How long a runner that is never idle goes between rereads of its settings
pub(super) const INTERVAL: Duration = Duration::from_secs(30);

/// When the loop last read the settings again, by the runner's clock
pub(super) struct Reread(Timestamp);

impl Reread {
    pub(super) fn at(runner: &Runner) -> Self {
        Self(runner.now())
    }

    /// Whether `INTERVAL` has passed since the last read
    pub(super) fn due(&self, runner: &Runner) -> bool {
        runner.now().0.saturating_sub(self.0.0) >= INTERVAL.as_secs()
    }

    pub(super) fn done(&mut self, runner: &Runner) {
        self.0 = runner.now();
    }
}
