//! The forge reads the gate and the merge make with the runner's lock let go
//!
//! A step that finds the read it needs missing returns it as a job (`turn/reads.rs`) and is made
//! again with the answer, which stays for its later asks. A read that fails reaches the step as
//! the inline call returned it. Read in place where no pass runs to make the job.

use super::super::Runner;
use super::super::report::Begin;
use crate::ports::{FailedLogs, ForgeError, PullRequest, QueueStanding};

impl Runner {
    /// Pull request `number` as the gate or the merge reads it: from the pass's listing, which a
    /// read lists first where the pass has made none. A read that failed parks the step for the
    /// gate, and a read to make is the step's `Begin::Reads`.
    pub(in crate::runner) fn gate_view(&self, number: u64) -> Result<PullRequest, Box<Begin>> {
        let viewed = match self.kept_view(number) {
            Some(viewed) => viewed,
            None => match self.view_job(number) {
                Some(job) => return Err(Box::new(Begin::Reads(job))),
                None => self.pull_request_view(number),
            },
        };
        viewed.map_err(|e| Box::new(self.gate_failed(format!("cannot read #{number}: {e}"))))
    }

    /// The end of each failed run's log at `head`, errors included, or the step's `Begin::Reads`
    pub(in crate::runner) fn failed_logs(
        &self,
        head: &str,
        merge_group: bool,
    ) -> Result<Result<FailedLogs, ForgeError>, Box<Begin>> {
        if let Some(kept) = self.reads.logs(head, merge_group) {
            return Ok(kept);
        }
        match self.logs_job(head, merge_group) {
            Some(job) => Err(Box::new(Begin::Reads(job))),
            None => Ok(self
                .ports
                .forge
                .failed_run_logs(&self.remote, head, merge_group)),
        }
    }

    /// Where pull request `number` stands in the merge queue. A read that failed parks the step
    /// for the gate, and a read to make is the step's `Begin::Reads`.
    pub(in crate::runner) fn queue_standing(
        &self,
        number: u64,
    ) -> Result<QueueStanding, Box<Begin>> {
        let read = match self.reads.standing(number) {
            Some(kept) => kept,
            None => match self.standing_job(number) {
                Some(job) => return Err(Box::new(Begin::Reads(job))),
                None => self.ports.forge.merge_queue(&self.remote, number),
            },
        };
        read.map_err(|e| Box::new(self.gate_failed(format!("cannot read #{number}'s queue: {e}"))))
    }
}
