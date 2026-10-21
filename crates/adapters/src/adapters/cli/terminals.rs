//! The terminal sessions a CLI has open

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::adapters::forwarder::Forwarder;
use crate::ports::{AgentCall, SessionId};

/// A terminal session until its end
#[derive(Debug)]
struct Terminal {
    // The settings the session ran with, which its end finds what it kept from
    settings: PathBuf,
    // The terminal's CLI dials it for as long as the session runs
    _forwarder: Option<Forwarder>,
}

/// The terminal sessions, by session id
#[derive(Debug, Clone, Default)]
pub(super) struct Terminals(Arc<Mutex<HashMap<String, Terminal>>>);

impl Terminals {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Terminal>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Keeps `call`'s session until [`Terminals::end`], with its forwarder open
    pub(super) fn keep(&self, call: &AgentCall, forwarder: Option<Forwarder>) {
        let terminal = Terminal {
            settings: call.settings.clone(),
            _forwarder: forwarder,
        };
        self.lock().insert(call.session.id().0.clone(), terminal);
    }

    /// Ends `session`, closing its forwarder, and says what settings it ran with,
    /// or `None` when it is not open
    pub(super) fn end(&self, session: &SessionId) -> Option<PathBuf> {
        self.lock().remove(&session.0).map(|t| t.settings)
    }
}
