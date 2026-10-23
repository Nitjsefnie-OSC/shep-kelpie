//! Where a ruling on a check stuck queued sends its item

use crate::ports::Timestamp;
use crate::work_item::{Phase, Review};

/// The wait the ruling interrupted, started afresh: a rerun's checks read as
/// queued for a moment, and a no leaves the forge to the maintainer
pub(super) fn waiting_again(review: Option<Review>, now: Timestamp) -> Phase {
    match review {
        Some(mut review) => {
            if let Some(ci) = review.ci.as_deref_mut() {
                ci.since = now;
            }
            Phase::Review(review)
        }
        None => Phase::Ci {
            head: None,
            since: now,
        },
    }
}
