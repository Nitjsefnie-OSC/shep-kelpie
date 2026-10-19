//! A work item opened for an issue, before its first turn

use super::Runner;
use super::paths::work_branch;
use crate::ports::{ForgeError, SessionId};
use crate::settings::AgentName;
use crate::ticket::TicketId;
use crate::work_item::{Timings, WorkItem};

impl Runner {
    // A work item on `kelpie/<ticket>` whose first turn is due, with nothing
    // recorded yet
    pub(super) fn fresh(
        &self,
        issue: u64,
        title: String,
        agent: AgentName,
        session: SessionId,
    ) -> Result<WorkItem, ForgeError> {
        // The forge read the issue, and none numbers one zero.
        let ticket = TicketId::from_issue(issue)
            .ok_or_else(|| ForgeError::Unreadable(format!("{issue} is not an issue number")))?;
        Ok(WorkItem {
            timings: Some(Timings::starting(self.ports.clock.now())),
            ..WorkItem::new(
                issue,
                title,
                work_branch(ticket),
                self.paths.worktree(ticket),
                self.paths.build(ticket),
                agent,
                session,
            )
        })
    }
}
