//! Codex calls handed from one image of the runner to the next
//!
//! The call's `codex` process survives the `execve`, and so do its record,
//! output and thread folder. A Codex call holds no gateway lease and goes
//! through no forwarder, so the old image carries only the session id kelpie
//! gave it.

use std::path::{Path, PathBuf};

use super::{CODEX, CodexCli, Files, finish};
use crate::adapters::claude::ClaudeCli;
use crate::adapters::process::RunError;
use crate::ports::{Adopted, AgentCall, AgentError, AgentReply, Ending};
use crate::settings::Harness;

impl CodexCli {
    /// What finishes `call` on another image, if it runs now as a worker's
    /// call that left a record, or why nothing does
    pub(super) fn adoption_of(&self, call: &AgentCall) -> Result<Adopted, String> {
        let mut adopted = ClaudeCli::adoption_of(call)?;
        adopted.harness = Harness::Codex;
        adopted.session = Some(call.session.id().clone());
        Ok(adopted)
    }

    /// Finishes `adopted` as `run` finishes a call it started
    pub(super) fn adopt_call(
        &self,
        adopted: &Adopted,
        ending: &Ending,
    ) -> Result<AgentReply, AgentError> {
        let Some(id) = adopted.session.as_ref() else {
            return self.unserved(adopted, ending, "the carried Codex call names no session");
        };
        let output = self
            .processes
            .output_of_adopted(adopted, ending)
            .map_err(|e| match e {
                RunError::Io(e) => AgentError::Failed(CODEX, e.to_string()),
                RunError::Stopped => AgentError::Stopped,
                RunError::TimedOut => AgentError::TimedOut(CODEX),
            })?;
        finish(&Files::at(&settings_of(&adopted.record)), id, &output)
    }

    // A call that cannot be finished is ended and waited out, so no `codex`
    // is left running with no one to finish it.
    fn unserved(
        &self,
        adopted: &Adopted,
        ending: &Ending,
        why: &str,
    ) -> Result<AgentReply, AgentError> {
        ending.end();
        let _ = self.processes.output_of_adopted(adopted, ending);
        Err(AgentError::Setup(why.into()))
    }
}

// The settings path a call's record sits beside, which every other file of
// the call is named from: `<stem>.call.json` for the settings `<stem>.json`.
pub(super) fn settings_of(record: &Path) -> PathBuf {
    let name = record.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let stem = name.strip_suffix(".call.json").unwrap_or(name);
    record.with_file_name(format!("{stem}.json"))
}
