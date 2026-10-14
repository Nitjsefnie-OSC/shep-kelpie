//! Draining: a runner that starts no new call until it restarts
//!
//! `drain` holds back every call the runner would start next, a worker's
//! turn, a reviewer's session or local round, and the project manager's
//! wake, while the calls already in flight run to their end and the rest of
//! each pass goes on, merges included. A drain that starts with work items
//! finished since the last retro starts the batch retro over them, the one
//! call it starts. `status` then lists the calls still
//! running, so `shep kelpie upgrade` can restart the runner once none is.
//! Draining is kept in memory only: a restart ends it, as `undrain` does.
//!
//! `drain pause`, which `shep kelpie pause` sends, also sets a pause for each
//! worker turn in flight on a harness that can end a turn at a tool result:
//! the turn ends at its next tool call and is resumed at the next start.

use serde::Serialize;

use super::Runner;
use crate::guard::Pause;
use crate::ports::Tools;
use crate::settings::Harness;
use crate::work_item::WorkItem;

/// What `status` shows while the runner drains
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Draining {
    /// The calls still running: each work item's, then the project manager's
    pub calls: Vec<CallRunning>,
    /// The longest a call the runner starts may run before it is ended, in
    /// seconds: the turn ceiling, or the project manager's or the issue
    /// writer's in flight when longer
    pub ceiling: u64,
    /// The work items whose worker turn ends at its next tool call, since
    /// `drain pause` set a pause for it
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub at_tool: Vec<u64>,
}

/// A call in flight
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CallRunning {
    /// The issue of the work item it is for, and none for the project manager's
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<u64>,
    /// Whose call it is
    pub role: CallRole,
}

/// Whose call is in flight
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CallRole {
    /// A worker's turn
    Worker,
    /// A reviewer's session or local round
    Reviewer,
    /// The project manager's wake
    Pm,
    /// The issue writer, labelling an issue
    IssueWriter,
    /// The security review, reading a contribution's head
    Security,
    /// The panel's experts, voting on a ruling
    Panel,
    /// The project manager's agent, running a batch retro
    Retro,
    /// A pi session in the maintainer's terminal, whose forwarder a restart
    /// would take with it
    PiSession,
    /// A goose session in the maintainer's terminal, whose forwarder a restart
    /// would take with it
    GooseSession,
}

impl Runner {
    /// Starts no new call from now until a restart or [`Runner::undrain`]
    ///
    /// With `at_tool`, each worker turn in flight that a hook can end is
    /// asked to end at its next tool result.
    pub(super) fn drain(&mut self, at_tool: bool) {
        // The one call a drain starts: a retro over what finished since the
        // last. It starts here, so the status this answers lists it. One that
        // cannot start yet is asked for on a later pass.
        self.retro_book.at_drain |= !self.draining;
        self.draining = true;
        self.retro_launched();
        if at_tool {
            self.pause_turns();
        }
    }

    /// Starts calls again
    pub(super) fn undrain(&mut self) {
        self.draining = false;
        // A turn that ended for the pause already stays paused, and resumes.
        for item in &self.state.work_items {
            Pause::in_build(&item.build).withdraw();
        }
    }

    // Sets the pause for each worker turn in flight that a tool result can end.
    fn pause_turns(&self) {
        for item in &self.state.work_items {
            self.pause_turn(item);
        }
    }

    // Sets the pause for `item`'s worker turn in flight, and says whether a
    // tool result can end it: only a call whose harness has the hook, and whose
    // settings carry it.
    pub(super) fn pause_turn(&self, item: &WorkItem) -> bool {
        let ends = (self.flights.turn_call(item.issue))
            .is_some_and(|(call, ..)| call.harness.ends_at_tool() && call.tools == Tools::Work);
        if ends && let Err(e) = Pause::in_build(&item.build).request() {
            eprintln!(
                "cannot pause issue #{}'s turn at a tool call: {e}",
                item.issue
            );
        }
        ends
    }

    /// What `status` shows of the drain, while the runner drains
    pub(super) fn draining_status(&self) -> Option<Draining> {
        if !self.draining {
            return None;
        }
        let pm = self.agents.pm.is_some().then_some(super::pm::CEILING);
        let labelling = (self.flights.labelling()).map(|_| super::flight::label::CEILING);
        let securing = (self.flights.securing()).then_some(super::flight::security::CEILING);
        let retro = self
            .flights
            .retro_running()
            .then_some(super::retro::CEILING);
        let panel = (self.flights.panel_running()).then_some(super::panel::CEILING);
        let ceiling = (self.turn_ceiling().as_secs())
            .max(pm.unwrap_or(0))
            .max(labelling.unwrap_or(0))
            .max(securing.unwrap_or(0))
            .max(retro.unwrap_or(0))
            .max(panel.unwrap_or(0));
        let attached_on = |harness, role| {
            (self.sessions_attached_on(harness).into_iter()).map(move |issue| CallRunning {
                issue: Some(issue),
                role,
            })
        };
        let attached = attached_on(Harness::Pi, CallRole::PiSession)
            .chain(attached_on(Harness::Goose, CallRole::GooseSession));
        let at_tool = (self.state.work_items.iter())
            .filter(|item| self.flights.runs_turn(item.issue))
            .filter(|item| Pause::in_build(&item.build).is_requested())
            .map(|item| item.issue)
            .collect();
        Some(Draining {
            calls: self.flights.running().into_iter().chain(attached).collect(),
            ceiling,
            at_tool,
        })
    }
}

#[cfg(test)]
mod tests;
