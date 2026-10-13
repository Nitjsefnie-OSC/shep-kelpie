//! A call another image of the runner started, finished by this one
//!
//! A handover replaces the runner's image and not its process, so the
//! `claude` it started is still its child. It cannot be a [`Child`](std::process::Child)
//! here, so it is waited on by pid, and its record says where its output went.

use std::io;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Instant;

use shep_handover::ChildExit;

use super::record::{Record, Recorded};
use super::{END_GRACE, POLL, Processes, RunError, Scrap, group_running, signal_group};
use crate::ports::{Adopted, Ending};

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

    /// Waits for `adopted`'s process to end and reads its output, as
    /// `output_of_call` does for one it started
    ///
    /// A call asked to end, by `ending` or by the record it was handed over
    /// with, is ended with its group and comes back as [`RunError::TimedOut`].
    /// A status the kernel no longer has is an [`io::ErrorKind::NotFound`].
    pub(in crate::adapters) fn output_of_adopted(
        &self,
        adopted: &Adopted,
        ending: &Ending,
    ) -> Result<Output, RunError> {
        let _finishing = Finishing::counting(self);
        let record = Record::read(&adopted.record).map_err(RunError::Io)?;
        let (stdout, stderr) = (record.stdout.clone(), record.stderr.clone());
        let _scrap = Scrap([&stdout, &stderr]);
        let asked = record.ending;
        let mut recorded = Recorded::adopted(&adopted.record, record);

        let exited = reap(adopted.pid);
        {
            let mut running = self.lock();
            if running.stopping {
                signal_group(adopted.pid, "KILL");
                return Err(RunError::Stopped);
            }
            running.adopted.push(adopted.pid);
        }
        let status = self.await_exit(adopted.pid, &exited, ending, asked, &mut recorded);
        self.lock().adopted.retain(|pgid| *pgid != adopted.pid);
        let status = status?;
        if self.stopping() {
            return Err(RunError::Stopped);
        }
        Ok(Output {
            status,
            stdout: std::fs::read(&stdout).map_err(RunError::Io)?,
            stderr: std::fs::read(&stderr).map_err(RunError::Io)?,
        })
    }

    fn await_exit(
        &self,
        pid: u32,
        exited: &Receiver<io::Result<ChildExit>>,
        ending: &Ending,
        asked: bool,
        recorded: &mut Recorded,
    ) -> Result<ExitStatus, RunError> {
        loop {
            match exited.recv_timeout(POLL) {
                Ok(exit) => return exit.map(status_of).map_err(RunError::Io),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(RunError::Io(io::Error::other(
                        "the wait for the call ended",
                    )));
                }
            }
            if self.stopping() {
                return Err(RunError::Stopped);
            }
            if asked || ending.asked() {
                recorded.ending();
                end_group(pid, exited);
                return Err(RunError::TimedOut);
            }
        }
    }
}

// Waits for `pid` on a thread of its own, with a runtime only for that
fn reap(pid: u32) -> Receiver<io::Result<ChildExit>> {
    let (send, exited) = mpsc::channel();
    thread::spawn(move || {
        let waited = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .and_then(|runtime| runtime.block_on(shep_handover::wait_for_child(pid)));
        let _ = send.send(waited);
    });
    exited
}

// SIGTERM to the group, then SIGKILL once the grace has passed with the
// leader still unreaped
fn end_group(pgid: u32, exited: &Receiver<io::Result<ChildExit>>) {
    signal_group(pgid, "TERM");
    let deadline = Instant::now() + END_GRACE;
    while Instant::now() < deadline {
        if !matches!(exited.recv_timeout(POLL), Err(RecvTimeoutError::Timeout)) {
            return;
        }
    }
    signal_group(pgid, "KILL");
    let _ = exited.recv_timeout(END_GRACE);
}

/// Ends the process groups `pgids` lead: TERM, then KILL for any left once
/// the grace has passed
///
/// For a runner that took over turns from a previous image and cannot
/// serve them, so none is left running with no one to finish it.
pub fn end_groups(pgids: &[u32]) {
    pgids.iter().for_each(|&pgid| signal_group(pgid, "TERM"));
    let deadline = Instant::now() + END_GRACE;
    while Instant::now() < deadline && pgids.iter().any(|&pgid| group_running(pgid)) {
        thread::sleep(POLL);
    }
    pgids.iter().for_each(|&pgid| signal_group(pgid, "KILL"));
}

fn status_of(exit: ChildExit) -> ExitStatus {
    match (exit.code, exit.signal) {
        (Some(code), _) => ExitStatus::from_raw((code & 0xff) << 8),
        (None, Some(signal)) => ExitStatus::from_raw(signal),
        (None, None) => ExitStatus::from_raw(1 << 8),
    }
}

#[cfg(test)]
mod tests;
