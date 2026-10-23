//! Approving a trusted head's held CI runs and following them
//!
//! GitHub holds a fork's first CI runs until a maintainer approves them, and
//! approving runs the contributor's code. Once a poll, a contribution whose
//! current head passed the security review (or that a yes to its ruling
//! trusted) gets its held runs approved as the maintainer's `gh`
//! login, and CI on the head is followed from then: waiting until the rollup
//! has had [`CHECKS_SETTLE`](crate::runner::CHECKS_SETTLE) to register, then
//! green or red with the failed checks' names. A head that is flagged,
//! unread or stale is never approved. A refused approval is tried again at
//! the next poll, and never fails the poll. The held runs are listed again at
//! each poll while the settled rollup still shows no result, since GitHub may
//! hold a run after the first listing.

use super::super::Runner;
use super::super::gate::settled;
use crate::ports::{Checks, Timestamp};
use crate::state::{Ci, CiState, StateError};

impl Runner {
    // Approves the held runs of each followed contribution whose current head
    // passed, if kelpie has not, and moves CI on the heads it approved along.
    pub(super) fn follow_ci(&mut self) -> Result<(), StateError> {
        let now = self.ports.clock.now();
        let mut next = self.state.clone();
        for held in next.contributions.iter_mut().filter(|c| c.is_passed()) {
            let number = held.pull_request;
            match held.ci.as_mut().filter(|ci| ci.head == held.head) {
                None => {
                    if self.approve(number, &held.head) {
                        let state = match self.settings.ci.block {
                            true => CiState::Waiting,
                            false => CiState::Green,
                        };
                        held.ci = Some(Ci {
                            head: held.head.clone(),
                            since: now,
                            state,
                        });
                    }
                }
                Some(ci) if ci.state == CiState::Waiting => {
                    match self.ci_state(number, &ci.head, ci.since, now) {
                        Some(state) => ci.state = state,
                        // A run GitHub held after the listing, or one whose
                        // approval did not take, leaves the rollup empty or
                        // pending past its settling time.
                        None if settled(ci.since, now) => {
                            self.approve(number, &ci.head);
                        }
                        None => {}
                    }
                }
                Some(_) => {}
            }
        }
        match next == self.state {
            true => Ok(()),
            false => self.save(next),
        }
    }

    // Whether the held runs of `head` were approved, or listed as none; a
    // refusal is logged for the next poll to try again.
    fn approve(&self, number: u64, head: &str) -> bool {
        match self.ports.forge.approve_workflow_runs(&self.remote, head) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("contribution #{number}: cannot approve the runs on {head}: {e}");
                false
            }
        }
    }

    // Where CI stands on `head`, which kelpie approved at `since`, or `None`
    // while it is not settled: the rollup is trusted two minutes after the
    // approval, as at the gate, and only when the forge reads the same head.
    fn ci_state(
        &self,
        number: u64,
        head: &str,
        since: Timestamp,
        now: Timestamp,
    ) -> Option<CiState> {
        if !settled(since, now) {
            return None;
        }
        let pr = match self.ports.forge.pull_request(&self.remote, number) {
            Ok(pr) => pr,
            Err(e) => {
                eprintln!("contribution #{number}: cannot read CI on {head}: {e}");
                return None;
            }
        };
        if pr.head != head {
            return None;
        }
        match pr.checks {
            Checks::None | Checks::Pending(_) => None,
            // A contribution is never a draft, so skipped runs are green.
            Checks::Passed | Checks::Skipped => Some(CiState::Green),
            Checks::Failed(failed) => Some(CiState::Red(failed)),
        }
    }
}
