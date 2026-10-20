//! The machine's wall clock

use std::time::{SystemTime, UNIX_EPOCH};

use super::{Clock, Timestamp};

/// The machine's wall clock
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Timestamp(since_epoch.as_secs())
    }
}
