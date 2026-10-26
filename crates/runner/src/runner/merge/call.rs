//! The forge calls of a merge, made with the runner's lock let go
//!
//! Marking a draft ready and merging a pull request wait their turn behind the login's pace.
//! The gate asks for each as a job (`turn/reads.rs`), is made again with the answer, and acts on
//! it as it would have on the call.

use std::sync::Arc;

use super::{PullRequestState, Runner};
use crate::ports::{Forge, ForgeError, PullRequest};
use crate::runner::report::Begin;
use crate::settings::ForgeSlug;

/// The marking of pull request `number` ready, which needs no lock
pub(in crate::runner) struct Marking {
    pub(in crate::runner) forge: Arc<dyn Forge>,
    pub(in crate::runner) remote: ForgeSlug,
    pub(in crate::runner) number: u64,
}

impl Marking {
    /// Marks it, as the gate would have inline
    pub(in crate::runner) fn run(self) -> Result<(), ForgeError> {
        self.forge.mark_ready(&self.remote, self.number)
    }
}

/// The merge of pull request `number` at `head`, which needs no lock
pub(in crate::runner) struct Merging {
    pub(in crate::runner) forge: Arc<dyn Forge>,
    pub(in crate::runner) remote: ForgeSlug,
    pub(in crate::runner) number: u64,
    pub(in crate::runner) head: String,
    pub(in crate::runner) auto: bool,
}

/// What the merge call and the reads after it came to
#[derive(Debug, Clone)]
pub(in crate::runner) enum Merged {
    /// The forge refused the merge and the yes was asked for, so the gate fails with this
    Refused(String),
    /// The call failed, and the pull request is in the merge queue all the same
    Waiting,
    /// The call failed, and the pull request is not merged at `head`, so the gate rules on it
    Declined(String),
    /// The call failed and the pull request could not be read to tell
    Unread(ForgeError),
    /// The pull request as the forge shows it after the call, which says whether it merged or
    /// only queued
    Read(Result<PullRequest, ForgeError>),
}

impl Merging {
    /// Merges, with the calls the gate would have made inline
    pub(in crate::runner) fn run(self) -> Merged {
        let Self {
            forge,
            remote,
            number,
            head,
            auto,
        } = self;
        if let Err(e) = forge.merge(&remote, number, &head) {
            let reason = format!("cannot merge #{number}: {e}");
            if !auto {
                return Merged::Refused(reason);
            }
            // A lost answer may have queued it rather than merged it.
            let waiting = forge
                .merge_queue(&remote, number)
                .is_ok_and(|standing| standing.queued || standing.armed);
            if waiting {
                return Merged::Waiting;
            }
            match forge.pull_request(&remote, number) {
                Ok(pr) if pr.state == PullRequestState::Merged && pr.head == head => {}
                Ok(_) => return Merged::Declined(reason),
                Err(e) => return Merged::Unread(e),
            }
        }
        // With a merge queue on, the call only queued the pull request.
        Merged::Read(forge.pull_request(&remote, number))
    }
}

impl Runner {
    /// The marking of pull request `number` ready, as the job made it, or the step's
    /// `Begin::Reads`
    pub(super) fn ready_marked(&self, number: u64) -> Result<Result<(), ForgeError>, Box<Begin>> {
        if let Some(kept) = self.reads.readied(number) {
            return Ok(kept);
        }
        match self.ready_job(number) {
            Some(job) => Err(Box::new(Begin::Reads(job))),
            None => Ok(self.ports.forge.mark_ready(&self.remote, number)),
        }
    }

    /// The merge of pull request `number` at `head`, as the job made it, or the step's
    /// `Begin::Reads`
    pub(super) fn merge_made(
        &self,
        number: u64,
        head: &str,
        auto: bool,
    ) -> Result<Merged, Box<Begin>> {
        if let Some(kept) = self.reads.merged(number) {
            return Ok(kept);
        }
        match self.merge_job(number, head, auto) {
            Some(job) => Err(Box::new(Begin::Reads(job))),
            None => Ok(Merging {
                forge: Arc::clone(&self.ports.forge),
                remote: self.remote.clone(),
                number,
                head: head.to_owned(),
                auto,
            }
            .run()),
        }
    }
}
