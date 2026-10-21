//! The source a project's board reads, held once for the runner and its ticket port

use std::sync::{Arc, RwLock};

use crate::settings::TicketSource;

/// The project's `tickets.source`, shared by the runner and the [`Routed`](super::Routed)
/// port it reads, which a settings change replaces
///
/// A clone is another handle to the same source, so a [`SharedSource::set`]
/// through one reaches the next call through every other.
#[derive(Debug, Clone, Default)]
pub struct SharedSource(Arc<RwLock<TicketSource>>);

impl SharedSource {
    /// A handle holding `source`
    pub fn new(source: TicketSource) -> Self {
        Self(Arc::new(RwLock::new(source)))
    }

    /// The source as it stands now
    pub fn get(&self) -> TicketSource {
        // A holder cannot leave the source half written, so a poisoned lock still holds a whole value.
        *self.0.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Has every handle to this source hold `source` from now on
    pub fn set(&self, source: TicketSource) {
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = source;
    }
}

impl From<TicketSource> for SharedSource {
    fn from(source: TicketSource) -> Self {
        Self::new(source)
    }
}
