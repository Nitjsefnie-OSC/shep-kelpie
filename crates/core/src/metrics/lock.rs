//! How long the runner's lock is held, summed and counted by whether a hold lasted a second

use super::{Family, Recorder};
use crate::ports::SeriesKind::Counter;

const HELD: Family = Family {
    name: "kelpie_runner_lock_held_seconds_total",
    kind: Counter,
    help: "Seconds the runner's lock has been held, summed over every hold.",
};
// The channel has no histogram type, so the buckets are counters with an `le` label, as for the
// requests' context sizes.
const HOLDS: Family = Family {
    name: "kelpie_runner_lock_holds_total",
    kind: Counter,
    help: "Holds of the runner's lock by length in seconds, as cumulative buckets with an le label.",
};

impl Recorder {
    /// Counts `all` holds of the runner's lock that together lasted `seconds`, `within_one` of
    /// them no longer than a second
    pub fn lock_holds(&self, seconds: f64, within_one: u32, all: u32) {
        self.add(&HELD, Vec::new(), seconds);
        self.add(&HOLDS, vec![("le", "1".to_owned())], f64::from(within_one));
        self.add(&HOLDS, vec![("le", "+Inf".to_owned())], f64::from(all));
    }
}
