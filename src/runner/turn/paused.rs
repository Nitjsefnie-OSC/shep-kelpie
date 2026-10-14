//! A worker turn that ended at a tool result because a pause was set
//!
//! The turn's hook (Claude Code's `PostToolUse`, pi's guard extension) leaves
//! a marker in the work item's build folder when it ends the turn. Such a turn
//! is not a finished one: the unpushed check, the handover and review wait for
//! the turn it resumes into, nothing is raised for a ruling, and the work item
//! keeps its phase and stage. The next start resumes the session with
//! [`PAUSED`] and a fresh turn ceiling, since a turn queued to send begins at
//! the time it starts.

use crate::guard::Pause;
use crate::ports::{AgentError, AgentReply, Role};
use crate::runner::Runner;
use crate::state::StateError;
use crate::work_item::Turn;

use crate::runner::report::StepReport;

/// The prompt for a turn resumed after a pause ended it at a tool call
pub(in crate::runner) const PAUSED: &str = "Kelpie paused your last turn after its last tool call. \
                                            Carry on with the work item from where you left off.";

impl Runner {
    // Whether the turn of `issue` that came back ended at a tool result for a pause
    pub(in crate::runner) fn paused_at_tool(&self, issue: u64) -> bool {
        (self.state.item(issue)).is_some_and(|item| Pause::in_build(&item.build).ended_turn())
    }

    // Ends the current work item's turn as paused, when it ended that way, and
    // answers `None` otherwise. A turn past its ceiling or stopped is not a
    // pause, whatever the hook left.
    pub(in crate::runner) fn end_paused(
        &mut self,
        result: &Result<AgentReply, AgentError>,
    ) -> Option<Result<Option<StepReport>, StateError>> {
        let issue = self.current()?.issue;
        if !self.paused_at_tool(issue) {
            return None;
        }
        let pause = Pause::in_build(&self.current()?.build);
        pause.clear();
        if matches!(result, Err(AgentError::Stopped | AgentError::TimedOut(_))) {
            return None;
        }
        let now = self.ports.clock.now();
        let mut next = self.state.clone();
        let item = self.current_in(&mut next)?;
        if let Ok(reply) = result {
            let session = item.session.clone();
            item.record_call(Role::Worker, now, session, reply.usage, reply.session_cost);
            item.context = reply.context;
        }
        item.turn = Turn::Next {
            prompt: PAUSED.to_owned(),
        };
        Some(self.save(next).map(|()| None))
    }
}
