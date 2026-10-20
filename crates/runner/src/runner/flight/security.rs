//! A call on a contribution's head, in flight: its security review, or the
//! pass of the reviewers over it
//!
//! One call at a time, on a thread of its own, so the runner's lock is not
//! held for the minutes a model takes and a call past its ceiling is ended
//! like any other. The thread does the whole call, the fetch, the diff and
//! the reading, and what it came to comes back as news; only then is it
//! recorded.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::thread;

use super::super::Runner;
use super::super::contributions::{Job, Reading};
use super::News;
use crate::ports::{Ending, Timestamp};
use crate::state::StateError;

/// How long the security review gets to read one head, in seconds
pub(in crate::runner) const CEILING: u64 = 15 * 60;

/// A call on a contribution's head, in flight
#[derive(Debug)]
pub(in crate::runner) struct Securing {
    pull_request: u64,
    deadline: Timestamp,
    ending: Ending,
}

impl Securing {
    // Ends the call once past its ceiling.
    pub(in crate::runner) fn end_overdue(&self, now: Timestamp) {
        if self.deadline <= now {
            self.ending.end();
        }
    }

    // When it is due to be ended, while it has not been.
    pub(in crate::runner) fn ceiling(&self) -> Option<Timestamp> {
        (!self.ending.asked()).then_some(self.deadline)
    }
}

impl super::Flights {
    /// Whether a contribution's head is being read for security
    pub(in crate::runner) fn securing(&self) -> bool {
        self.securing.is_some()
    }

    /// The contribution whose head is being read, which holds a slot while it is
    pub(in crate::runner) fn securing_pull_request(&self) -> Option<u64> {
        self.securing.as_ref().map(|s| s.pull_request)
    }

    /// Whether a security review started on `pull_request` since the last poll
    pub(in crate::runner) fn security_tried(&self, pull_request: u64) -> bool {
        self.security_tried.contains(&pull_request)
    }

    /// Lets every contribution's head be tried again, as a poll does
    pub(in crate::runner) fn security_retry(&mut self) {
        self.security_tried.clear();
    }
}

impl Runner {
    /// Starts the call `job` is on a thread of its own, ended past its
    /// ceiling, whose end comes back as news
    pub(in crate::runner) fn launch_security(&mut self, job: Job) {
        let now = self.ports.clock.now();
        let (news, wake) = (self.flights.send.clone(), self.flights.wake.clone());
        let ending = Ending::default();
        let held = ending.clone();
        let agents = self.counted(Arc::clone(&self.ports.agents));
        let reviewer = Arc::clone(&self.ports.reviewer);
        let (pull_request, head) = (job.pull_request(), job.head().to_owned());
        let name = format!("#{pull_request} {}", job.name());
        let unread = job.unread();
        let run = move || {
            // A panic in the call leaves the head unread, as a failed call does.
            let run = catch_unwind(AssertUnwindSafe(|| {
                job.run(agents.as_ref(), reviewer.as_ref(), &held)
            }));
            let reading = run.unwrap_or_else(|_| {
                eprintln!("contribution #{pull_request}: the call on its head panicked");
                unread
            });
            let heard = News::Secured {
                pull_request,
                head,
                reading,
            };
            let _ = news.send(heard);
            if let Some(wake) = &wake {
                let _ = wake.send(());
            }
        };
        if let Err(e) = thread::Builder::new().name(name).spawn(run) {
            // Nothing is in flight, so the next poll tries again.
            eprintln!("contribution #{pull_request}: cannot start a thread to read it: {e}");
            return;
        }
        self.flights.securing = Some(Securing {
            pull_request,
            deadline: Timestamp(now.0.saturating_add(CEILING)),
            ending,
        });
        self.flights.security_tried.push(pull_request);
    }

    // Records the end of the call in flight, and gives its slot back: what it
    // cost, and its verdict or pass when it gave one, which otherwise leaves
    // the head to be read at the next poll.
    pub(super) fn security_heard(
        &mut self,
        pull_request: u64,
        head: &str,
        reading: Reading,
    ) -> Result<(), StateError> {
        self.flights.securing = None;
        match reading {
            Reading::Security(reading) => self.record_security(pull_request, head, reading),
            Reading::Pass(walked) => self.record_pass(pull_request, head, *walked),
        }
    }
}
