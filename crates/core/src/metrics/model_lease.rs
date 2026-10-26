//! The leases on the gateways' models that this runner's calls wait for and hold, by model

use super::{Family, Recorder};
use crate::ports::SeriesKind::{Counter, Gauge};

const WAITING: Family = Family {
    name: "kelpie_model_lease_waiting",
    kind: Gauge,
    help: "Calls of this runner waiting for a lease on a gateway's model, by model.",
};
const HELD: Family = Family {
    name: "kelpie_model_lease_held",
    kind: Gauge,
    help: "Calls of this runner holding a lease on a gateway's model, by model.",
};
const WAIT_SECONDS: Family = Family {
    name: "kelpie_model_lease_wait_seconds_total",
    kind: Counter,
    help: "Seconds calls waited for a lease on a gateway's model, by model; over \
           kelpie_model_lease_grants_total for a mean.",
};
const GRANTS: Family = Family {
    name: "kelpie_model_lease_grants_total",
    kind: Counter,
    help: "Leases on a gateway's model granted to calls of this runner, by model.",
};

impl Recorder {
    /// Sets how many calls hold and how many wait for a lease on `model`
    pub fn model_lease_standing(&self, model: &str, held: usize, waiting: usize) {
        let labels = || vec![("model", model.to_owned())];
        self.set(&HELD, labels(), count(held));
        self.set(&WAITING, labels(), count(waiting));
    }

    /// Counts a lease on `model` granted after `waited` seconds
    pub fn model_lease_granted(&self, model: &str, waited: u64) {
        let labels = || vec![("model", model.to_owned())];
        self.add(&GRANTS, labels(), 1.0);
        #[expect(clippy::cast_precision_loss, reason = "seconds stay far below 2^53")]
        self.add(&WAIT_SECONDS, labels(), waited as f64);
    }
}

#[expect(clippy::cast_precision_loss, reason = "a count of calls")]
fn count(n: usize) -> f64 {
    n as f64
}
