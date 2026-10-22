use std::io;
use std::os::unix::process::CommandExt;
use std::process::Command;

use rustix::io::Errno;
use rustix::process::{
    Pid, Signal as Sig, kill_process, kill_process_group, test_kill_process,
    test_kill_process_group,
};

use super::{Reach, Signal};

pub(super) fn lead(command: &mut Command) -> &mut Command {
    command.process_group(0)
}

pub(super) fn lead_tokio(command: &mut tokio::process::Command) -> &mut tokio::process::Command {
    command.process_group(0)
}

// The group a leader leads, which is its pid, or the process alone. None when
// the pid is not one a signal can name, which reaches nothing.
#[derive(Debug)]
pub(super) struct Tree {
    pid: Option<Pid>,
    alone: bool,
}

impl Tree {
    #[allow(clippy::unnecessary_wraps)]
    pub(super) fn of(child: &std::process::Child) -> io::Result<Self> {
        Ok(Self::from_group(child.id()))
    }

    pub(super) fn of_tokio(child: &tokio::process::Child) -> io::Result<Self> {
        child
            .id()
            .map(Self::from_group)
            .ok_or_else(|| io::Error::other("the child has been waited on"))
    }

    pub(super) fn from_group(pgid: u32) -> Self {
        Self {
            pid: pid_of(pgid),
            alone: false,
        }
    }

    pub(super) fn alone(self) -> Self {
        Self {
            alone: true,
            ..self
        }
    }

    // The leader alone when the group signal is refused, as a worker's sandbox
    // may. A group that is already gone is what was wanted.
    pub(super) fn send(&self, signal: Signal) -> Reach {
        let Some(group) = self.pid else {
            return Reach::Partial;
        };
        let signal = match signal {
            Signal::Interrupt => Sig::INT,
            Signal::Hangup => Sig::HUP,
            Signal::Terminate => Sig::TERM,
            Signal::Kill => Sig::KILL,
        };
        if self.alone {
            return match kill_process(group, signal) {
                Ok(()) | Err(Errno::SRCH) => Reach::Whole,
                Err(_) => Reach::Partial,
            };
        }
        match kill_process_group(group, signal) {
            Ok(()) | Err(Errno::SRCH) => Reach::Whole,
            Err(_) => {
                let _ = kill_process(group, signal);
                Reach::Partial
            }
        }
    }

    // A zombie in the group counts, so a leader is waited on before asking. A
    // group that exists but is refused a signal still runs.
    pub(super) fn running(&self) -> bool {
        let Some(pid) = self.pid else {
            return false;
        };
        let asked = if self.alone {
            test_kill_process(pid)
        } else {
            test_kill_process_group(pid)
        };
        matches!(asked, Ok(()) | Err(Errno::PERM))
    }
}

pub(super) fn alive(pid: u32) -> bool {
    pid_of(pid).is_some_and(|pid| test_kill_process(pid).is_ok())
}

fn pid_of(pid: u32) -> Option<Pid> {
    Pid::from_raw(i32::try_from(pid).ok()?)
}
