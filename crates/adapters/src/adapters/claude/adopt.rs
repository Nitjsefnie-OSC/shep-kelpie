//! Claude Code calls handed from one image of the runner to the next

use super::{CLAUDE, ClaudeCli, parse_result};
use crate::adapters::process::RunError;
use crate::ports::{Adopted, AgentError, AgentReply, Ending, Session};

impl ClaudeCli {
    /// Finishes `adopted` as `run` finishes a call it started
    pub(super) fn adopt_call(
        &self,
        adopted: &Adopted,
        ending: &Ending,
    ) -> Result<AgentReply, AgentError> {
        let output = self
            .base
            .processes
            .output_of_adopted(adopted, ending)
            .map_err(|e| match e {
                RunError::Io(e) => AgentError::Failed(CLAUDE, e.to_string()),
                RunError::Stopped => AgentError::Stopped,
                RunError::TimedOut => AgentError::TimedOut(CLAUDE),
            })?;
        let session = match &adopted.resumes {
            Some(id) => Session::Resume(id.clone()),
            None => Session::New(crate::ports::SessionId(String::new())),
        };
        parse_result(&output, &session)
    }
}
