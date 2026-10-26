//! Reading the board with the runner's lock let go
//!
//! The poll lists the ready issues, the open pull requests, and each finished
//! ticket's issue. A pass would hold the lock through all of it, so
//! [`read_board`] runs first: [`Runner::poll_job`] decides a poll is due,
//! [`BoardJob::run`] reads, and [`Runner::store_board_read`] keeps what was
//! read. [`Runner::take_board_read`] hands it to `dispatch`, which stamps the
//! look. A read a pass did not use, or one kelpie's own write follows, is
//! dropped. The next pass reads again.

use std::sync::{Arc, Mutex};

use super::Runner;
use super::parent::{self, Refused, Unbuilt, Unlisted};
use super::trigger::lock;
use super::turn::Slot;
use crate::board::{OpenPullRequest, ReadyIssue};
use crate::ports::{Forge, ForgeError, Tickets, Timestamp};
use crate::settings::ForgeSlug;
use crate::ticket::TicketId;

/// A read of the board the runner decided was due, which needs no lock
pub(super) struct BoardJob {
    tickets: Arc<dyn Tickets>,
    forge: Arc<dyn Forge>,
    repo: ForgeSlug,
    now: Timestamp,
    /// The writes of kelpie's made when the job was built
    writes: u64,
    /// The open pull requests, when the pass has listed them already
    listed: Option<Result<Vec<OpenPullRequest>, ForgeError>>,
    finished: Vec<TicketId>,
    items: Vec<TicketId>,
    refused: Refused,
    unbuilt: Unbuilt,
}

/// What a read of the board found
#[derive(Debug)]
pub(super) struct BoardRead {
    /// The writes of kelpie's made before the read began, which it is stale after
    writes: u64,
    /// The ready issues, or why they could not be read
    pub(super) ready: Result<Vec<ReadyIssue>, ForgeError>,
    /// The open pull requests, listed once the ready issues were read
    open: Option<Result<Vec<OpenPullRequest>, ForgeError>>,
    /// The finished tickets the forge reads as closed
    pub(super) closed: Vec<TicketId>,
    /// The search for a parent off the ready list, when the board lists none to close
    pub(super) unlisted: Option<Unlisted>,
}

/// Reads the board, if a poll is due, asking the forge with the runner's lock let go
pub(super) fn read_board(runner: &Mutex<Runner>) {
    let job = lock(runner).poll_job();
    let Some(job) = job else {
        return;
    };
    let read = job.run();
    lock(runner).store_board_read(read);
}

impl BoardJob {
    // Reads in the order the board always has: the ready issues, the open pull requests, the issue
    // of each finished ticket the board no longer lists, then the parents off its list. A read
    // that fails ends the rest.
    fn run(self) -> BoardRead {
        let mut read = BoardRead {
            writes: self.writes,
            ready: self.tickets.ready_issues(&self.repo),
            open: None,
            closed: Vec::new(),
            unlisted: None,
        };
        let Ok(ready) = &read.ready else {
            return read;
        };
        let open = (self.listed).unwrap_or_else(|| self.forge.open_pull_requests(&self.repo));
        let listed = open.is_ok();
        read.open = Some(open);
        if !listed {
            return read;
        }
        read.closed = (self.finished.iter().copied())
            .filter(|t| !ready.iter().any(|r| r.ticket == *t))
            .filter(|t| (self.tickets.issue(&self.repo, *t)).is_ok_and(|issue| !issue.open))
            .collect();
        if !parent::lists_done_parent(&self.refused, ready, &self.items) {
            let held: Vec<TicketId> = (ready.iter().map(|i| i.ticket))
                .chain(self.items.iter().copied())
                .collect();
            read.unlisted = Some(parent::find_unlisted(
                self.tickets.as_ref(),
                &self.repo,
                self.now,
                &held,
                &self.refused,
                self.unbuilt,
            ));
        }
        read
    }
}

impl Runner {
    // Decides whether the board is read now: while the board may open a work item and its look is
    // due, which is what the rotation asks of its step. Forgets a read an earlier pass left.
    fn poll_job(&mut self) -> Option<BoardJob> {
        self.board_read = None;
        self.snapshot.drop_preload();
        let now = self.ports.clock.now();
        if !self.board_open() || !self.looks.due(Slot::Board, now) {
            return None;
        }
        // A limit used up is learned by the first call the pass makes, which is not the board's
        // listing.
        if self.forge_hold.until(now).is_some() {
            return None;
        }
        Some(self.job_at(now, None))
    }

    fn job_at(
        &self,
        now: Timestamp,
        listed: Option<Result<Vec<OpenPullRequest>, ForgeError>>,
    ) -> BoardJob {
        BoardJob {
            tickets: Arc::clone(&self.ports.tickets),
            forge: Arc::clone(&self.ports.forge),
            repo: self.remote.clone(),
            now,
            writes: self.forge_ledger.writes(),
            listed,
            finished: self.state.finished.clone(),
            items: self.state.work_items.iter().map(|i| i.issue).collect(),
            refused: self.close_refused.clone(),
            unbuilt: self.unbuilt.clone(),
        }
    }

    // Keeps what a read found for the pass that follows, listing included
    fn store_board_read(&mut self, read: BoardRead) {
        if let Some(open) = &read.open {
            self.snapshot.preload(open.clone());
        }
        self.board_read = Some(read);
    }

    /// Starts the pass's snapshot, with the listing the board read took unless a write of
    /// kelpie's came after it
    pub(super) fn begin_pass(&mut self) {
        if (self.board_read.as_ref()).is_some_and(|read| read.writes != self.forge_ledger.writes())
        {
            self.board_read = None;
            self.snapshot.drop_preload();
        }
        self.reads.clear();
        self.snapshot.begin_pass();
    }

    /// Ends the pass, dropping a board read it did not use
    pub(super) fn end_pass(&mut self) {
        self.board_read = None;
        self.reads.clear();
        self.snapshot.end_pass();
    }

    /// What the board shows: the read taken before the pass, or one made now with the lock held
    pub(super) fn take_board_read(&mut self) -> BoardRead {
        if let Some(read) = self.board_read.take() {
            return read;
        }
        let now = self.ports.clock.now();
        let read = self.job_at(now, self.snapshot.listed()).run();
        if let Some(open) = &read.open {
            self.snapshot.preload(open.clone());
        }
        read
    }
}
