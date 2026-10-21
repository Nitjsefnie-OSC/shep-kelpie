//! What running calls keep in this image's memory, for a handover to carry

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::adapters::process::record::CallFiles;
use crate::ports::{AgentCall, Kept};

/// What running calls keep in this image's memory, by the call's record
#[derive(Debug, Clone, Default)]
pub(crate) struct Carried(Arc<Mutex<HashMap<PathBuf, Kept>>>);

impl Carried {
    fn lock(&self) -> MutexGuard<'_, HashMap<PathBuf, Kept>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// What the call with `record` keeps, if a running call noted it
    pub(crate) fn kept_by(&self, record: &Path) -> Option<Kept> {
        self.lock().get(record).cloned()
    }

    /// Notes what `call` keeps, until the returned guard is dropped
    pub(crate) fn note(&self, call: &AgentCall, kept: Kept) -> Noted {
        let record = CallFiles::of(call).record;
        self.lock().insert(record.clone(), kept);
        Noted {
            carried: self.clone(),
            record,
        }
    }
}

/// A call's note in [`Carried`], which its end removes
pub(crate) struct Noted {
    carried: Carried,
    record: PathBuf,
}

impl Drop for Noted {
    fn drop(&mut self) {
        self.carried.lock().remove(&self.record);
    }
}
