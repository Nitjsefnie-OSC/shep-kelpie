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
//!
//! The board's read, which lists them with the runner's lock let go before the
//! pass begins, hands its listing over: [`Snapshot::preload`] keeps it, and the
//! pass that begins next starts with it listed.

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
    // A listing made before the pass began, which the pass starts with
    preloaded: RefCell<Option<Result<Vec<OpenPullRequest>, ForgeError>>>,
    // The counts each pull request's review was last read at without finding
    // a rework to start, so a read waits for a count to change
    settled: RefCell<BTreeMap<u64, ReviewMarks>>,
}

impl Snapshot {
    /// Starts a pass, which lists the pull requests at most once, or not at all
    /// when a listing was preloaded
    pub(super) fn begin_pass(&self) {
        *self.pass.borrow_mut() = match self.preloaded.borrow_mut().take() {
            Some(listed) => Pass::Listed(listed),
            None => Pass::Running,
        };
    }

    /// Ends the pass, so the next one lists again
    pub(super) fn end_pass(&self) {
        *self.pass.borrow_mut() = Pass::Between;
        self.drop_preload();
    }

    /// Keeps a listing made with no pass running for the pass that begins next,
    /// and gives it to the pass running now if that has listed nothing yet
    pub(super) fn preload(&self, listed: Result<Vec<OpenPullRequest>, ForgeError>) {
        let mut pass = self.pass.borrow_mut();
        match &*pass {
            Pass::Between => *self.preloaded.borrow_mut() = Some(listed),
            Pass::Running => *pass = Pass::Listed(listed),
            Pass::Listed(_) => {}
        }
    }

    /// Forgets a preloaded listing, so the next pass lists for itself
    pub(super) fn drop_preload(&self) {
        *self.preloaded.borrow_mut() = None;
    }

    /// What the pass in progress listed, if it has
    pub(super) fn listed(&self) -> Option<Result<Vec<OpenPullRequest>, ForgeError>> {
        match &*self.pass.borrow() {
            Pass::Listed(listed) => Some(listed.clone()),
            Pass::Between | Pass::Running => None,
        }
    }

    /// Whether a pass is running
    pub(super) fn in_pass(&self) -> bool {
        !matches!(*self.pass.borrow(), Pass::Between)
    }

    /// Whether the pass running has listed nothing yet, so the first step that needs the listing
    /// makes it
    pub(super) fn lists_next(&self) -> bool {
        matches!(*self.pass.borrow(), Pass::Running)
    }

    /// Whether the pass's listing holds pull request `number`
    pub(super) fn serves(&self, number: u64) -> bool {
        matches!(&*self.pass.borrow(), Pass::Listed(Ok(open)) if open.iter().any(|pr| pr.number == number))
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
            && let Some(pr) = view_of(&self.open_listed(), number)
        {
            return Ok(pr);
        }
        self.ports.forge.pull_request(&self.remote, number)
    }
}

/// Pull request `number` as `listed` holds it
pub(super) fn view_of(
    listed: &Result<Vec<OpenPullRequest>, ForgeError>,
    number: u64,
) -> Option<PullRequest> {
    let open = listed.as_ref().ok()?;
    open.iter()
        .find(|pr| pr.number == number)
        .map(OpenPullRequest::view)
}

#[cfg(test)]
mod tests;
