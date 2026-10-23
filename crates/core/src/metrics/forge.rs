//! The forge calls kelpie makes, and the rate limits GitHub counts them in

use super::{Family, Recorder};
use crate::forge_ledger::Line;
use crate::ports::RateLimits;
use crate::ports::SeriesKind::{Counter, Gauge};

const CALLS: Family = Family {
    name: "kelpie_forge_calls_total",
    kind: Counter,
    help: "Forge calls through the ports, by API (rest or graphql), whose budget (login or app) and how each ended.",
};
const RATE_USED: Family = Family {
    name: "kelpie_forge_rate_used",
    kind: Gauge,
    help: "Calls GitHub has counted against the login's core or graphql limit since it last reset.",
};
const RATE_RESET: Family = Family {
    name: "kelpie_forge_rate_reset_timestamp_seconds",
    kind: Gauge,
    help: "When the login's core or graphql limit next resets, in Unix seconds.",
};

impl Recorder {
    /// Counts the forge call `line` records, unless GitHub counts it against no limit
    pub fn forge_call(&self, line: &Line) {
        if !line.kind.counts() {
            return;
        }
        let labels = vec![
            ("api", line.api.name()),
            ("by", line.by.name()),
            ("ended", line.ended.name()),
        ];
        self.add(&CALLS, labels, 1.0);
    }

    /// Sets the gauges of the login's `limits`
    pub fn rate_limits(&self, limits: &RateLimits) {
        for (bucket, limit) in [("core", limits.core), ("graphql", limits.graphql)] {
            let labels = || vec![("bucket", bucket.to_owned())];
            #[expect(
                clippy::cast_precision_loss,
                reason = "a count of calls and Unix seconds"
            )]
            {
                self.set(&RATE_USED, labels(), limit.used as f64);
                self.set(&RATE_RESET, labels(), limit.reset.0 as f64);
            }
        }
    }
}
