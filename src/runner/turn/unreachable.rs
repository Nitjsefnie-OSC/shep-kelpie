//! A worker's turn whose model host cannot be reached
//!
//! A host that is down, restarting or refusing connections passes, so a turn
//! that ends with [`AgentError::Unreachable`] does not park its work item on
//! a ruling. It keeps its session, and the runner tries the same call again
//! after a pause that grows from [`RETRY_FIRST`] to [`RETRY_MAX`]. The item
//! shows as waiting for its model host from the first failure. Past [`LIMIT`]
//! since that failure the turn parks as any failed one does, with the last
//! error. The wait is in memory only, so a restart begins a new one: the
//! turn is still marked running, and the restart resumes its session.

use std::collections::BTreeMap;
use std::time::Duration;

use super::super::Runner;
use super::super::alert::backoff;
use super::super::flight::Waiting;
use super::super::report::Begin;
use crate::ports::{AgentCall, AgentError, Timestamp, Wait};

/// Seconds the wait lasts from the first failure before the turn parks
///
/// A constant: `agents.fallback_after` does not move an item for this wait.
const LIMIT: u64 = 30 * 60;

/// Seconds before the first retry
const RETRY_FIRST: u64 = 30;

/// The longest pause between two retries, in seconds
const RETRY_MAX: u64 = 5 * 60;

// One work item's turn waiting for its host
#[derive(Debug)]
struct Pause {
    // The call that failed, tried again as it was, session and prompt
    call: AgentCall,
    // Whether the turn is its work item's first
    first: bool,
    // The seconds of the turn's ceiling it had when the call began, which the
    // retry has again, since the wait is not the worker's time
    left: u64,
    // When the host first could not be reached
    since: Timestamp,
    // The failures in a row
    failures: u32,
    // Until when no retry is made
    until: Timestamp,
}

/// The turns waiting for their model host, by issue, in memory only
#[derive(Debug, Default)]
pub(in crate::runner) struct Unreachable(BTreeMap<u64, Pause>);

impl Unreachable {
    /// What `issue`'s turn waits for, if it waits for its host
    pub(in crate::runner) fn waiting(&self, issue: u64) -> Option<Waiting> {
        self.0.get(&issue).map(|pause| Waiting {
            since: pause.since,
            why: Wait::Unreachable,
        })
    }

    /// When the soonest turn waiting for its host is tried again
    pub(in crate::runner) fn next_try(&self, now: Timestamp) -> Option<Duration> {
        let soonest = self.0.values().map(|pause| pause.until).min()?;
        Some(Duration::from_secs(soonest.0.saturating_sub(now.0)))
    }

    /// Forgets the waits of work items no longer open
    pub(in crate::runner) fn keep_only(&mut self, open: &[u64]) {
        self.0.retain(|issue, _| open.contains(issue));
    }
}

impl Runner {
    /// Whether `issue`'s turn that just ended with `result` waits for its
    /// host and is tried again, rather than parks on a ruling
    ///
    /// Any result but an unreachable host ends the wait. Call it while the
    /// turn's flight still stands, for the call it keeps.
    pub(in crate::runner) fn waits_for_host(
        &mut self,
        issue: u64,
        result: &Result<crate::ports::AgentReply, AgentError>,
    ) -> bool {
        let Err(AgentError::Unreachable(_, last)) = result else {
            if self.unreachable.0.remove(&issue).is_some() {
                self.board_changed();
            }
            return false;
        };
        let now = self.ports.clock.now();
        let Some(flight) = self.flights.turn_call(issue) else {
            return false;
        };
        let (call, first, left) = flight;
        let held = self.unreachable.0.remove(&issue);
        let since = held.as_ref().map_or(now, |pause| pause.since);
        let failures = held.as_ref().map_or(0, |pause| pause.failures) + 1;
        self.board_changed();
        if now.0.saturating_sub(since.0) >= LIMIT {
            self.notes.push(format!(
                "issue #{issue}'s model host stayed unreachable for {} minutes: {}",
                LIMIT / 60,
                last.trim()
            ));
            return false;
        }
        let pause = backoff(RETRY_FIRST, failures, RETRY_MAX);
        if held.is_none() {
            self.notes.push(format!(
                "issue #{issue}'s model host cannot be reached, so its turn is tried \
                 again in {pause} seconds: {}",
                last.trim()
            ));
        }
        self.unreachable.0.insert(
            issue,
            Pause {
                call,
                first,
                left,
                since,
                failures,
                until: Timestamp(now.0.saturating_add(pause)),
            },
        );
        true
    }

    /// The call of `issue`'s turn waiting for its host, once its pause has
    /// passed, or `Begin::Idle` while it holds; `None` for a turn that does
    /// not wait
    pub(super) fn retry_for_host(&self, issue: u64) -> Option<Begin> {
        let pause = self.unreachable.0.get(&issue)?;
        let now = self.ports.clock.now();
        if now < pause.until || self.draining {
            return Some(Begin::Idle);
        }
        Some(Begin::Call {
            call: pause.call.clone(),
            deadline: Timestamp(now.0.saturating_add(pause.left)),
            first: pause.first,
            compact: false,
        })
    }
}

#[cfg(test)]
mod tests;
