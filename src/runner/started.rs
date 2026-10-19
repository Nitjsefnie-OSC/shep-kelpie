//! The `session-started` step, logged as each call starts
//!
//! A call's end names its session in a step, but its transcript fills while
//! it runs. This names the session at the start, with its work item, role and
//! agent and where its harness writes the transcript, so `shep kelpie watch`
//! can attribute a session by its id as it happens.

use super::{Runner, StepReport};
use crate::ports::{AgentCall, Role};
use crate::ticket::TicketId;

impl Runner {
    /// The steps of what happened in the middle of a pass since the last call,
    /// oldest first: the `session-started` of each call started, and a
    /// `pull-request-not-opened` of a turn's end
    ///
    /// They are not a pass's result, since a call starts in the middle of one,
    /// so the runner logs them as they come.
    pub fn take_started(&mut self) -> Vec<StepReport> {
        std::mem::take(&mut self.started)
    }

    /// Records that `call`, on the agent named `agent`, is starting
    pub(super) fn call_started(&mut self, call: &AgentCall, agent: &str) {
        self.started.push(StepReport::SessionStarted {
            issue: (call.role != Role::Pm)
                .then(|| TicketId::from_issue(call.issue))
                .flatten(),
            role: call.role,
            agent: agent.to_owned(),
            harness: call.harness.harness().as_str(),
            session: call.session.id().clone(),
            transcript: self.ports.agents.transcript(call),
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::*;
    use crate::ports::{Session, SessionId, Tools};
    use crate::settings::{AgentHarness, Effort};
    use crate::test::Rig;

    // An expert's call as `expert_call` makes it: for a ruling with no issue,
    // which is how an advisor or a researcher votes on an adopt-failed ruling,
    // `issue` is 0.
    fn expert(issue: u64) -> AgentCall {
        AgentCall {
            issue,
            ..AgentCall::new(
                Role::Advisor,
                Tools::Advise,
                AgentHarness::ClaudeCode,
                "sonnet-high",
                Effort::Low,
                Session::New(SessionId("s-7".into())),
                PathBuf::from("/work"),
                "Vote.",
            )
        }
    }

    #[test]
    fn a_panel_call_for_a_ruling_with_no_issue_is_logged_with_no_work_item() {
        let rig = Rig::new("koji");
        let runner = rig.open().unwrap();
        runner.lock().unwrap().call_started(&expert(0), "advisor");
        let [step] = runner.lock().unwrap().take_started().try_into().unwrap();
        let StepReport::SessionStarted {
            issue, role, agent, ..
        } = step.clone()
        else {
            panic!("the step is a session-started: {step:?}")
        };
        assert_eq!(
            (issue, role, agent.as_str()),
            (None, Role::Advisor, "advisor"),
            "{step:?}"
        );
        assert_eq!(
            serde_json::to_value(&step).unwrap()["issue"],
            json!(null),
            "no work item is named: {step:?}"
        );
        // An expert's call for a ruling that has an issue keeps naming its ticket.
        runner.lock().unwrap().call_started(&expert(7), "advisor");
        let [step] = runner.lock().unwrap().take_started().try_into().unwrap();
        assert_eq!(serde_json::to_value(&step).unwrap()["issue"], json!("7"));
    }
}
