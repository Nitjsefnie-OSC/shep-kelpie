//! What the end of a worker's turn reads, made with the runner's lock let go
//!
//! A turn's end asks git for the head on `origin`, the worktree's head and its uncommitted files,
//! and asks the forge for the pull request the branch opened. `hear_news` builds an [`EndJob`] for
//! them while it holds the lock, runs it with the lock let go and keeps the [`EndRead`] for
//! `end_turn`, which finds each answer where it would have asked. A turn the job was not made for
//! reads in place: a fork worker pushes before it reads, so its reads wait for that.

use std::path::PathBuf;
use std::sync::Arc;

use super::Runner;
use crate::board::OpenPullRequest;
use crate::ports::{Forge, ForgeError};
use crate::settings::ForgeSlug;
use crate::ticket::TicketId;
use crate::worktree::{self, Remote};

/// The reads of one turn's end, which needs no lock
pub(in crate::runner) struct EndJob {
    ticket: TicketId,
    checkout: PathBuf,
    worktree: PathBuf,
    branch: String,
    remote: Remote,
    // The forge and its repo, when the branch has no pull request yet to look for
    listing: Option<(Arc<dyn Forge>, ForgeSlug)>,
}

/// What an [`EndJob`] found, each as the inline call returned it
#[derive(Debug)]
pub(in crate::runner) struct EndRead {
    ticket: TicketId,
    origin: Result<String, String>,
    local: Result<String, String>,
    uncommitted: Result<Vec<String>, String>,
    listed: Option<Result<Vec<OpenPullRequest>, ForgeError>>,
}

impl EndJob {
    /// Reads
    pub(in crate::runner) fn run(self) -> EndRead {
        let origin = worktree::origin_head(&self.checkout, &self.branch, &self.remote)
            .map_err(|e| e.to_string());
        let local = worktree::head(&self.checkout, &self.worktree).map_err(|e| e.to_string());
        let uncommitted =
            worktree::uncommitted(&self.checkout, &self.worktree).map_err(|e| e.to_string());
        let listed = self
            .listing
            .map(|(forge, remote)| forge.open_pull_requests(&remote));
        EndRead {
            ticket: self.ticket,
            origin,
            local,
            uncommitted,
            listed,
        }
    }
}

impl EndRead {
    /// The branch's head on `origin` as the worker's own push, as `Runner::own_push` says it
    pub(in crate::runner) fn own_push(&self, adopted: bool) -> Option<String> {
        let head = self.origin.clone().ok()?;
        if !adopted {
            return Some(head);
        }
        (self.local.as_ref().ok()? == &head).then_some(head)
    }

    /// The commit the worktree has checked out, or none when git could not say
    pub(in crate::runner) fn worktree_head(&self, issue: TicketId) -> Option<String> {
        match &self.local {
            Ok(head) => Some(head.clone()),
            Err(e) => {
                eprintln!("cannot read {}'s worktree head: {e}", issue.shown());
                None
            }
        }
    }

    /// The worktree's uncommitted files, or why git could not list them
    pub(in crate::runner) fn uncommitted(&self) -> Result<Vec<String>, String> {
        self.uncommitted.clone()
    }

    /// The open pull request from `branch`, when the job listed them
    pub(in crate::runner) fn pull_request_from(&self, branch: &str) -> Option<Option<u64>> {
        self.listed
            .as_ref()
            .map(|listed| from_branch(listed.as_ref().ok(), branch))
    }
}

/// The number of the pull request in `open` whose head is `branch`
pub(super) fn from_branch(open: Option<&Vec<OpenPullRequest>>, branch: &str) -> Option<u64> {
    open?
        .iter()
        .find(|pr| pr.head == branch)
        .map(|pr| pr.number)
}

impl Runner {
    /// The reads `ticket`'s turn end makes, or none when it reads in place
    pub(in crate::runner) fn end_job(&self, ticket: TicketId) -> Option<EndJob> {
        let item = self.state.item(ticket)?;
        if !item.remote.is_origin() {
            return None;
        }
        Some(EndJob {
            ticket,
            checkout: self.settings.git.checkout.clone(),
            worktree: item.worktree.clone(),
            branch: item.branch.clone(),
            remote: item.remote.clone(),
            listing: item
                .pull_request
                .is_none()
                .then(|| (Arc::clone(&self.ports.forge), self.remote.clone())),
        })
    }

    /// Keeps what a turn's end read, for `end_turn` to take
    pub(in crate::runner) fn keep_end_read(&mut self, read: EndRead) {
        self.reads.ended = Some(read);
    }

    /// What the focused work item's turn end read with the lock let go, once
    pub(in crate::runner) fn take_end_read(&mut self) -> Option<EndRead> {
        let ticket = self.focus?;
        self.reads.ended.take_if(|read| read.ticket == ticket)
    }
}
