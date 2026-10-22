//! A process and everything it started, stopped as one
//!
//! On Unix a tree is the process group the leader leads, found by its pid. On
//! Windows it is a job object the leader is assigned to as it starts, so a
//! process it starts is in the job too. A job ends everything in it when its
//! handle closes, which dropping a [`Tree`] does; a group outlives its handle.

use std::io;
use std::process::Command;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as imp;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;
#[cfg(not(any(unix, windows)))]
compile_error!("kelpie stops a process tree on Unix and Windows only");

/// What to tell a tree
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// Ctrl-C. A console delivers it to its processes itself, so on Windows
    /// sending it does nothing.
    Interrupt,
    /// A closed terminal. On Windows it ends the tree, as there is no hangup.
    Hangup,
    /// A request to end. On Windows it ends the tree, as there is no request.
    Terminate,
    /// An end the tree cannot refuse
    Kill,
}

/// How much of a tree a [`Tree::send`] reached
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// The whole tree, or it was already gone
    Whole,
    /// The leader alone on Unix, after a group signal was refused, as a
    /// worker's sandbox may; nothing on Windows, when the job refused to end
    Partial,
}

/// Makes a command's process the leader of a tree of its own
///
/// Call it before the command starts, then [`Tree::of`] once it has.
pub trait Lead {
    /// Gives the command its own process group on Unix, so a signal to
    /// kelpie's group, a terminal's Ctrl-C, does not reach it. On Windows it
    /// changes nothing: the job is made as the process starts.
    fn lead(&mut self) -> &mut Self;
}

impl Lead for Command {
    fn lead(&mut self) -> &mut Self {
        imp::lead(self)
    }
}

impl Lead for tokio::process::Command {
    fn lead(&mut self) -> &mut Self {
        imp::lead_tokio(self)
    }
}

/// A process and everything it started
///
/// Dropping one ends the tree on Windows, and does nothing on Unix.
#[derive(Debug)]
pub struct Tree(imp::Tree);

impl Tree {
    /// The tree `child` leads, which it started leading by [`Lead::lead`]
    ///
    /// # Errors
    ///
    /// On Windows, the job could not be made, or the process could not be
    /// put in it, as when the process already ended or kelpie runs in a job
    /// that does not allow another.
    pub fn of(child: &std::process::Child) -> io::Result<Self> {
        imp::Tree::of(child).map(Self)
    }

    /// [`Self::of`], for a child from tokio
    ///
    /// # Errors
    ///
    /// As [`Self::of`], and also when the child has been waited on.
    pub fn of_tokio(child: &tokio::process::Child) -> io::Result<Self> {
        imp::Tree::of_tokio(child).map(Self)
    }

    /// The tree a process group holds, led by the process `pgid`
    ///
    /// A group another image of the runner started is reached this way. On
    /// Windows, where a handover is not made, it holds nothing.
    #[must_use]
    pub fn from_group(pgid: u32) -> Self {
        Self(imp::Tree::from_group(pgid))
    }

    /// Reaches the process alone on Unix, for one that shares kelpie's group
    ///
    /// A signal to its group would reach kelpie too. On Windows it changes
    /// nothing: the job holds only what the process started.
    #[must_use]
    pub fn alone(self) -> Self {
        Self(self.0.alone())
    }

    /// Tells the tree `signal`
    pub fn send(&self, signal: Signal) -> Reach {
        self.0.send(signal)
    }

    /// Whether anything in the tree is still running
    ///
    /// A leader that ended but was not waited on still counts on Unix.
    #[must_use]
    pub fn running(&self) -> bool {
        self.0.running()
    }
}

/// Whether a process with this pid exists and is the caller's to signal
///
/// One owned by another user counts as gone, as `kill -0` always judged it.
#[must_use]
pub fn alive(pid: u32) -> bool {
    imp::alive(pid)
}

#[cfg(test)]
mod tests;
