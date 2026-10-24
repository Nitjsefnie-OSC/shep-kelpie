//! A forge call the test holds open

use std::sync::mpsc::{Receiver, Sender};
use std::sync::{PoisonError, mpsc};
use std::time::Duration;

use super::FakeForge;

// Which forge call a test waits on
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum At {
    // The next call the runner makes, whichever it is
    Any,
    // The read of `main`'s workflow runs, which a pass makes for every work item
    MainRuns,
    // The read of one issue, which a review round makes for its criteria
    Issue,
    // The listing of the open issues, which triage's poll makes
    Listing,
    // The listing of the ready issues, which the board's poll starts with
    Board,
    // The read of one pull request, which a parked work item's check makes
    PullRequest,
    // The listing of the open pull requests, which following the bumps makes
    OpenPullRequests,
    // The read of a review bot's activity on one pull request, which a bot's round makes
    Activity,
    // The read of the comments and threads on one pull request, which a talk step makes
    Talk,
    // The read of the end of the failed runs' logs, which a red run makes
    FailedLogs,
    // The read of where a pull request stands in the merge queue, which a merge makes
    MergeQueue,
}

// Who reached the forge and who lets it go
#[derive(Debug)]
pub struct Gate {
    entered: Sender<()>,
    release: Receiver<()>,
    at: At,
}

/// A forge that holds its next call open until this is dropped
///
/// A pass asks the forge with the runner's lock held, and each call waits its
/// turn behind the login's pace, so a call held here is a pass that holds the
/// lock. A test takes one to ask a trigger of a runner in the middle of a pass.
#[derive(Debug)]
pub struct HeldCall {
    entered: Receiver<()>,
    // Dropping it is what lets the call through
    _release: Sender<()>,
}

impl HeldCall {
    /// Whether a call reached the forge and waits here, within `within`
    pub fn waiting(&self, within: Duration) -> bool {
        self.entered.recv_timeout(within).is_ok()
    }
}

impl FakeForge {
    /// Holds the forge's next call open, so a test can ask a trigger of a
    /// runner whose pass is in the middle of it
    pub fn hold_next_call(&self) -> HeldCall {
        self.hold_open(At::Any)
    }

    /// Holds the next read of `main`'s workflow runs open, and lets every
    /// other call through
    pub fn hold_next_main_runs_read(&self) -> HeldCall {
        self.hold_open(At::MainRuns)
    }

    /// Holds the next read of one issue open, and lets every other call through
    pub fn hold_next_issue_read(&self) -> HeldCall {
        self.hold_open(At::Issue)
    }

    /// Holds the next listing of the open issues open, and lets every other
    /// call through
    pub fn hold_next_listing_read(&self) -> HeldCall {
        self.hold_open(At::Listing)
    }

    /// Holds the next listing of the ready issues open, and lets every other call through
    pub fn hold_next_board_read(&self) -> HeldCall {
        self.hold_open(At::Board)
    }

    /// Holds the next read of one pull request open, and lets every other call through
    pub fn hold_next_pull_request_read(&self) -> HeldCall {
        self.hold_open(At::PullRequest)
    }

    /// Holds the next listing of the open pull requests open, and lets every other call through
    pub fn hold_next_open_pull_requests_read(&self) -> HeldCall {
        self.hold_open(At::OpenPullRequests)
    }

    /// Holds the next read of a review bot's activity open, and lets every other call through
    pub fn hold_next_activity_read(&self) -> HeldCall {
        self.hold_open(At::Activity)
    }

    /// Holds the next read of a pull request's comments open, and lets every other call through
    pub fn hold_next_talk_read(&self) -> HeldCall {
        self.hold_open(At::Talk)
    }

    /// Holds the next read of the failed runs' logs open, and lets every other call through
    pub fn hold_next_failed_logs_read(&self) -> HeldCall {
        self.hold_open(At::FailedLogs)
    }

    /// Holds the next read of a pull request's merge queue standing open, and lets every other
    /// call through
    pub fn hold_next_merge_queue_read(&self) -> HeldCall {
        self.hold_open(At::MergeQueue)
    }

    fn hold_open(&self, at: At) -> HeldCall {
        let (entered, heard) = mpsc::channel();
        let (release, waited) = mpsc::channel();
        *self.hold.lock().unwrap_or_else(PoisonError::into_inner) = Some(Gate {
            entered,
            release: waited,
            at,
        });
        HeldCall {
            entered: heard,
            _release: release,
        }
    }

    /// Holds one call here until the test drops the [`HeldCall`] it took.
    /// A gate named for one call waits for that call and lets the rest through.
    pub fn wait_if_held(&self, at: At) {
        let gate = {
            let mut hold = self.hold.lock().unwrap_or_else(PoisonError::into_inner);
            match hold.as_ref() {
                Some(gate) if gate.at == At::Any || gate.at == at => hold.take().expect("checked"),
                _ => return,
            }
        };
        let _ = gate.entered.send(());
        let _ = gate.release.recv();
    }
}
