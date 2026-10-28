//! The leases a gateway holds on local models, as `status` shows them

use serde::{Deserialize, Serialize};

use crate::forwarder::Upstream;

/// Who holds a lease on a model, as the gateway's `GET /paddock/status` lists it
// wire format: `status.local_leases` shows it, so a rename breaks readers of status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// Reads the leases on a gateway's models
pub trait LocalLeases: Send + Sync {
    /// Who holds a lease on `model` at `gateway`, or `None` when it is free
    /// or the gateway cannot be asked
    fn paddock_holder(&self, gateway: &Upstream, model: &str) -> Option<PaddockHolder>;

    /// Whether all lease turns are held and a waiter queues on `model`
    ///
    /// An unreadable status or a model with no turn limit counts as free.
    fn paddock_queued(&self, _gateway: &Upstream, _model: &str) -> bool {
        false
    }
}
