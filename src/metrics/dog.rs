//! The dog's leases, review bot summons and forge hold
//!
//! Sent by the dog through the same [`Recorder`] a runner uses. The shepherd's
//! metrics dog labels them `dog` and `source` where a runner's carry `sheep`.

use super::{Family, Recorder};
use crate::ports::SeriesKind::{Counter, Gauge};

const LEASES_HELD: Family = Family {
    name: "kelpie_leases_held",
    kind: Gauge,
    help: "Commands and runners holding the lease, by kind.",
};
const LEASE_WAITING: Family = Family {
    name: "kelpie_lease_waiting",
    kind: Gauge,
    help: "Commands and runners waiting for the lease, by kind.",
};
const LEASE_GRANTS: Family = Family {
    name: "kelpie_lease_grants_total",
    kind: Counter,
    help: "Times the lease was granted, by kind.",
};
const LEASE_WAIT_SECONDS: Family = Family {
    name: "kelpie_lease_wait_seconds_total",
    kind: Counter,
    help: "Seconds grants waited in the queue; over kelpie_lease_grants_total for a mean.",
};
const SUMMONS: Family = Family {
    name: "kelpie_summons_total",
    kind: Counter,
    help: "Review bot summons runners reported, by bot and whether it accepted or refused.",
};
const FORGE_HOLDS: Family = Family {
    name: "kelpie_forge_holds_total",
    kind: Counter,
    help: "Times the forge's rate limit began holding every runner's calls, by kind of login.",
};
const FORGE_HELD: Family = Family {
    name: "kelpie_forge_held",
    kind: Gauge,
    help: "1 while the forge's rate limit holds every runner's calls, 0 when not.",
};

/// The only kind of login the dog's forge hold is for: the maintainer's `gh`
/// login. The App's budget is its own, and its used-up answer holds nothing
/// the dog keeps.
const LOGIN: &str = "login";

impl Recorder {
    /// Sets how many hold and how many wait for the lease `kind`
    pub fn lease_standing(&self, kind: &str, held: usize, waiting: usize) {
        let labels = || vec![("kind", kind.to_owned())];
        self.set(&LEASES_HELD, labels(), count(held));
        self.set(&LEASE_WAITING, labels(), count(waiting));
    }

    /// Counts the lease `kind` granted after `waited` seconds in the queue
    pub fn lease_granted(&self, kind: &str, waited: u64) {
        let labels = || vec![("kind", kind.to_owned())];
        self.add(&LEASE_GRANTS, labels(), 1.0);
        #[expect(clippy::cast_precision_loss, reason = "seconds stay far below 2^53")]
        self.add(&LEASE_WAIT_SECONDS, labels(), waited as f64);
    }

    /// Counts a summon of review bot `bot` that it `accepted` or refused
    pub fn summon(&self, bot: &str, accepted: bool) {
        let outcome = if accepted { "accepted" } else { "refused" };
        let labels = vec![("bot", bot.to_owned()), ("outcome", outcome.to_owned())];
        self.add(&SUMMONS, labels, 1.0);
    }

    /// Counts a forge hold that began
    pub fn forge_hold(&self) {
        self.add(&FORGE_HOLDS, vec![("login_kind", LOGIN.to_owned())], 1.0);
    }

    /// Sets whether the forge is held
    pub fn forge_held(&self, held: bool) {
        self.set(&FORGE_HELD, Vec::new(), f64::from(u8::from(held)));
    }
}

#[expect(clippy::cast_precision_loss, reason = "a count of holders")]
fn count(n: usize) -> f64 {
    n as f64
}
