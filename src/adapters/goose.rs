//! goose, headless as `goose run`
//!
//! Only a stand-in for now: it refuses every call, so the places that branch
//! on the harness can route to goose before its adapter is reviewed.

use super::claude::ClaudeCli;
use crate::ports::{AgentCall, AgentError, AgentReply, Agents, Ending};

/// The refusal every call gets until the adapter is built
const NOT_BUILT: &str = "goose is not built yet: its adapter is #787";

/// goose's stand-in adapter, which refuses every call
#[derive(Debug, Clone, Copy, Default)]
pub struct GooseCli;

impl ClaudeCli {
    /// goose's adapter, which refuses every call until #787
    pub fn goose(&self) -> GooseCli {
        GooseCli
    }
}

impl Agents for GooseCli {
    fn prepare(&self, _: &AgentCall) -> Result<(), AgentError> {
        Err(AgentError::Setup(NOT_BUILT.into()))
    }

    fn run(&self, _: &AgentCall, _: &Ending) -> Result<AgentReply, AgentError> {
        Err(AgentError::Setup(NOT_BUILT.into()))
    }
}

#[cfg(test)]
mod tests;
