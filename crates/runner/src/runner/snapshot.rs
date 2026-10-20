//! The open pull requests, listed once for all the steps of a pass
//!
//! The board, the rework check, each work item's CI gate and its merge read
//! the same pull requests, so a pass lists them in one query the first time
//! a step asks and every later step in the pass reads that listing. A step
//! reads a single pull request only when the listing does not hold it, such
//! as a merged one, or when it runs outside a pass, such as the end of a
//! worker's turn, which may have just pushed. The merge call itself holds
//! the head it was asked about, so a head that moved since the listing is
//! refused by the forge.

use std::cell::RefCell;
use std::collections::BTreeMap;

use super::Runner;
use crate::board::{OpenPullRequest, ReviewMarks};
use crate::ports::{ForgeError, PullRequest};

#[derive(Debug, Default)]
enum Pass {
    // No pass is running, so nothing is kept
    #[default]
    Between,
    // A pass is running and has listed nothing yet
    Running,
    // What the pass listed, or why it could not
    Listed(Result<Vec<OpenPullRequest>, ForgeError>),
}

/// What the pass in progress listed, and what each review read found
#[derive(Debug, Default)]
pub(super) struct Snapshot {
    pass: RefCell<Pass>,
    // The counts each pull request's review was last read at without finding
    // a rework to start, so a read waits for a count to change
    settled: RefCell<BTreeMap<u64, ReviewMarks>>,
}

impl Snapshot {
    /// Starts a pass, which lists the pull requests at most once
    pub(super) fn begin_pass(&self) {
        *self.pass.borrow_mut() = Pass::Running;
    }

    /// Ends the pass, so the next one lists again
    pub(super) fn end_pass(&self) {
        *self.pass.borrow_mut() = Pass::Between;
    }

    /// Whether a pass is running
    pub(super) fn in_pass(&self) -> bool {
        !matches!(*self.pass.borrow(), Pass::Between)
    }

    /// Whether `pr`'s reviews and threads are the same as when they were last
    /// read and nothing came of them, or there are none
    pub(super) fn nothing_new(&self, pr: &OpenPullRequest) -> bool {
        pr.marks.reviews == 0 || self.settled.borrow().get(&pr.number) == Some(&pr.marks)
    }

    /// Records that `pr`'s reviews, as counted when listed, were read and gave
    /// no rework
    pub(super) fn settle(&self, pr: &OpenPullRequest) {
        self.settled.borrow_mut().insert(pr.number, pr.marks);
    }

    /// Forgets the pull requests that are not among `open`
    pub(super) fn keep_only(&self, open: &[OpenPullRequest]) {
        self.settled
            .borrow_mut()
            .retain(|number, _| open.iter().any(|pr| pr.number == *number));
    }
}

impl Runner {
    // The open pull requests: what the pass listed, or a listing made now,
    // which a pass keeps for its later steps
    pub(super) fn open_listed(&self) -> Result<Vec<OpenPullRequest>, ForgeError> {
        let mut pass = self.snapshot.pass.borrow_mut();
        match &*pass {
            Pass::Listed(listed) => listed.clone(),
            Pass::Between => self.ports.forge.open_pull_requests(&self.remote),
            Pass::Running => {
                let listed = self.ports.forge.open_pull_requests(&self.remote);
                *pass = Pass::Listed(listed.clone());
                listed
            }
        }
    }

    // Pull request `number` as a gate or a merge reads it: from the pass's
    // listing when it holds the pull request, and from the forge otherwise
    pub(super) fn pull_request_view(&self, number: u64) -> Result<PullRequest, ForgeError> {
        if self.snapshot.in_pass()
            && let Ok(open) = self.open_listed()
            && let Some(pr) = open.iter().find(|pr| pr.number == number)
        {
            return Ok(pr.view());
        }
        self.ports.forge.pull_request(&self.remote, number)
    }
}

#[cfg(test)]
mod tests;
