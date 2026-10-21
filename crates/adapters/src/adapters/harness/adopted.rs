//! A worker's call that left a record, as a handover finds it

use crate::adapters::process::record::{CallFiles, Record};
use crate::ports::{Adopted, AgentCall, Role, Session};
use crate::settings::Harness;

/// What finishes `call` on another image, if it runs now as a worker's call
/// that left a record, or why nothing does
///
/// # Errors
///
/// A message when the call is not a worker's, or its record cannot be read.
pub(crate) fn adoption_of(call: &AgentCall, harness: Harness) -> Result<Adopted, String> {
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
        harness,
        kept: None,
        session: None,
    })
}
