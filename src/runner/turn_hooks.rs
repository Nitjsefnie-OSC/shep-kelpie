//! The project's `turn_start` and `reset` hooks, run on the thread of the
//! call they come before
//!
//! A call's thread wraps its agent port in [`TurnHooks`], so the hooks reach
//! every prompt kelpie sends a worker, a reviewer or the project manager, from
//! the one place they all pass. A `/compact` is a reset, not a turn: it tells
//! the `reset` hooks, which are notices, and runs. Any other prompt must pass
//! the `turn_start` hooks, and one they refuse is never sent: the call fails
//! naming the hook, as a call that could not start does, which parks a worker
//! on a ruling.

use std::sync::Arc;

use serde_json::{Value, json};

use super::claim::Notes;
use super::points::phase_name;
use crate::hooks::{Hooks, Role};
use crate::ports::{
    AgentCall, AgentError, AgentReply, Agents, COMPACT, CallActivity, Ending, Session, SessionId,
};
use crate::ports::{Role as Recipient, Transcript};
use crate::settings::HookPoint;
use crate::work_item::WorkItem;

/// Agents whose calls first run the project's `turn_start` and `reset` hooks
pub(super) struct TurnHooks {
    agents: Arc<dyn Agents>,
    hooks: Hooks,
    notes: Notes,
    // The phase the work item is in as the call starts, none for the project manager's
    phase: Option<String>,
}

impl TurnHooks {
    /// `agents`, wrapped when the project has a hook at either point
    ///
    /// `item` is the work item the call is made for, if it is one's.
    pub(super) fn around(
        agents: Arc<dyn Agents>,
        hooks: &Hooks,
        notes: &Notes,
        item: Option<&WorkItem>,
    ) -> Arc<dyn Agents> {
        if !hooks.has(HookPoint::TurnStart) && !hooks.has(HookPoint::Reset) {
            return agents;
        }
        Arc::new(Self {
            agents,
            hooks: hooks.clone(),
            notes: notes.clone(),
            phase: item.map(|item| phase_name(&item.phase)),
        })
    }

    // The payload fields every call shares.
    fn details(&self, call: &AgentCall) -> Value {
        let issue = (call.role != Recipient::Pm).then_some(call.issue);
        json!({
            "issue": issue,
            "phase": self.phase,
            "recipient": call.role.as_str(),
            "session": match call.session {
                Session::New(_) => "new",
                Session::Resume(_) => "resumed",
            },
        })
    }

    // Whose work the hook judges: a worker's, or kelpie's own prompt to another role.
    fn role(call: &AgentCall) -> Role {
        match call.role {
            Recipient::Worker => Role::Worker,
            _ => Role::Kelpie,
        }
    }

    // Where the hook runs: a worker's worktree, else the project's checkout.
    fn folder(call: &AgentCall) -> Option<&std::path::Path> {
        (call.role == Recipient::Worker).then_some(call.cwd.as_path())
    }
}

impl std::fmt::Debug for TurnHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TurnHooks").finish_non_exhaustive()
    }
}

impl Agents for TurnHooks {
    fn prepare(&self, call: &AgentCall) -> Result<(), AgentError> {
        self.agents.prepare(call)
    }

    fn run(&self, call: &AgentCall, ending: &Ending) -> Result<AgentReply, AgentError> {
        let mut details = self.details(call);
        if call.prompt == COMPACT {
            // A compaction keeps no handoff of kelpie's: the model writes its own summary.
            details["kind"] = "compact".into();
            details["handoff"] = Value::Null;
            let told = self.hooks.notify(
                HookPoint::Reset,
                Self::role(call),
                Self::folder(call),
                details,
            );
            if let Some(line) = told {
                self.notes.push(line);
            }
        } else if self.hooks.has(HookPoint::TurnStart) {
            details["prompt"] = call.prompt.clone().into();
            let checked = self.hooks.check(
                HookPoint::TurnStart,
                Self::role(call),
                Self::folder(call),
                details,
            );
            if let Err(refusal) = checked {
                return Err(AgentError::Setup(format!(
                    "{refusal}, so kelpie sent nothing to the {}",
                    call.role.as_str()
                )));
            }
        }
        self.agents.run(call, ending)
    }

    fn transcript(&self, call: &AgentCall) -> Option<Transcript> {
        self.agents.transcript(call)
    }

    fn last_active(&self, call: &AgentCall) -> CallActivity {
        self.agents.last_active(call)
    }

    fn foreground(&self, call: &AgentCall) -> Result<std::process::Command, AgentError> {
        self.agents.foreground(call)
    }

    fn foreground_ended(&self, session: &SessionId) {
        self.agents.foreground_ended(session);
    }
}
