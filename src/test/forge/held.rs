//! A forge call the test holds open

use std::sync::mpsc::{Receiver, Sender};
use std::sync::{PoisonError, mpsc};
use std::time::Duration;

use super::FakeForge;

// Who reached the forge and who lets it go
#[derive(Debug)]
pub(super) struct Gate {
    entered: Sender<()>,
    release: Receiver<()>,
}

/// A forge that holds its next call open until this is dropped
///
/// A pass asks the forge with the runner's lock held, and each call waits its
/// turn behind the login's pace, so a call held here is a pass that holds the
/// lock. A test takes one to ask a trigger of a runner in the middle of a pass.
#[derive(Debug)]
pub(crate) struct HeldCall {
    entered: Receiver<()>,
    // Dropping it is what lets the call through
    _release: Sender<()>,
}

impl HeldCall {
    /// Whether a call reached the forge and waits here, within `within`
    pub(crate) fn waiting(&self, within: Duration) -> bool {
        self.entered.recv_timeout(within).is_ok()
    }
}

impl FakeForge {
    /// Holds the forge's next call open, so a test can ask a trigger of a
    /// runner whose pass is in the middle of it
    pub(crate) fn hold_next_call(&self) -> HeldCall {
        let (entered, heard) = mpsc::channel();
        let (release, waited) = mpsc::channel();
        *self.hold.lock().unwrap_or_else(PoisonError::into_inner) = Some(Gate {
            entered,
            release: waited,
        });
        HeldCall {
            entered: heard,
            _release: release,
        }
    }

    // Holds one call here until the test drops the [`HeldCall`] it took.
    pub(super) fn wait_if_held(&self) {
        let gate = self
            .hold
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(gate) = gate {
            let _ = gate.entered.send(());
            let _ = gate.release.recv();
        }
    }
}
