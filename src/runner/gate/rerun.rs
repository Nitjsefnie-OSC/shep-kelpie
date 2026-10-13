//! A red run whose log names no file the branch changed is rerun once
//!
//! A flaky test outside the branch fails a run the worker cannot fix, and
//! its credential cannot rerun a job. Kelpie reads the failed runs' logs as
//! the login `gh` acts as, and when none names a file the branch changed it
//! reruns that head's failed jobs itself, once, before it spends a turn. A
//! head that fails again goes to the worker with the log, saying so. A log
//! kelpie cannot read, a branch it cannot list, or a forge that refuses the
//! rerun leaves the red run to the worker.

use crate::runner::Runner;
use crate::runner::report::{Begin, StepReport};
use crate::state::StateError;
use crate::work_item::Phase;
use crate::worktree;

impl Runner {
    /// Reruns the failed jobs of `head`, instead of a fix turn, when its logs
    /// name no file the branch changed and it has not been rerun
    pub(super) fn rerun_if_unrelated(
        &mut self,
        number: u64,
        head: &str,
        checks: &[String],
    ) -> Result<Option<Begin>, StateError> {
        let item = self.current().expect("CI runs on a work item");
        if item.reran.as_deref() == Some(head) {
            return Ok(None);
        }
        let issue = item.issue;
        let Ok(logs) = self.ports.forge.failed_run_logs(&self.remote, head, false) else {
            return Ok(None);
        };
        let Ok(changed) = worktree::changed_files(&self.settings.git.checkout, head) else {
            return Ok(None);
        };
        if logs.trim().is_empty() || changed.is_empty() || names_any(&logs, &changed) {
            return Ok(None);
        }
        if self.ports.forge.rerun_failed(&self.remote, head).is_err() {
            return Ok(None);
        }
        // The checks read red for a moment after a rerun starts, so the
        // verdict waits out the same time a new head's checks do.
        let now = self.ports.clock.now();
        let reran = head.to_owned();
        self.update(|item| {
            item.reran = Some(reran);
            match &mut item.phase {
                Phase::Ci { since, .. } => *since = now,
                Phase::Review(review) => {
                    if let Some(ci) = review.ci.as_deref_mut() {
                        ci.since = now;
                    }
                }
                _ => {}
            }
        })?;
        Ok(Some(Begin::Report(StepReport::CiRerun {
            issue,
            pull_request: number,
            head: head.to_owned(),
            checks: checks.to_vec(),
        })))
    }
}

// Whether `log` mentions any of `files`, by the path the branch changes it at
fn names_any(log: &str, files: &[String]) -> bool {
    files.iter().any(|file| log.contains(file.as_str()))
}

#[cfg(test)]
mod tests;
