//! Opening a work item for a ticket

use std::fmt;

use super::Runner;
use super::trigger::issue_list;
use crate::board::{LabelError, agent_label, old_worker_label, pinned};
use crate::ports::{ForgeError, Issue};
use crate::settings::AgentName;
use crate::state::StateError;
use crate::ticket::TicketId;
use crate::work_item::new_session_id;

/// Why `add` was refused
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddError {
    /// No slot is free under `concurrency.active_items`, counting the items waiting for
    /// one, or one for this ticket is open already: the tickets of those in
    /// the way
    InFlight(Vec<TicketId>),
    /// The forge could not show the issue
    Forge(ForgeError),
    /// The issue is closed
    Closed(TicketId),
    /// The issue's `agent:` label cannot be used
    Label(LabelError),
    /// The issue writer is labelling the issue
    Labelling(TicketId),
    /// `finish` holds the board back, so no new work item opens
    Finishing,
    /// No random session id could be drawn, with the OS's reason
    Session(String),
    /// The work item could not be saved
    State(StateError),
}

impl fmt::Display for AddError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InFlight(issues) if issues.len() == 1 => {
                write!(f, "the work item for {} is in flight", issue_list(issues))
            }
            Self::InFlight(issues) => {
                write!(f, "the work items for {} are in flight", issue_list(issues))
            }
            Self::Forge(e) => write!(f, "cannot read the issue: {e}"),
            Self::Closed(ticket) => write!(f, "{} is closed", ticket.shown()),
            Self::Label(e) => e.fmt(f),
            Self::Labelling(ticket) => write!(
                f,
                "the issue writer is labelling {}: add it once that call ends",
                ticket.shown()
            ),
            Self::Finishing => f.write_str(
                "the runner is finishing, so it opens no new work item: `shep kelpie start` \
                 lets it pick again",
            ),
            Self::Session(e) => write!(f, "cannot draw a session id: {e}"),
            Self::State(e) => e.fmt(f),
        }
    }
}

impl core::error::Error for AddError {}

impl Runner {
    /// Opens a work item for `ticket`, and returns the implementer its
    /// worker runs on. Its first turn runs on a later pass.
    ///
    /// # Errors
    ///
    /// [`AddError`] when the runner is finishing, the project has
    /// `concurrency.active_items` working or one for this issue open
    /// already, the issue cannot be read or is closed, or its `agent:` label names no
    /// listed implementer, or the change cannot be saved. Nothing changes then.
    pub fn add(&mut self, ticket: TicketId) -> Result<AgentName, AddError> {
        self.looks.poll_reset();
        self.add_allowed(ticket)?;
        let found = (self.ports.tickets.issue(&self.remote, ticket)).map_err(AddError::Forge)?;
        self.add_found(ticket, found)
    }

    // Whether a work item for `ticket` may open now, before the issue is read.
    pub(super) fn add_allowed(&self, ticket: TicketId) -> Result<(), AddError> {
        if !self.picks() {
            return Err(AddError::Finishing);
        }
        if self.state.item(ticket).is_some() {
            return Err(AddError::InFlight(vec![ticket]));
        }
        if self.flights.labelling() == Some(ticket) {
            return Err(AddError::Labelling(ticket));
        }
        if !self.slot_free() {
            return Err(AddError::InFlight(self.slot_issues()));
        }
        Ok(())
    }

    // Opens the work item for `ticket` from the issue as read. The guards are
    // checked again, since the runner may have moved since they were.
    pub(super) fn add_found(
        &mut self,
        ticket: TicketId,
        found: Issue,
    ) -> Result<AgentName, AddError> {
        self.add_allowed(ticket)?;
        if !found.open {
            return Err(AddError::Closed(ticket));
        }
        let (agent, note) = self
            .labelled_agent(ticket, &found.labels)
            .map_err(AddError::Label)?;
        let session = new_session_id().map_err(|e| AddError::Session(e.to_string()))?;
        let mut next = self.state.clone();
        let pinned = pinned(&found.labels);
        let mut item = self.fresh(ticket, found.title, agent.clone(), session);
        item.pinned = pinned;
        next.work_items.push(item);
        Self::label_ruled(&mut next, ticket);
        self.save(next).map_err(AddError::State)?;
        self.notes.extend(note);
        self.mark_held(ticket, true);
        Ok(agent)
    }

    // The implementer a ticket with `labels` runs on: the one its `agent:`
    // label names, or the default. Beside it, the log line an old `worker:`
    // label gets once the work item opens, since it no longer picks anything.
    pub(super) fn labelled_agent(
        &self,
        ticket: TicketId,
        labels: &[String],
    ) -> Result<(AgentName, Option<String>), LabelError> {
        let listed = self.agents.implementer_names();
        if let Some(agent) = agent_label(labels, &listed)? {
            return Ok((agent, None));
        }
        let agent = self.agents.default_implementer.name.clone();
        let note = old_worker_label(labels).map(|old| {
            format!(
                "{} is labelled `{old}`, which kelpie no longer reads, so it runs on the \
                 default implementer, {agent}: an `agent:<name>` label picks another that \
                 `agents.implementers` lists",
                ticket.named().to_lowercase()
            )
        });
        Ok((agent, note))
    }
}
