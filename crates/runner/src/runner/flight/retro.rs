//! The batch retro's call in flight
//!
//! One call with the project manager's agent, on a thread of its own and
//! beside the standing session's wakes. Its ledger draft is keyed apart
//! from a wake's, so the two never share a line.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::thread;

use super::super::Runner;
use super::super::in_flight::Key;
use super::super::turn_hooks::TurnHooks;
use super::{End, News};
use crate::ports::{AgentCall, AgentError, Ending, Timestamp};
use crate::settings::Harness;
use crate::usage::{CallKind, Draft};

/// The key of the retro's draft in the usage ledger's calls in flight; no
/// work item owns the retro's call
pub(in crate::runner) const KEY: Key = Key::Retro;

/// The batch retro, in flight
#[derive(Debug)]
pub(in crate::runner) struct Retrospecting {
    deadline: Timestamp,
    ending: Ending,
}

impl Retrospecting {
    // When it is due to be ended, unless it has been
    pub(super) fn ceiling(&self) -> Option<Timestamp> {
        (!self.ending.asked()).then_some(self.deadline)
    }

    // Ends it if it is past its ceiling
    pub(super) fn end_overdue(&self, now: Timestamp) {
        if self.deadline <= now {
            self.ending.end();
        }
    }
}

impl Runner {
    /// Starts the retro's `call` on the agent named `agent`, ended past
    /// `ceiling` seconds, whose end comes back as news
    pub(in crate::runner) fn launch_retro(&mut self, call: AgentCall, ceiling: u64, agent: &str) {
        let now = self.ports.clock.now();
        let draft = Draft::of(&call, agent, CallKind::Retro, now);
        self.flights.ledger.open(KEY, draft, self.pacer_lines());
        self.call_started(&call, agent);
        let ledger = self.flights.ledger.clone();
        let (news, wake) = (self.flights.send.clone(), self.flights.wake.clone());
        let ending = Ending::default();
        let held = ending.clone();
        let agents = self.counted(TurnHooks::around(
            Arc::clone(&self.ports.agents),
            &self.hooks,
            &self.notes,
            None,
        ));
        let run = move || {
            let ran = catch_unwind(AssertUnwindSafe(|| End::Turn(agents.run(&call, &held))));
            let end = ran.unwrap_or_else(End::Panicked);
            if end.reached_no_model() {
                ledger.no_model(KEY);
            }
            let _ = news.send(News::Retro(end));
            if let Some(wake) = &wake {
                let _ = wake.send(());
            }
        };
        if let Err(e) = thread::Builder::new().name("retro".into()).spawn(run) {
            let reason = format!("cannot start a thread for the call: {e}");
            let end = End::Turn(Err(AgentError::Spawn(Harness::ClaudeCode, reason)));
            let _ = self.flights.send.send(News::Retro(end));
        }
        let deadline = Timestamp(now.0.saturating_add(ceiling));
        self.flights.retro = Some(Retrospecting { deadline, ending });
    }

    /// Records the end of the retro in flight: its line goes in the ledger,
    /// and its reply is the report
    pub(super) fn retro_heard(&mut self, end: End) {
        self.flights.retro = None;
        let open = self.flights.ledger.take(KEY);
        let result = match end {
            End::Turn(result) => result,
            // A panicking retro is skipped as a failed one is: it must not end the runner.
            End::Panicked(_) => {
                if let Some(open) = &open {
                    self.panicked_line(open);
                }
                Err(AgentError::Setup("its call panicked".into()))
            }
            End::Review(_) => return,
        };
        // A call's line goes in only once its end is heard.
        if let Some(line) = open.and_then(|open| self.turn_line(&open, &result)) {
            self.append_call(line, false);
        }
        self.retro_ended(result);
    }
}
