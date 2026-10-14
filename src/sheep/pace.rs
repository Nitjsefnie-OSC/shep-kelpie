//! How long a runner's loop sleeps

use std::time::Duration;

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
