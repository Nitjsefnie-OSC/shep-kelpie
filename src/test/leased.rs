//! The gateway's leases, answered with nothing, and counted

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::forwarder::Upstream;
use crate::ports::{LocalLeases, PaddockHolder};

/// A gateway whose lease reads answer that nothing is held, counted so a test
/// can say who asked
#[derive(Debug, Default)]
pub(crate) struct LeaseReads {
    reads: AtomicUsize,
}

impl LeaseReads {
    /// How many lease reads reached the gateway
    pub(crate) fn count(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

impl LocalLeases for LeaseReads {
    fn paddock_holder(&self, _gateway: &Upstream, _model: &str) -> Option<PaddockHolder> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        None
    }
}
