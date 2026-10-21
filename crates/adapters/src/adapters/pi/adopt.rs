//! pi calls handed from one image of the runner to the next
//!
//! The call's `pi` process survives the `execve`, and so do its record and
//! output files. Two things it uses do not, since the runner's memory held
//! them: the forwarder on the socket in the call's folder, and the gateway
//! lease. The old image carries where they were, and the new one opens the
//! forwarder again at that path and goes on renewing the lease, from its own
//! gateway settings, and holds both until the adopted call ends.

use super::{PI, PiCli, read_as};
use crate::adapters::forwarder::Forwarder;
use crate::adapters::harness::{adoption_of, host_of};
use crate::adapters::process::RunError;
use crate::ports::{Adopted, AgentCall, AgentError, AgentReply, Ending, Unheld};
use crate::settings::{Harness, ModelHost};

impl PiCli {
    /// What finishes `call` on another image, if it runs now as a worker's
    /// call that left a record, or why nothing does
    pub(super) fn adoption_of(&self, call: &AgentCall) -> Result<Adopted, String> {
        let mut adopted = adoption_of(call, Harness::Pi)?;
        let kept = self.carried.kept_by(&adopted.record);
        adopted.kept = Some(kept.ok_or("this image holds no forwarder for the call")?);
        Ok(adopted)
    }

    /// Finishes `adopted` as `run` finishes a call it started: with its
    /// forwarder open again, and its lease, if it held one, renewed
    pub(super) fn adopt_call(
        &self,
        adopted: &Adopted,
        ending: &Ending,
    ) -> Result<AgentReply, AgentError> {
        let wait = || self.base.processes.output_of_adopted(adopted, ending);
        let Some(kept) = adopted.kept.as_ref() else {
            return self.unserved(
                adopted,
                ending,
                "the carried pi call names no forwarder".into(),
            );
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
                    Err(Unheld::GaveUp) => return Err(AgentError::TimedOut(PI)),
                    Err(Unheld::Lost(why)) => return Err(AgentError::Failed(PI, why)),
                }
            }
            _ => wait(),
        };
        let output = waited.map_err(|e| match e {
            RunError::Io(e) => AgentError::Failed(PI, e.to_string()),
            RunError::Stopped => AgentError::Stopped,
            RunError::TimedOut => AgentError::TimedOut(PI),
        })?;
        let compacting = adopted.flying.as_ref().is_some_and(|f| f.compact);
        read_as(compacting, &output, &kept.session)
    }

    // A call that cannot be served is ended and waited out, so no `pi`
    // is left running with no one to finish it.
    fn unserved(
        &self,
        adopted: &Adopted,
        ending: &Ending,
        why: String,
    ) -> Result<AgentReply, AgentError> {
        ending.end();
        let _ = self.base.processes.output_of_adopted(adopted, ending);
        Err(AgentError::Setup(why))
    }
}
