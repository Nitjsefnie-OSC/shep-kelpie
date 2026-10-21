//! Letting go of the calls in flight as the runner stops

use super::{Base, LocalReviewer};

/// Refuses new calls on `base` and `reviewer`, sends each call in flight
/// SIGTERM, and leaves the rest to shep's stop, which ends every lamb once
/// the runner exits
pub fn stop_calls(base: &Base, reviewer: &LocalReviewer) {
    base.stop();
    reviewer.stop();
}
