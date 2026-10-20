//! A call in flight that a test lets go of when it chooses

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// How often a held call looks whether it was ended, as the real adapters
/// look at their child
const LOOK_EVERY: Duration = Duration::from_millis(5);

/// A call in flight that a test lets go of when it chooses
#[derive(Debug, Clone, Default)]
pub struct Hold(Arc<(Mutex<Held>, Condvar)>);

#[derive(Debug, Default)]
struct Held {
    entered: bool,
    released: bool,
    returned: bool,
}

impl Hold {
    /// Waits up to `within` for the call to begin, and says whether it did
    pub fn entered(&self, within: Duration) -> bool {
        let (held, changed) = &*self.0;
        let held = held.lock().unwrap();
        let (held, _) = changed
            .wait_timeout_while(held, within, |h| !h.entered)
            .unwrap();
        held.entered
    }

    /// Lets the call answer
    pub fn release(&self) {
        let (held, changed) = &*self.0;
        held.lock().unwrap().released = true;
        changed.notify_all();
    }

    /// Whether the call has answered
    pub fn returned(&self) -> bool {
        self.0.0.lock().unwrap().returned
    }

    /// Waits up to `within` for the call to answer, and says whether it did
    pub fn answered(&self, within: Duration) -> bool {
        let (held, changed) = &*self.0;
        let held = held.lock().unwrap();
        let (held, _) = changed
            .wait_timeout_while(held, within, |h| !h.returned)
            .unwrap();
        held.returned
    }

    /// Holds the caller until the hold is let go
    pub fn block(&self) {
        self.block_unless(|| false);
    }

    /// Blocks until the test releases it, or until `ended` says the call
    /// was ended first, and says whether it was released
    pub fn block_unless(&self, ended: impl Fn() -> bool) -> bool {
        let (held, changed) = &*self.0;
        let mut held = held.lock().unwrap();
        held.entered = true;
        changed.notify_all();
        while !held.released && !ended() {
            held = changed.wait_timeout(held, LOOK_EVERY).unwrap().0;
        }
        held.returned = true;
        changed.notify_all();
        held.released
    }
}
