//! A check stuck at queued on a finished run
//!
//! GitHub sometimes leaves one job of a finished workflow run queued: the
//! run reads completed, the job never starts, and its check never
//! concludes. A check that started ends at its own timeout, so only a
//! queued one is waited on past `ci.queued_minutes`, counted from the wait's
//! own `since`. Past it, the run is rerun whole, once per head, and the cap
//! starts again from the rerun. Still queued past it, the item is parked on a
//! `check-queued` ruling. The rerun is kept in memory, like `held_up`: a
//! restart can cost one more, which is harmless.

use std::collections::HashMap;

use crate::ports::{Queued, Rerun, Timestamp};
use crate::runner::Runner;
use crate::runner::report::{Begin, StepReport};
use crate::state::{StateError, Stuck};
use crate::ticket::TicketId;
use crate::work_item::{Phase, Review};

/// The head of each work item whose queued check has had its run rerun
#[derive(Debug, Default)]
pub(in crate::runner) struct Reruns(HashMap<TicketId, String>);

impl Reruns {
    /// Forgets the work items no longer open
    pub(in crate::runner) fn keep_only(&mut self, open: &[TicketId]) {
        self.0.retain(|ticket, _| open.contains(ticket));
    }
}

impl Runner {
    /// What the step does about `queued`, the checks still queued on `head`
    /// since `since`: nothing while they are within the cap, a rerun of their
    /// run once past it, and a ruling when they are still queued after that
    ///
    /// `review` is the round whose CI wait it is, when one.
    pub(in crate::runner) fn check_queued(
        &mut self,
        number: u64,
        head: &str,
        since: Timestamp,
        queued: &[Queued],
        review: Option<&Review>,
    ) -> Result<Option<Begin>, StateError> {
        let now = self.ports.clock.now();
        let cap = u64::from(self.settings.ci.queued_minutes.get()) * 60;
        if now.0.saturating_sub(since.0) < cap {
            return Ok(None);
        }
        // A check with no run to name cannot be rerun, so it is waited on.
        let Some(run) = queued.iter().find_map(|check| check.run) else {
            return Ok(None);
        };
        let checks: Vec<String> = (queued.iter())
            .filter(|check| check.run == Some(run))
            .map(|check| check.name.clone())
            .collect();
        let item = self.current().expect("CI runs on a work item");
        let issue = item.issue;
        if self.queued_reruns.0.get(&issue).map(String::as_str) == Some(head) {
            let url = format!(
                "https://github.com/{}/actions/runs/{run}",
                self.remote.as_str()
            );
            let stuck = Stuck::CheckQueued {
                head: head.to_owned(),
                checks,
                run,
                url,
                review: review.cloned(),
            };
            return self.raise(number, stuck.into()).map(Some);
        }
        if let Err(e) = self.ports.forge.rerun_run(&self.remote, run, Rerun::All) {
            return Ok(Some(
                self.gate_failed(format!("cannot rerun run {run}: {e}")),
            ));
        }
        self.queued_reruns.0.insert(issue, head.to_owned());
        self.update(|item| match &mut item.phase {
            Phase::Ci { since, .. } => *since = now,
            Phase::Review(review) => {
                if let Some(ci) = review.ci.as_deref_mut() {
                    ci.since = now;
                }
            }
            _ => {}
        })?;
        Ok(Some(Begin::Report(StepReport::CiRerun {
            issue,
            pull_request: number,
            head: head.to_owned(),
            checks,
        })))
    }
}

#[cfg(test)]
mod tests;
