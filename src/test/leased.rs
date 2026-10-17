//! The gateway's leases, answered with nothing or with one holder, and counted

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::forwarder::Upstream;
use crate::ports::{LocalLeases, PaddockHolder};

/// A gateway whose lease reads answer that nothing is held, or one holder once
/// a test names one, counted so a test can say who asked
#[derive(Debug, Default)]
pub(crate) struct LeaseReads {
    reads: AtomicUsize,
    holder: Mutex<Option<PaddockHolder>>,
}

impl LeaseReads {
    /// How many lease reads reached the gateway
    pub(crate) fn count(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }

    /// Answer every lease read that `client` holds its model for `what`
    pub(crate) fn held_by(&self, client: &str, what: &str) {
        *self.holder.lock().unwrap() = Some(PaddockHolder {
            lease: "L4".to_owned(),
            client: client.to_owned(),
            what: what.to_owned(),
            since: "2026-10-13T09:30:00Z".to_owned(),
        });
    }
}

impl LocalLeases for LeaseReads {
    fn paddock_holder(&self, _gateway: &Upstream, _model: &str) -> Option<PaddockHolder> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.holder.lock().unwrap().clone()
    }
}
