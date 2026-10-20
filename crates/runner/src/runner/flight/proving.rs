//! The proof of a web bump's head, in flight
//!
//! One proof at a time, on a thread of its own, since the host takes one run
//! at a time and a run waits in its queue for as long as the dog's ceiling.
//! The thread checks the head out and, where the project has the browser,
//! opens the routes in it first. Unless a route failed it asks the dog for the
//! e2e run, then removes the checkout, and what the proof came to comes back as
//! news; only then is it recorded.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::thread;

use super::super::Runner;
use super::super::dev_server::{Browsed, BrowserProof};
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
    // The heads that cannot be proved, with why, which are not tried again
    declined: Vec<(u64, String, String)>,
}

/// What the proof of a bump's head came to
pub(in crate::runner) struct Proof {
    pull_request: u64,
    head: String,
    verdict: Verdict,
}

/// How the proof of a head ended
pub(in crate::runner) enum Verdict {
    /// The e2e run ended so, after the browser proof passed if it ran
    E2e { outcome: E2eOutcome, browsed: bool },
    /// The browser proof ended without a pass
    Browser(Browsed),
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

    /// Why `head`, bump `pull_request`'s, cannot be proved, if the dog said the
    /// project has no e2e host or the browser could not be had for it
    pub(in crate::runner) fn declined(&self, pull_request: u64, head: &str) -> Option<&str> {
        (self.proving.declined.iter())
            .find(|(n, h, _)| *n == pull_request && h == head)
            .map(|(_, _, why)| why.as_str())
    }

    /// Remembers that `head` cannot be proved, and why
    pub(in crate::runner) fn decline(&mut self, pull_request: u64, head: &str, why: &str) {
        (self.proving.declined).push((pull_request, head.to_owned(), why.to_owned()));
    }
}

impl Runner {
    /// Starts the proof of bump `pull_request`'s `head` on a thread of its
    /// own, whose end comes back as news, and says whether it started
    ///
    /// The browser proof, if the project has one, comes before the e2e run.
    pub(in crate::runner) fn launch_proof(
        &mut self,
        pull_request: u64,
        head: &str,
        browser: Option<BrowserProof>,
    ) -> bool {
        let repo = self.settings.git.checkout.clone();
        let base = self.settings.git.base().to_owned();
        let (worktree, door) = (
            self.paths.bump_worktree(pull_request),
            self.paths.door.clone(),
        );
        let runner = Arc::clone(&self.ports.e2e_runner);
        let (news, wake) = (self.flights.send.clone(), self.flights.wake.clone());
        let head_run = head.to_owned();
        let run = move || {
            let verdict = catch_unwind(AssertUnwindSafe(|| {
                let fetched = worktree::contribution_checkout(
                    &repo,
                    pull_request,
                    &head_run,
                    &worktree,
                    &base,
                );
                let verdict = match fetched {
                    Ok(()) => prove(browser, &worktree, || {
                        runner.run(&E2eRun {
                            worktree: worktree.clone(),
                            door,
                        })
                    }),
                    Err(e) => Verdict::E2e {
                        outcome: E2eOutcome::NotRun {
                            reason: format!("cannot check the head out: {e}"),
                        },
                        browsed: false,
                    },
                };
                if let Err(e) = worktree::remove_view(&repo, &worktree) {
                    eprintln!("bump #{pull_request}: cannot remove its checkout: {e}");
                }
                verdict
            }))
            .unwrap_or_else(|_| Verdict::E2e {
                outcome: E2eOutcome::NotRun {
                    reason: "the run panicked".to_owned(),
                },
                browsed: false,
            });
            let proof = Proof {
                pull_request,
                head: head_run,
                verdict,
            };
            let _ = news.send(News::Proved(proof));
            if let Some(wake) = &wake {
                let _ = wake.send(());
            }
        };
        let name = format!("#{pull_request} proof");
        if let Err(e) = thread::Builder::new().name(name).spawn(run) {
            // Nothing is in flight, so the next poll tries again.
            eprintln!("bump #{pull_request}: cannot start a thread to prove it: {e}");
            self.release_proof_port(pull_request);
            return false;
        }
        self.flights.proving.open = Some((pull_request, head.to_owned()));
        self.flights.proving.launched = true;
        // The proof's run is in flight from here.
        self.post();
        true
    }

    /// Whether a run began since the last time this was asked
    pub(in crate::runner) fn proofs_launched(&mut self) -> bool {
        std::mem::take(&mut self.flights.proving.launched)
    }

    // Records the end of the run in flight, and frees the host for the next.
    pub(super) fn proof_heard(&mut self, proof: Proof) -> Result<(), StateError> {
        self.flights.proving.open = None;
        self.release_proof_port(proof.pull_request);
        self.record_proof(proof.pull_request, &proof.head, proof.verdict)
    }
}

// The browser proof, then the e2e run unless a route failed or the browser was not to be had.
fn prove(
    browser: Option<BrowserProof>,
    worktree: &std::path::Path,
    e2e: impl FnOnce() -> E2eOutcome,
) -> Verdict {
    let browsed = match browser.map(|proof| proof.run(worktree)) {
        None => false,
        Some(Browsed::Passed) => true,
        Some(ended) => return Verdict::Browser(ended),
    };
    Verdict::E2e {
        outcome: e2e(),
        browsed,
    }
}
