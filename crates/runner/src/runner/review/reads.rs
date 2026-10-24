//! The pull request and the comments a review step reads with the runner's lock let go
//!
//! A step that finds the read it needs missing returns it as a job (`turn/reads.rs`) and is made
//! again with the answer. Every read the step made stays for its steps made again, so a step
//! that reads the pull request in more than one place asks the forge once.

use super::super::Runner;
use super::super::report::Begin;
use super::super::turn::reads::Job;
use crate::ports::{ForgeError, PullRequest, Talk};

impl Runner {
    /// Pull request `number` as the step reads it, errors included, or the job that reads it
    ///
    /// Read in place where no pass runs to make the job and step again.
    pub(in crate::runner) fn read_view(
        &self,
        number: u64,
    ) -> Result<Result<PullRequest, ForgeError>, Box<Job>> {
        if let Some(kept) = self.kept_view(number) {
            return Ok(kept);
        }
        match self.pull_request_job(number) {
            Some(job) => Err(job),
            None => Ok(self.ports.forge.pull_request(&self.remote, number)),
        }
    }

    /// Pull request `number` as a round's step needs it: a read that failed parks the step for
    /// the gate, and a read to make is the step's `Begin::Reads`
    pub(in crate::runner) fn round_view(&mut self, number: u64) -> Result<PullRequest, Begin> {
        match self.read_view(number) {
            Ok(Ok(pr)) => Ok(pr),
            Ok(Err(e)) => Err(self.gate_failed(format!("cannot read #{number}: {e}"))),
            Err(job) => Err(Begin::Reads(job)),
        }
    }

    /// What people wrote on pull request `number`, errors included, or the job that reads it
    pub(in crate::runner) fn read_talk(
        &self,
        number: u64,
    ) -> Result<Result<Talk, ForgeError>, Box<Job>> {
        if let Some(kept) = self.reads.talk(number) {
            return Ok(kept);
        }
        match self.talk_job(number) {
            Some(job) => Err(job),
            None => Ok(self.ports.forge.talk(&self.remote, number)),
        }
    }
}
