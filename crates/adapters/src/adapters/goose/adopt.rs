//! goose calls handed from one image of the runner to the next
//!
//! The call's `goose` process survives the `execve`, and so do its record,
//! output files and root. Two things it uses do not, since the runner's memory
//! held them: the forwarder on the socket in the call's folder, and the
//! gateway lease. The old image carries where they were, and the new one opens
//! the forwarder again at that path and goes on renewing the lease, from its
//! own gateway settings, and holds both until the adopted call ends.
//!
//! Nothing else is in memory. The worker's fence plugin and its `config.yaml`
//! are files in the call's root ([`super::hooks`] writes them in `prepare`),
//! and so are the session's token totals ([`Files::keep`]), which the new
//! image reads to count what the call spent. goose has no thread record of its
//! own to find: the session is named by the id kelpie gave it.

use super::compact;
use super::files::Files;
use super::{GOOSE, GooseCli};
use crate::adapters::claude::ClaudeCli;
use crate::adapters::forwarder::Forwarder;
use crate::adapters::pi::adopt::host_of;
use crate::adapters::process::RunError;
use crate::ports::{Adopted, AgentCall, AgentError, AgentReply, Ending, Session, Unheld};
use crate::settings::{Harness, ModelHost};

impl GooseCli {
    /// What finishes `call` on another image, if it runs now as a worker's
    /// call that left a record, or why nothing does
    pub(super) fn adoption_of(&self, call: &AgentCall) -> Result<Adopted, String> {
        let mut adopted = ClaudeCli::adoption_of(call)?;
        let kept = self.carried.kept_by(&adopted.record);
        adopted.kept = Some(kept.ok_or("this image holds no forwarder for the call")?);
        adopted.harness = Harness::Goose;
        Ok(adopted)
    }

    /// Finishes `adopted` as `run` finishes a call it started: with its
    /// forwarder open again, and its lease, if it held one, renewed
    pub(super) fn adopt_call(
        &self,
        adopted: &Adopted,
        ending: &Ending,
    ) -> Result<AgentReply, AgentError> {
        let wait = || self.processes.output_of_adopted(adopted, ending);
        let Some(kept) = adopted.kept.as_ref() else {
            return self.unserved(
                adopted,
                ending,
                "the carried goose call names no forwarder".into(),
            );
        };
        let files = Files::beside(&adopted.record);
        // The totals the call began with, which `run` kept before it spawned.
        // A file that is gone is no reason to end a turn that is running: the
        // session counts from nothing, as a new one does.
        let before = match files.spent(&kept.session) {
            Ok(before) => before.unwrap_or_default(),
            Err(e) => return self.unserved(adopted, ending, e.to_string()),
        };
        let served = host_of(&kept.server).and_then(|host| {
            let upstream = self.gateways.upstream(&host)?;
            let leased = match &kept.lease {
                Some(id) => upstream.clone().with_lease(id),
                None => upstream.clone(),
            };
            let forwarder = Forwarder::reopen(&kept.forwarder, leased)?;
            Ok((host, upstream, forwarder))
        });
        // The forwarder stays until the call has ended.
        let (host, upstream, _forwarder) = match served {
            Ok(served) => served,
            Err(why) => return self.unserved(adopted, ending, why),
        };
        let waited = match (&kept.lease, &host) {
            (Some(id), ModelHost::Gateway(name)) => {
                let port = match crate::adapters::port_named(&self.gateways, name) {
                    Ok(port) => port,
                    Err(why) => return self.unserved(adopted, ending, why),
                };
                match port.around_resumed(&upstream, id, ending, wait) {
                    Ok(waited) => waited,
                    Err(Unheld::Refused(why)) => {
                        let why = format!("cannot resume the lease {id} on gateway {name}: {why}");
                        return self.unserved(adopted, ending, why);
                    }
                    Err(Unheld::GaveUp) => return Err(AgentError::TimedOut(GOOSE)),
                    Err(Unheld::Lost(why)) => return Err(AgentError::Failed(GOOSE, why)),
                }
            }
            _ => wait(),
        };
        let output = waited.map_err(|e| match e {
            RunError::Io(e) => AgentError::Failed(GOOSE, e.to_string()),
            RunError::Stopped => AgentError::Stopped,
            RunError::TimedOut => AgentError::TimedOut(GOOSE),
        })?;
        let session = match &adopted.resumes {
            Some(id) => Session::Resume(id.clone()),
            None => Session::New(kept.session.clone()),
        };
        let compacting = adopted.flying.as_ref().is_some_and(|f| f.compact);
        let read = compact::read(compacting, &output, &session, before);
        // The session's totals grow whatever the call's reply was.
        if let Some(spent) = read.spent {
            files.keep(&kept.session, spent)?;
        }
        read.reply
    }

    // A call that cannot be served is ended and waited out, so no `goose`
    // is left running with no one to finish it.
    fn unserved(
        &self,
        adopted: &Adopted,
        ending: &Ending,
        why: String,
    ) -> Result<AgentReply, AgentError> {
        ending.end();
        let _ = self.processes.output_of_adopted(adopted, ending);
        Err(AgentError::Setup(why))
    }
}
