//! CI on the head a reviewer is about to read
//!
//! A red run is a sure defect that costs no model's read and no bot's window,
//! so no reviewer reads a head until it is pushed and green (ADR 0008). The
//! round that is about to start a reviewer, or summon a bot, first waits for
//! the run on the head it would read: the same two minutes for the checks to
//! register, then pending waits, green goes on, and red is the worker's CI fix
//! turn, counted toward `ci.fix_attempts`, which comes back to this round.
//! A head already green stays so for the rest of the pass, and with
//! `ci.block` off the round starts at once.

use super::super::Runner;
use super::super::gate::settled;
use super::super::report::Begin;
use crate::ports::{Checks, PullRequestState};
use crate::state::{StateError, Stuck};
use crate::work_item::{Awaited, Phase, Review};

impl Runner {
    /// What the step does instead of `review`'s reviewer reading the pushed
    /// head, when CI has not passed on it: wait, or send the worker its CI
    /// fix
    ///
    /// `None` lets the reviewer go ahead, and keeps in `review` the run it
    /// waited on.
    pub(super) fn before_read(&mut self, review: &mut Review) -> Result<Option<Begin>, StateError> {
        if !self.settings.ci.block {
            return Ok(None);
        }
        let Some(head) = review.reading.clone() else {
            return Ok(None);
        };
        if review
            .ci
            .as_ref()
            .is_some_and(|ci| ci.green && ci.head == head)
        {
            return Ok(None);
        }
        let item = self.current().expect("a round is a work item's");
        let (issue, number) = (item.issue, item.pull_request);
        let number = number.expect("review runs on a pull request");
        let pr = match self.ports.forge.pull_request(&self.remote, number) {
            Ok(pr) => pr,
            Err(e) => {
                return Ok(Some(
                    self.gate_failed(format!("cannot read #{number}: {e}")),
                ));
            }
        };
        // A pull request merged or closed by hand ends as the gate ends it,
        // and no reviewer reads it.
        match pr.state {
            PullRequestState::Open => {}
            PullRequestState::Merged => {
                return self.merged(issue, number, pr.head, false).map(Some);
            }
            PullRequestState::Closed => {
                return self.raise(number, Stuck::Closed.into()).map(Some);
            }
        }
        // The forge lags a push by a moment, and its checks are of its head.
        if pr.head != head {
            return Ok(Some(Begin::Idle));
        }
        let now = self.ports.clock.now();
        let since = (review.ci.as_ref())
            .filter(|ci| ci.head == head)
            .map_or(now, |ci| ci.since);
        let mut ci = Awaited {
            head: head.clone(),
            since,
            green: false,
        };
        let conclusion = match &pr.checks {
            Checks::Failed(_) => "failed",
            Checks::Skipped => "skipped",
            _ => "passed",
        };
        let failed = match &pr.checks {
            Checks::Failed(checks) => checks.clone(),
            _ => Vec::new(),
        };
        let verdict = match pr.checks {
            _ if !settled(since, now) => None,
            Checks::None | Checks::Pending => None,
            // Nothing ran on a draft whose jobs all skip it, and the real
            // run starts only once it is marked ready, which a bot's round
            // does after this wait: so a draft waits, and doctor says why.
            Checks::Skipped if pr.draft => None,
            Checks::Passed | Checks::Skipped => {
                ci.green = true;
                Some(Ok(()))
            }
            Checks::Failed(checks) => Some(Err(checks)),
        };
        if review.ci.as_deref() != Some(&ci) {
            review.ci = Some(Box::new(ci.clone()));
            self.update(|item| {
                if let Phase::Review(review) = &mut item.phase {
                    review.ci = Some(Box::new(ci));
                }
            })?;
        }
        if verdict.is_some() {
            self.ci_settled(issue, number, &head, conclusion, &failed);
        }
        match verdict {
            None => Ok(Some(Begin::Idle)),
            Some(Ok(())) => Ok(None),
            Some(Err(checks)) => {
                let back = Review {
                    reading: None,
                    ..review.clone()
                };
                self.ci_failed(number, head, checks, Some(back)).map(Some)
            }
        }
    }
}
