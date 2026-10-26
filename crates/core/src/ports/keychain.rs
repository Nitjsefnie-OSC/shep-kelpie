//! Calls on a repo kelpie's App covers, run with the maintainer's keychain out of reach
//!
//! The sandbox can only cut the keychain's services out of its profile for a
//! whole call. A repo the App does not cover keeps them: its worker pushes and
//! runs `gh` with the login the keychain holds, so blocking it there breaks the
//! push and protects nothing.

use std::fmt;
use std::sync::Arc;

use super::{
    Adopted, AgentCall, AgentError, AgentReply, Agents, CallActivity, Ending, NotAdopted,
    SessionId, Transcript, Voice,
};
use crate::settings::ForgeSlug;

/// Agents whose calls block the keychain while an App covers `repo`
pub struct KeychainBlocked {
    agents: Arc<dyn Agents>,
    voice: Arc<dyn Voice>,
    repo: ForgeSlug,
}

impl KeychainBlocked {
    /// `agents`, with each call's sandbox blocking the keychain while `voice` covers `repo`
    pub fn new(agents: Arc<dyn Agents>, voice: Arc<dyn Voice>, repo: ForgeSlug) -> Self {
        Self {
            agents,
            voice,
            repo,
        }
    }

    // Asked again for each call, so an App installed on the repo takes effect
    // at the next call, and one removed lets the next push use the login again.
    fn blocking(&self, call: &AgentCall) -> AgentCall {
        let mut call = call.clone();
        call.reach.block_keychain = self.voice.covers(&self.repo);
        call
    }
}

impl fmt::Debug for KeychainBlocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeychainBlocked")
            .field("repo", &self.repo)
            .finish_non_exhaustive()
    }
}

impl Agents for KeychainBlocked {
    fn prepare(&self, call: &AgentCall) -> Result<(), AgentError> {
        self.agents.prepare(&self.blocking(call))
    }

    fn run(&self, call: &AgentCall, ending: &Ending) -> Result<AgentReply, AgentError> {
        self.agents.run(&self.blocking(call), ending)
    }

    fn adoption(&self, call: &AgentCall) -> Result<Adopted, NotAdopted> {
        self.agents.adoption(call)
    }

    fn adopt(&self, adopted: &Adopted, ending: &Ending) -> Result<AgentReply, AgentError> {
        self.agents.adopt(adopted, ending)
    }

    fn transcript(&self, call: &AgentCall) -> Option<Transcript> {
        self.agents.transcript(call)
    }

    fn last_active(&self, call: &AgentCall) -> CallActivity {
        self.agents.last_active(call)
    }

    fn foreground(&self, call: &AgentCall) -> Result<std::process::Command, AgentError> {
        self.agents.foreground(&self.blocking(call))
    }

    fn foreground_ended(&self, session: &SessionId) {
        self.agents.foreground_ended(session);
    }
}
