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

/// The runs rerun for each work item's queued checks, and the head they were
/// rerun at
#[derive(Debug, Default)]
pub(in crate::runner) struct Reruns(HashMap<TicketId, (String, Vec<u64>)>);

impl Reruns {
    /// Forgets the work items no longer open
    pub(in crate::runner) fn keep_only(&mut self, open: &[TicketId]) {
        self.0.retain(|ticket, _| open.contains(ticket));
    }

    fn done(&self, issue: TicketId, head: &str, run: u64) -> bool {
        (self.0.get(&issue)).is_some_and(|(at, runs)| at == head && runs.contains(&run))
    }

    fn record(&mut self, issue: TicketId, head: &str, run: u64) {
        let entry = self.0.entry(issue).or_default();
        if entry.0 != head {
            *entry = (head.to_owned(), Vec::new());
        }
        entry.1.push(run);
    }
}

impl Runner {
    /// What the step does about `queued`, the checks still queued on `head`
    /// since `since`: nothing while they are within the cap, a rerun of a
    /// run not yet rerun once past it, and a ruling when every run they
    /// belong to has been rerun and one is still queued
    ///
    /// A rerun the forge refuses, as it does for a run still in progress,
    /// waits another cap, so a backed-up runner is waited on and told once
    /// per cap. `review` is the round whose CI wait it is, when one.
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
        let Some(first) = queued.iter().find_map(|check| check.run) else {
            return Ok(None);
        };
        let issue = self.current().expect("CI runs on a work item").issue;
        let runs = queued.iter().filter_map(|check| check.run);
        let Some(run) = { runs }.find(|run| !self.queued_reruns.done(issue, head, *run)) else {
            let url = format!(
                "https://github.com/{}/actions/runs/{first}",
                self.remote.as_str()
            );
            let stuck = Stuck::CheckQueued {
                head: head.to_owned(),
                checks: named(queued, first),
                run: first,
                url,
                review: review.cloned(),
            };
            return self.raise(number, stuck.into()).map(Some);
        };
        let rerun = self.ports.forge.rerun_run(&self.remote, run, Rerun::All);
        self.update(|item| match &mut item.phase {
            Phase::Ci { since, .. } => *since = now,
            Phase::Review(review) => {
                if let Some(ci) = review.ci.as_deref_mut() {
                    ci.since = now;
                }
            }
            _ => {}
        })?;
        if let Err(e) = rerun {
            return Ok(Some(
                self.gate_failed(format!("cannot rerun run {run}: {e}")),
            ));
        }
        self.queued_reruns.record(issue, head, run);
        Ok(Some(Begin::Report(StepReport::CiRerun {
            issue,
            pull_request: number,
            head: head.to_owned(),
            checks: named(queued, run),
        })))
    }
}

// The queued checks that belong to `run`
fn named(queued: &[Queued], run: u64) -> Vec<String> {
    (queued.iter())
        .filter(|check| check.run == Some(run))
        .map(|check| check.name.clone())
        .collect()
}

#[cfg(test)]
mod tests;
