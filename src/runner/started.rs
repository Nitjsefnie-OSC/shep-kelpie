//! The `session-started` step, logged as each call starts
//!
//! A call's end names its session in a step, but its transcript fills while
//! it runs. This names the session at the start, with its work item, role and
//! agent and where its harness writes the transcript, so `shep kelpie watch`
//! can attribute a session by its id as it happens.

use super::{Runner, StepReport};
use crate::ports::{AgentCall, Role};

impl Runner {
    /// The `session-started` steps of the calls started since the last call,
    /// oldest first
    ///
    /// They are not a pass's result, since a call starts in the middle of one,
    /// so the runner logs them as they come.
    pub fn take_started(&mut self) -> Vec<StepReport> {
        std::mem::take(&mut self.started)
    }

    /// Records that `call`, on the agent named `agent`, is starting
    pub(super) fn call_started(&mut self, call: &AgentCall, agent: &str) {
        self.started.push(StepReport::SessionStarted {
            issue: (call.role != Role::Pm).then_some(call.issue),
            role: call.role,
            agent: agent.to_owned(),
            harness: call.harness.harness().as_str(),
            session: call.session.id().clone(),
            transcript: self.ports.agents.transcript(call),
        });
    }
}
