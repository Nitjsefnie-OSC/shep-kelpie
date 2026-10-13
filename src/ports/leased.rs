//! The leases a local agent holds for each whole call
//!
//! A local model's limit is the GPU it runs on, not an account, so a call
//! on one waits for its lease and keeps it until the call ends. A review
//! round on the same lock then waits behind a worker's turn.

use std::fmt;
use std::sync::Arc;

use serde::Serialize;

use super::{AgentCall, AgentError, AgentReply, Agents, CallActivity, Ending, Transcript, Wait};
use crate::forwarder::Upstream;
use crate::lease::gpu::{GpuHold, LockHolder};
use crate::settings::LeaseName;

/// Who holds a lease on a model, as the gateway's `GET /paddock/status` lists it
// wire format: `status.local_leases` shows it, so a rename breaks readers of status
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PaddockHolder {
    /// The lease's id, such as `L12`
    pub lease: String,
    /// The client that took it
    pub client: String,
    /// What it is for, from its note, or empty
    pub what: String,
    /// When it was taken, as the gateway says it
    pub since: String,
}

/// Who holds a lease a local agent's calls take, by where the lease lives
// wire format: `status.local_leases` shows it, so a change breaks readers of status
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum LeaseHolder {
    /// A lock of kelpie's own, such as the GPU lock
    Lock(LockHolder),
    /// A gateway's lease on a model
    Paddock(PaddockHolder),
}

/// Takes and reads the locks that leases name
pub trait LocalLeases: Send + Sync {
    /// Waits for `lease`, then holds it for `what` until the hold drops
    ///
    /// `None` when `ending` is asked while it waits, so a call past its
    /// ceiling leaves the queue rather than start only to be ended.
    ///
    /// # Errors
    ///
    /// [`AgentError::Stopped`] when the runner stops first, and
    /// [`AgentError::Setup`] when the lock cannot be taken.
    fn hold(
        &self,
        lease: &LeaseName,
        what: &str,
        ending: &Ending,
    ) -> Result<Option<GpuHold>, AgentError>;

    /// Who holds `lease`, or `None` when it is free
    fn holder(&self, lease: &LeaseName) -> Option<LockHolder>;

    /// Who holds a lease on `model` at `gateway`, or `None` when it is free
    /// or the gateway cannot be asked
    fn paddock_holder(&self, gateway: &Upstream, model: &str) -> Option<PaddockHolder>;
}

/// Agents whose calls hold the lease each names for as long as they run
pub struct Leased {
    agents: Arc<dyn Agents>,
    leases: Arc<dyn LocalLeases>,
}

impl Leased {
    /// `agents`, holding each call's lease from `leases`
    pub fn new(agents: Arc<dyn Agents>, leases: Arc<dyn LocalLeases>) -> Self {
        Self { agents, leases }
    }
}

impl fmt::Debug for Leased {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Leased").finish_non_exhaustive()
    }
}

impl Agents for Leased {
    fn prepare(&self, call: &AgentCall) -> Result<(), AgentError> {
        self.agents.prepare(call)
    }

    fn run(&self, call: &AgentCall, ending: &Ending) -> Result<AgentReply, AgentError> {
        let _held = match &call.lease {
            Some(lease) => {
                let what = format!(
                    "kelpie {} call for #{} in {}",
                    call.role.as_str(),
                    call.issue,
                    call.cwd.display()
                );
                ending.wait(Wait::Lease);
                let held = self.leases.hold(lease, &what, ending)?;
                let Some(held) = held else {
                    return Err(AgentError::TimedOut(call.harness.harness()));
                };
                ending.begin();
                Some(held)
            }
            None => None,
        };
        self.agents.run(call, ending)
    }

    fn transcript(&self, call: &AgentCall) -> Option<Transcript> {
        self.agents.transcript(call)
    }

    fn last_active(&self, call: &AgentCall) -> CallActivity {
        self.agents.last_active(call)
    }

    // The maintainer drives the session, so no lease of kelpie's holds it.
    fn foreground(&self, call: &AgentCall) -> Result<std::process::Command, AgentError> {
        self.agents.foreground(call)
    }
}
