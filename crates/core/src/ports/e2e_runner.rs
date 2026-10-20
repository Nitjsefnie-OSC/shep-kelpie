//! The e2e runner port: a project's end-to-end tests, run on the dog's host
//!
//! A bump's head is checked out in a worktree under the project's
//! `worktrees` folder, and the dog works out the project from where that
//! folder is. The host, the command and the results folder are the
//! project's settings, never the ask's.

use std::path::PathBuf;

/// One e2e run to ask the dog for
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct E2eRun {
    /// The worktree to run the tests from: a folder under the project's `worktrees`
    pub worktree: PathBuf,
    /// The dog's door, where the run takes its place in the host's queue
    pub door: PathBuf,
}

/// How an e2e run ended
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum E2eOutcome {
    /// The command exited 0
    Passed,
    /// It exited with anything else, or ran past the dog's ceiling
    Failed {
        /// The last [`OUTPUT_KEPT`](super::OUTPUT_KEPT) bytes of what it wrote
        output: String,
    },
    /// The dog says the project has no e2e host
    NoHost,
    /// It never ran to a result: the dog was not reached, or went away
    NotRun {
        /// Why
        reason: String,
    },
}

/// Runs a project's e2e tests
pub trait E2eRunner: Send + Sync {
    /// Runs `run` to its end, however long the host's queue and the run take
    ///
    /// Never fails: a run that could not happen is [`E2eOutcome::NotRun`].
    fn run(&self, run: &E2eRun) -> E2eOutcome;
}
