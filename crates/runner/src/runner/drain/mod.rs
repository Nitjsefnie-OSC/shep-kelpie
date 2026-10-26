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
//! worker turn in flight on a harness that can end it at a tool result: it
//! resumes at the next start, which `undrain keep` puts off by [`PAUSE_HOLD`].

use std::collections::BTreeSet;

use serde::Serialize;

use super::Runner;
use super::posted::Pending;
use super::turn::paused::resumes_pause;
use crate::guard::Pause;
use crate::ports::{Timestamp, Tools};
use crate::settings::Harness;
use crate::ticket::TicketId;
use crate::work_item::WorkItem;

/// Seconds a turn that ended for a pause stays unstarted after `undrain keep`
pub(super) const PAUSE_HOLD: u64 = 10 * 60;

/// The turns an `undrain keep` found ended for a pause, and until when they stay unstarted
#[derive(Debug)]
pub(super) struct PauseHold {
    until: Timestamp,
    issues: BTreeSet<TicketId>,
}

/// Where a pause set for a turn in flight ends it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Pausing {
    /// At its next tool result
    AtTool,
    /// At its end, since its harness has no hook
    AtEnd,
    /// Nowhere: the pause could not be set
    Failed,
}

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
    pub at_tool: Vec<TicketId>,
}

/// A call in flight
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CallRunning {
    /// The ticket of the work item it is for, and none for the project manager's
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<TicketId>,
    /// Whose call it is
    pub role: CallRole,
}

/// Whose call is in flight
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// The triager, researching an untouched issue
    Triager,
    /// The panel's experts, voting on a ruling
    Panel,
    /// The project manager's agent, running a batch retro
    Retro,
    /// The e2e run of a dependency bump's head, on the project's e2e host
    E2e,
    /// A session in the maintainer's terminal, as its harness's facts name it
    ///
    /// A restart would take with it a forwarder or a trusted folder that only
    /// this image holds.
    Attached(&'static str),
}

impl CallRole {
    // The name `status` shows.
    fn name(self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::Reviewer => "reviewer",
            Self::Pm => "pm",
            Self::IssueWriter => "issue_writer",
            Self::Security => "security",
            Self::Triager => "triager",
            Self::Panel => "panel",
            Self::Retro => "retro",
            Self::E2e => "e2e",
            Self::Attached(name) => name,
        }
    }
}

impl Serialize for CallRole {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl Runner {
    /// Starts no new call from now until a restart or [`Runner::undrain`]
    ///
    /// With `at_tool`, each worker turn in flight that a hook can end is
    /// asked to end at its next tool result.
    pub(super) fn drain(&mut self, at_tool: bool) {
        // A `drain` or `undrain` that landed while a pass held the lock is taken
        // up here rather than left for the pass after this one, which would act
        // on it after this. A pause asked for there is still asked for here.
        let (posted_drain, at_tool) = match self.postings.take_pending() {
            Some(Pending::Drain { at_tool: was }) => (true, at_tool || was),
            Some(Pending::Undrain { .. }) => {
                self.pause_line = None;
                (false, at_tool)
            }
            None => (false, at_tool),
        };
        // The one call a drain starts: a retro over what finished since the
        // last. It starts here, so the status this answers lists it. One that
        // cannot start yet is asked for on a later pass.
        self.retro_book.at_drain |= posted_drain || !self.draining();
        self.postings.set_draining(true);
        self.retro_launched();
        if at_tool {
            self.pause_turns();
        }
    }

    /// Acts on a `drain` or `undrain` that landed while a pass held the runner's
    /// lock
    ///
    /// Its flag was set at once, so no new call started. What it asked the
    /// runner itself is taken up here, at the pass's next lock.
    pub(super) fn take_posted(&mut self) {
        match self.postings.take_pending() {
            Some(Pending::Drain { at_tool }) => {
                self.retro_book.at_drain = true;
                self.retro_launched();
                if at_tool {
                    self.pause_turns();
                }
            }
            Some(Pending::Undrain { keep }) => self.lift_drain(keep),
            None => {}
        }
    }

    /// Starts calls again, and with `keep` leaves a turn that ended for a pause
    /// unstarted for [`PAUSE_HOLD`]
    pub(super) fn undrain(&mut self, keep: bool) {
        // A `drain` that landed while a pass held the lock is taken up here
        // rather than left for the pass after this one, which would pause the
        // turns this one let go of.
        self.postings.take_pending();
        self.postings.set_draining(false);
        self.lift_drain(keep);
    }

    // Withdraws the pauses, and holds the turns that ended for one when `keep`.
    fn lift_drain(&mut self, keep: bool) {
        self.pause_line = None;
        let until = Timestamp(self.ports.clock.now().0.saturating_add(PAUSE_HOLD));
        self.pause_hold = keep.then(|| PauseHold {
            until,
            issues: (self.state.work_items.iter())
                .filter(|item| self.ended_by_pause(item))
                .map(|item| item.issue)
                .collect(),
        });
        self.withdraw_pauses();
    }

    // Whether `item`'s turn ended for a pause that `pause` set, not one the
    // build limits set.
    fn ended_by_pause(&self, item: &WorkItem) -> bool {
        resumes_pause(item) && !self.builds.holds_pause(item.issue)
    }

    // Whether `item`'s turn ended for a pause that gave up, and its hold has
    // not passed. An `undrain keep` not yet taken up holds every such turn, so
    // the pass that holds the lock while it lands cannot start one.
    pub(super) fn pause_held(&self, item: &WorkItem) -> bool {
        if self.postings.keeping() {
            return self.ended_by_pause(item);
        }
        (self.pause_hold.as_ref()).is_some_and(|hold| {
            self.ports.clock.now() < hold.until
                && hold.issues.contains(&item.issue)
                && resumes_pause(item)
        })
    }

    // Withdraws the pause set for each worker turn in flight.
    // A turn that ended for the pause already stays paused, and resumes.
    fn withdraw_pauses(&mut self) {
        for item in &self.state.work_items {
            // A pause the build limits set for a turn in flight stands.
            let held = self.builds.holds_pause(item.issue);
            if !held {
                Pause::in_build(&item.build).withdraw();
            }
        }
    }

    // Sets the pause for each worker turn in flight that a tool result can end,
    // and says which when that differs from the last time it said.
    fn pause_turns(&mut self) {
        let (mut asked, mut refused) = (Vec::new(), Vec::new());
        for item in &self.state.work_items {
            match self.pause_turn(item) {
                Pausing::AtTool => asked.push(item.issue.shown()),
                Pausing::AtEnd => {}
                Pausing::Failed => refused.push(item.issue.shown()),
            }
        }
        let mut line = if !asked.is_empty() {
            let turns = asked.join(", ");
            format!("drain pause: turns asked to end at their next tool call: {turns}")
        } else if refused.is_empty() {
            "drain pause: no worker turn in flight can end at a tool call".to_owned()
        } else {
            "drain pause: no turn was asked to end at a tool call".to_owned()
        };
        if !refused.is_empty() {
            let turns = refused.join(", ");
            line.push_str(&format!("; the pause could not be written for {turns}"));
        }
        if self.pause_line.as_ref() != Some(&line) {
            self.notes.push(line.clone());
            self.pause_line = Some(line);
        }
    }

    // Sets the pause for `item`'s worker turn in flight. A tool result can end
    // only a call whose harness has the hook, and whose settings carry it.
    pub(super) fn pause_turn(&self, item: &WorkItem) -> Pausing {
        let ends = (self.flights.turn_call(item.issue))
            .is_some_and(|(call, ..)| call.harness.ends_at_tool() && call.tools == Tools::Work);
        if !ends {
            return Pausing::AtEnd;
        }
        match Pause::in_build(&item.build).request() {
            Ok(()) => Pausing::AtTool,
            Err(e) => {
                eprintln!(
                    "cannot pause issue #{}'s turn at a tool call: {e}",
                    item.issue
                );
                Pausing::Failed
            }
        }
    }

    /// What `status` shows of the drain, while the runner drains
    pub(super) fn draining_status(&self) -> Option<Draining> {
        self.draining().then(|| self.drain_state())
    }

    /// The calls in flight and the longest any of them may run, whether or not
    /// the runner drains
    ///
    /// Posted at each point the status is, so a `drain` that lands while a pass
    /// holds the runner's lock can answer what is running.
    pub(super) fn drain_state(&self) -> Draining {
        let pm = self.agents.pm.is_some().then_some(super::pm::CEILING);
        let labelling = (self.flights.labelling()).map(|_| super::flight::label::CEILING);
        let securing = (self.flights.securing()).then_some(super::flight::security::CEILING);
        let triaging = (self.flights.triaging()).map(|_| super::flight::triage::CEILING);
        let retro = self
            .flights
            .retro_running()
            .then_some(super::retro::CEILING);
        let panel = (self.flights.panel_running()).then_some(super::panel::CEILING);
        let ceiling = (self.turn_ceiling().as_secs())
            .max(pm.unwrap_or(0))
            .max(labelling.unwrap_or(0))
            .max(securing.unwrap_or(0))
            .max(triaging.unwrap_or(0))
            .max(retro.unwrap_or(0))
            .max(panel.unwrap_or(0));
        let attached = Harness::ALL.into_iter().flat_map(|harness| {
            let role = harness.facts().attached_role.map(CallRole::Attached);
            (role.into_iter()).flat_map(move |role| {
                (self.sessions_attached_on(harness).into_iter()).map(move |issue| CallRunning {
                    issue: Some(issue),
                    role,
                })
            })
        });
        let at_tool = (self.state.work_items.iter())
            .filter(|item| self.flights.runs_turn(item.issue))
            .filter(|item| Pause::in_build(&item.build).is_requested())
            .map(|item| item.issue)
            .collect();
        Draining {
            calls: self.flights.running().into_iter().chain(attached).collect(),
            ceiling,
            at_tool,
        }
    }
}

#[cfg(test)]
mod tests;
