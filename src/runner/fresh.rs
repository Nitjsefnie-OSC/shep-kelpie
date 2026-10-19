//! A work item opened for an issue, before its first turn

use super::Runner;
use crate::home::work_branch;
use crate::ports::SessionId;
use crate::settings::AgentName;
use crate::ticket::TicketId;
use crate::work_item::{Timings, WorkItem};

impl Runner {
    // A work item on `kelpie/<ticket>` whose first turn is due, with nothing
    // recorded yet
    pub(super) fn fresh(
        &self,
        ticket: TicketId,
        title: String,
        agent: AgentName,
        session: SessionId,
    ) -> WorkItem {
        WorkItem {
            timings: Some(Timings::starting(self.ports.clock.now())),
            ..WorkItem::new(
                ticket,
                title,
                work_branch(ticket),
                self.paths.worktree(ticket),
                self.paths.build(ticket),
                agent,
                session,
            )
        }
    }
}
