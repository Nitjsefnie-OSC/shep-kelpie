//! A call another image of the runner started, finished by this one
//!
//! A handover is an exec, which Windows has none of: there a call is never
//! adopted.
//!
//! A handover replaces the runner's image and not its process, so the
//! `claude` it started is still its child. It cannot be a [`Child`](std::process::Child)
//! here, so it is waited on by pid, and its record says where its output went.

use super::Processes;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::end_groups;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::end_groups;

// Counts a call as being finished while it lives, from its child's reaping
// to its output read and its record removed
pub(super) struct Finishing {
    processes: Processes,
    pub(super) counted: bool,
}

impl Finishing {
    pub(super) fn new(processes: &Processes) -> Self {
        Self {
            processes: processes.clone(),
            counted: false,
        }
    }
}

impl Finishing {
    // One that counts from now: an adopted call is waited on from a thread
    // that reaps it when it likes, so a seal never finds it half done
    #[cfg(unix)]
    fn counting(processes: &Processes) -> Self {
        processes.lock().finishing += 1;
        Self {
            processes: processes.clone(),
            counted: true,
        }
    }
}

impl Drop for Finishing {
    fn drop(&mut self) {
        if self.counted {
            self.processes.lock().finishing -= 1;
        }
    }
}

/// No child is reaped while it lives
///
/// A child that exits meanwhile stays a zombie, for the image a handover
/// execs to reap and read.
#[derive(Debug)]
pub struct Sealed(Processes);

impl Drop for Sealed {
    fn drop(&mut self) {
        self.0.lock().sealed = false;
    }
}

impl Processes {
    /// Stops reaping, unless a call has ended and is still being read, whose
    /// record would be gone by the time a successor looked for it
    pub(in crate::adapters) fn seal(&self) -> Option<Sealed> {
        let mut running = self.lock();
        if running.sealed || running.finishing > 0 {
            return None;
        }
        running.sealed = true;
        Some(Sealed(self.clone()))
    }
}

#[cfg(all(test, unix))]
mod tests;
