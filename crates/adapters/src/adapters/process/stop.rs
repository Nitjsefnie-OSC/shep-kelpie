//! A child and the tree it leads, ended together

use std::io;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

use super::POLL;
use crate::tree::{Lead, Signal, Tree};

/// A child started as the leader of a tree
///
/// A program it starts and leaves behind (a build, a test run) is reached
/// through the tree, not only through the one pid `child` names.
#[derive(Debug)]
pub(in crate::adapters) struct Kept {
    pub(in crate::adapters) child: Child,
    // None when the system would not make one, which leaves the child alone
    tree: Option<Tree>,
}

impl Kept {
    pub(in crate::adapters) fn spawn(command: &mut Command) -> io::Result<Self> {
        let child = command.lead().spawn()?;
        let tree = Tree::of(&child).ok();
        Ok(Self { child, tree })
    }

    pub(super) fn send(&mut self, signal: Signal) {
        match &self.tree {
            Some(tree) => {
                tree.send(signal);
            }
            None if matches!(signal, Signal::Terminate | Signal::Kill) => {
                let _ = self.child.kill();
            }
            None => {}
        }
    }

    // Whether the leader is reaped and nothing it started runs. A zombie in
    // the tree counts as running, so the leader is reaped on the way.
    pub(super) fn ended(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
            && !self.tree.as_ref().is_some_and(Tree::running)
    }

    // The whole tree killed, and the leader reaped.
    pub(super) fn kill(&mut self) {
        self.send(Signal::Kill);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// The tree asked to end, then killed once `grace` has passed with any of it
// still running. A leader that exits on the request can leave a member that
// ignores it, so the wait is for the tree.
pub(in crate::adapters) fn stop_child(kept: &mut Kept, grace: Duration) {
    kept.send(Signal::Terminate);
    let deadline = Instant::now() + grace;
    while Instant::now() < deadline {
        if kept.ended() {
            return;
        }
        thread::sleep(POLL);
    }
    kept.kill();
}
