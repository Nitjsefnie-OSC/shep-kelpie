//! The project's hooks, held once for everything that runs them

use std::sync::{Arc, RwLock};

use super::Hooks;

/// The project's hooks, shared by the runner and the ports that run them,
/// which a settings change replaces
///
/// A clone is another handle to the same hooks. [`SharedHooks::get`] hands out
/// the hooks as they stand, so a call or a write that took them finishes under
/// them, and the next one takes the replacement.
#[derive(Debug, Clone, Default)]
pub struct SharedHooks(Arc<RwLock<Arc<Hooks>>>);

impl SharedHooks {
    /// A handle holding `hooks`
    pub fn new(hooks: Hooks) -> Self {
        Self(Arc::new(RwLock::new(Arc::new(hooks))))
    }

    /// The hooks as they stand now
    pub fn get(&self) -> Arc<Hooks> {
        // A holder cannot leave the hooks half written, so a poisoned lock still holds a whole value.
        Arc::clone(&self.0.read().unwrap_or_else(|e| e.into_inner()))
    }

    /// Has every handle to these hooks hold `hooks` from now on
    pub fn replace(&self, hooks: Hooks) {
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(hooks);
    }
}

impl From<Hooks> for SharedHooks {
    fn from(hooks: Hooks) -> Self {
        Self::new(hooks)
    }
}
