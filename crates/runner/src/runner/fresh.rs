//! A work item opened for an issue, before its first turn

use super::Runner;
use crate::home::work_branch;
use crate::ports::SessionId;
use crate::settings::AgentName;
use crate::state::ProjectState;
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
            // Cut for the base it opens on, which a later `git.base` leaves alone.
            base: Some(self.settings.git.base().to_owned()),
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

impl Runner {
    // The base `item` was cut for: the one it recorded, else the runner's
    pub(super) fn item_base<'a>(&'a self, item: &'a WorkItem) -> &'a str {
        item.base.as_deref().unwrap_or(self.settings.git.base())
    }
}

// Records `base` on the work items a file from before they recorded one
// holds, and says whether any changed
pub(super) fn record_bases(state: &mut ProjectState, base: &str) -> bool {
    let unrecorded = state
        .work_items
        .iter_mut()
        .filter(|item| item.base.is_none());
    unrecorded.fold(false, |_, item| {
        item.base = Some(base.to_owned());
        true
    })
}
