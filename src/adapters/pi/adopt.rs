//! pi calls handed from one image of the runner to the next
//!
//! The call's `pi` process survives the `execve`, and so do its record and
//! output files. Two things it uses do not, since the runner's memory held
//! them: the forwarder on the socket in the call's folder, and the gateway
//! lease. The old image carries where they were, and the new one opens the
//! forwarder again at that path and goes on renewing the lease, from its own
//! gateway settings, and holds both until the adopted call ends.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::{PI, PiCli, parse_result};
use crate::adapters::claude::ClaudeCli;
use crate::adapters::forwarder::Forwarder;
use crate::adapters::process::RunError;
use crate::adapters::process::record::CallFiles;
use crate::ports::{Adopted, AgentCall, AgentError, AgentReply, Ending, Kept, KeptServer, Unheld};
use crate::settings::{EndpointUrl, GatewayName, Harness, ModelHost};

/// What running calls keep in this image's memory, by the call's record
#[derive(Debug, Clone, Default)]
pub(super) struct Carried(Arc<Mutex<HashMap<PathBuf, Kept>>>);

impl Carried {
    fn lock(&self) -> MutexGuard<'_, HashMap<PathBuf, Kept>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Notes what `call` keeps, until the returned guard is dropped
    pub(super) fn note(&self, call: &AgentCall, kept: Kept) -> Noted {
        let record = CallFiles::of(call).record;
        self.lock().insert(record.clone(), kept);
        Noted {
            carried: self.clone(),
            record,
        }
    }
}

/// A call's note in [`Carried`], which its end removes
pub(super) struct Noted {
    carried: Carried,
    record: PathBuf,
}

impl Drop for Noted {
    fn drop(&mut self) {
        self.carried.lock().remove(&self.record);
    }
}

/// How a call's model server is written in a [`Kept`]
pub(super) fn kept_server(host: &ModelHost) -> KeptServer {
    match host {
        ModelHost::Url(url) => KeptServer::Url(url.as_str().to_owned()),
        ModelHost::Gateway(name) => KeptServer::Gateway(name.to_string()),
    }
}

fn host_of(server: &KeptServer) -> Result<ModelHost, String> {
    let unusable =
        |what: &str, e: &str| format!("the carried model server {what} is unusable: {e}");
    match server {
        KeptServer::Url(url) => EndpointUrl::try_from(url.clone())
            .map(ModelHost::Url)
            .map_err(|e| unusable("URL", e)),
        KeptServer::Gateway(name) => GatewayName::try_from(name.clone())
            .map(ModelHost::Gateway)
            .map_err(|e| unusable("gateway", e)),
    }
}

impl PiCli {
    /// What finishes `call` on another image, if it runs now as a worker's
    /// call that left a record, or why nothing does
    pub(super) fn adoption_of(&self, call: &AgentCall) -> Result<Adopted, String> {
        let mut adopted = ClaudeCli::adoption_of(call)?;
        let kept = self.carried.lock().get(&adopted.record).cloned();
        adopted.kept = Some(kept.ok_or("this image holds no forwarder for the call")?);
        adopted.harness = Harness::Pi;
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
        let output = match (&kept.lease, &host) {
            (Some(id), ModelHost::Gateway(name)) => {
                let port = self.gateways.port_named(name).map_err(AgentError::Setup)?;
                port.around_resumed(&upstream, id, ending, wait)
                    .map_err(|unheld| match unheld {
                        Unheld::GaveUp => AgentError::TimedOut(PI),
                        Unheld::Refused(why) => AgentError::Setup(format!(
                            "cannot resume the lease {id} on gateway {name}: {why}"
                        )),
                        Unheld::Lost(why) => AgentError::Failed(PI, why),
                    })?
            }
            _ => wait(),
        }
        .map_err(|e| match e {
            RunError::Io(e) => AgentError::Failed(PI, e.to_string()),
            RunError::Stopped => AgentError::Stopped,
            RunError::TimedOut => AgentError::TimedOut(PI),
        })?;
        parse_result(&output, &kept.session)
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
        let _ = self.processes.output_of_adopted(adopted, ending);
        Err(AgentError::Setup(why))
    }
}
