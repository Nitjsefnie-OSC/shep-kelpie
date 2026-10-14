//! The e2e run of a web bump's head, in flight
//!
//! One run at a time, on a thread of its own, since the host takes one run
//! at a time and a run waits in its queue for as long as the dog's ceiling.
//! The thread checks the head out, asks the dog, removes the checkout, and
//! what the run came to comes back as news; only then is it recorded.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::thread;

use super::super::Runner;
use super::News;
use crate::ports::{E2eOutcome, E2eRun};
use crate::state::StateError;
use crate::worktree;

/// The bump whose head is being run, and whether a run began since the loop
/// last looked
#[derive(Debug, Default)]
pub(in crate::runner) struct Proving {
    open: Option<(u64, String)>,
    launched: bool,
    // The heads the dog said have no host to run on, which are not asked again
    declined: Vec<(u64, String)>,
}

/// What an e2e run of a bump's head came to
pub(in crate::runner) struct Proof {
    pull_request: u64,
    head: String,
    outcome: E2eOutcome,
}

impl super::Flights {
    /// Whether the run of `head`, bump `pull_request`'s, is in flight
    pub(in crate::runner) fn proving(&self, pull_request: u64, head: &str) -> bool {
        (self.proving.open.as_ref()).is_some_and(|(n, h)| *n == pull_request && h == head)
    }

    /// Whether an e2e run is in flight
    pub(in crate::runner) fn proof_running(&self) -> bool {
        self.proving.open.is_some()
    }

    /// Whether the dog said the project has no e2e host when it was asked to
    /// run `head`, bump `pull_request`'s
    pub(in crate::runner) fn declined(&self, pull_request: u64, head: &str) -> bool {
        (self.proving.declined.iter()).any(|(n, h)| *n == pull_request && h == head)
    }

    /// Remembers that `head` has no host to run on
    pub(in crate::runner) fn decline(&mut self, pull_request: u64, head: &str) {
        self.proving.declined.push((pull_request, head.to_owned()));
    }
}

impl Runner {
    /// Starts the e2e run of bump `pull_request`'s `head` on a thread of its
    /// own, whose end comes back as news, and says whether it started
    pub(in crate::runner) fn launch_proof(&mut self, pull_request: u64, head: &str) -> bool {
        let repo = self.settings.git.checkout.clone();
        let (worktree, door) = (
            self.paths.bump_worktree(pull_request),
            self.paths.door.clone(),
        );
        let runner = Arc::clone(&self.ports.e2e_runner);
        let (news, wake) = (self.flights.send.clone(), self.flights.wake.clone());
        let head_run = head.to_owned();
        let run = move || {
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                let fetched =
                    worktree::contribution_checkout(&repo, pull_request, &head_run, &worktree);
                let outcome = match fetched {
                    Ok(()) => runner.run(&E2eRun {
                        worktree: worktree.clone(),
                        door,
                    }),
                    Err(e) => E2eOutcome::NotRun {
                        reason: format!("cannot check the head out: {e}"),
                    },
                };
                if let Err(e) = worktree::remove_view(&repo, &worktree) {
                    eprintln!("bump #{pull_request}: cannot remove its checkout: {e}");
                }
                outcome
            }))
            .unwrap_or_else(|_| E2eOutcome::NotRun {
                reason: "the run panicked".to_owned(),
            });
            let proof = Proof {
                pull_request,
                head: head_run,
                outcome,
            };
            let _ = news.send(News::Proved(proof));
            if let Some(wake) = &wake {
                let _ = wake.send(());
            }
        };
        let name = format!("#{pull_request} e2e");
        if let Err(e) = thread::Builder::new().name(name).spawn(run) {
            // Nothing is in flight, so the next poll tries again.
            eprintln!("bump #{pull_request}: cannot start a thread to run its e2e tests: {e}");
            return false;
        }
        self.flights.proving.open = Some((pull_request, head.to_owned()));
        self.flights.proving.launched = true;
        true
    }

    /// Whether a run began since the last time this was asked
    pub(in crate::runner) fn proofs_launched(&mut self) -> bool {
        std::mem::take(&mut self.flights.proving.launched)
    }

    // Records the end of the run in flight, and frees the host for the next.
    pub(super) fn proof_heard(&mut self, proof: Proof) -> Result<(), StateError> {
        self.flights.proving.open = None;
        self.record_proof(proof.pull_request, &proof.head, proof.outcome)
    }
}
