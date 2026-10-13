//! How many conditional reads GitHub answered 304 Not Modified

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// A tally the forge's adapters add to as a conditional read comes back 304,
/// and [`RateHeld`](super::RateHeld) takes from after each call to put in
/// the ledger
///
/// Clones share one tally. A call that overlaps another takes the other's
/// count, so a line of the ledger may carry a neighbour's 304, and the total
/// is exact.
#[derive(Debug, Clone, Default)]
pub struct Revalidated(Arc<AtomicU32>);

impl Revalidated {
    /// Adds one read that came back 304
    pub fn note(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }

    /// The reads noted since the last take, which starts the count over
    pub fn take(&self) -> u32 {
        self.0.swap(0, Ordering::Relaxed)
    }
}
