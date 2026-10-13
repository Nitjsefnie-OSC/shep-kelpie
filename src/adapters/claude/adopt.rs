//! Claude Code calls handed from one image of the runner to the next

use super::{CLAUDE, ClaudeCli, parse_result};
use crate::adapters::process::record::{CallFiles, Record};
use crate::adapters::process::{RunError, Sealed};
use crate::ports::{Adopted, AgentCall, AgentError, AgentReply, Ending, Role, Session};

impl ClaudeCli {
    /// Stops this runner's calls from ending, so a handover can carry them,
    /// or `None` when one is just ending
    pub fn seal(&self) -> Option<Sealed> {
        self.processes.seal()
    }

    /// What finishes `call` on another image, if it runs now as a worker's
    /// call that left a record, or why nothing does
    pub(super) fn adoption_of(call: &AgentCall) -> Result<Adopted, String> {
        if call.role != Role::Worker {
            return Err("only a worker's call is carried".into());
        }
        let record = CallFiles::of(call).record;
        let read = Record::read(&record)
            .map_err(|e| format!("the call's record {} cannot be read: {e}", record.display()))?;
        Ok(Adopted {
            pid: read.pid,
            record,
            flying: read.flying,
            resumes: match &call.session {
                Session::Resume(id) => Some(id.clone()),
                Session::New(_) => None,
            },
        })
    }

    /// Finishes `adopted` as `run` finishes a call it started
    pub(super) fn adopt_call(
        &self,
        adopted: &Adopted,
        ending: &Ending,
    ) -> Result<AgentReply, AgentError> {
        let output = self
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
