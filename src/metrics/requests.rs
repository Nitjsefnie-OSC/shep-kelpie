//! Each request a call made to its model, read from its transcript once it
//! ends: the sizes of their contexts, and what they wrote to the cache

use super::{Family, Label, Recorder};
use crate::ports::Role;
use crate::ports::SeriesKind::Counter;
use crate::request_use::{After, Request};

// The channel has no histogram type, so the buckets are counters with an `le` label, each counting
// the requests no larger than it, and `histogram_quantile` reads them over `rate`.
const CONTEXT: Family = Family {
    name: "kelpie_request_context_tokens",
    kind: Counter,
    help: "Requests to the model by context size in tokens, as cumulative buckets with an le label.",
};
const CACHE_WRITES: Family = Family {
    name: "kelpie_cache_write_tokens_total",
    kind: Counter,
    help: "Tokens requests wrote to the cache, by what came before: start, gap or growth.",
};

// The roles whose requests are counted. They are the long sessions, where a context grows and a
// cache write after a gap costs; the one-shot roles' calls are short. Every role would hold nine
// series for each harness it runs on, which the runner's 200 cannot hold beside the rest.
const ROLES: [Role; 3] = [Role::Worker, Role::Reviewer, Role::Pm];

// The buckets' upper bounds, in tokens, and then +Inf
const BOUNDS: [u64; 5] = [25_000, 50_000, 100_000, 150_000, 200_000];

impl Recorder {
    /// Counts `request`, made by a call of `role` on `harness`, if the role's requests are counted
    pub(crate) fn request(&self, role: &str, harness: &str, request: &Request) {
        if !ROLES.iter().any(|counted| counted.as_str() == role) {
            return;
        }
        let labels = |extra: Label| {
            let role = ("role", role.to_owned());
            vec![role, ("harness", harness.to_owned()), extra]
        };
        // Every bucket is sent with the first request, at 0 where it is not in, so a quantile
        // never reads a series that has only just appeared.
        for bound in BOUNDS {
            let within = request.context <= bound;
            self.add(
                &CONTEXT,
                labels(("le", bound.to_string())),
                f64::from(within),
            );
        }
        self.add(&CONTEXT, labels(("le", "+Inf".to_owned())), 1.0);
        for after in After::ALL {
            let written = if after == request.after {
                request.cache_write
            } else {
                0
            };
            #[expect(clippy::cast_precision_loss, reason = "a request's tokens")]
            self.add(
                &CACHE_WRITES,
                labels(("after", after.as_str().to_owned())),
                written as f64,
            );
        }
    }
}
