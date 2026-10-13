//! The security review of a contribution's head, in flight
//!
//! One read at a time, on a thread of its own, so the runner's lock is not
//! held for the minutes a model takes and a call past its ceiling is ended
//! like any other. The thread does the whole read, the fetch, the diff and
//! the call, and its verdict comes back as news; only then is it recorded.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::thread;

use super::super::Runner;
use super::super::contributions::Job;
use super::News;
use crate::ports::{Ending, Timestamp};
use crate::state::{SecurityVerdict, StateError};

/// How long the security review gets to read one head, in seconds
pub(in crate::runner) const CEILING: u64 = 15 * 60;

/// A head's security review, in flight
#[derive(Debug)]
pub(in crate::runner) struct Securing {
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

impl Runner {
    /// Starts reading `job`'s head on a thread of its own, ended past its
    /// ceiling, whose verdict comes back as news
    pub(in crate::runner) fn launch_security(&mut self, job: Job) {
        let now = self.ports.clock.now();
        let (news, wake) = (self.flights.send.clone(), self.flights.wake.clone());
        let ending = Ending::default();
        let held = ending.clone();
        let agents = Arc::clone(&self.ports.agents);
        let (pull_request, head) = (job.pull_request, job.head.clone());
        let name = format!("#{pull_request} security");
        let run = move || {
            // A panic in the read leaves the head unread, as a failed call does.
            let read = catch_unwind(AssertUnwindSafe(|| job.read(agents.as_ref(), &held)));
            let verdict = read.unwrap_or_else(|_| {
                eprintln!("contribution #{pull_request}: the security review panicked");
                None
            });
            let heard = News::Secured {
                pull_request,
                head,
                verdict,
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
            deadline: Timestamp(now.0.saturating_add(CEILING)),
            ending,
        });
        self.flights.security_cursor = pull_request;
        self.flights.security_launched = true;
    }

    // Records the end of the security review in flight: the verdict, when it
    // gave one, or nothing, which leaves the head to be read at the next poll.
    pub(super) fn security_heard(
        &mut self,
        pull_request: u64,
        head: &str,
        verdict: Option<SecurityVerdict>,
    ) -> Result<(), StateError> {
        self.flights.securing = None;
        match verdict {
            Some(verdict) => self.record_security(pull_request, head, verdict),
            None => Ok(()),
        }
    }
}
