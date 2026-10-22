//! Finishing a call by pid, which only a process that execs has

use std::io;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Instant;

use shep_handover::ChildExit;

use super::super::record::{Record, Recorded};
use super::super::{END_GRACE, POLL, Processes, RunError, Scrap};
use super::Finishing;
use crate::ports::{Adopted, Ending};
use crate::tree::{Signal, Tree};

impl Processes {
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
                Tree::from_group(adopted.pid).send(Signal::Kill);
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
    let tree = Tree::from_group(pgid);
    tree.send(Signal::Terminate);
    let deadline = Instant::now() + END_GRACE;
    while Instant::now() < deadline {
        if !matches!(exited.recv_timeout(POLL), Err(RecvTimeoutError::Timeout)) {
            return;
        }
    }
    tree.send(Signal::Kill);
    let _ = exited.recv_timeout(END_GRACE);
}

/// Ends the process groups `pgids` lead: TERM, then KILL for any left once
/// the grace has passed
///
/// For a runner that took over turns from a previous image and cannot
/// serve them, so none is left running with no one to finish it.
pub fn end_groups(pgids: &[u32]) {
    let trees: Vec<Tree> = pgids.iter().map(|&pgid| Tree::from_group(pgid)).collect();
    trees.iter().for_each(|tree| {
        tree.send(Signal::Terminate);
    });
    let deadline = Instant::now() + END_GRACE;
    while Instant::now() < deadline && trees.iter().any(Tree::running) {
        thread::sleep(POLL);
    }
    trees.iter().for_each(|tree| {
        tree.send(Signal::Kill);
    });
}

fn status_of(exit: ChildExit) -> ExitStatus {
    match (exit.code, exit.signal) {
        (Some(code), _) => ExitStatus::from_raw((code & 0xff) << 8),
        (None, Some(signal)) => ExitStatus::from_raw(signal),
        (None, None) => ExitStatus::from_raw(1 << 8),
    }
}
