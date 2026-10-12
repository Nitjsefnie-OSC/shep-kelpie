//! The rig's git: serves the repos a test registers and keeps every sync it was asked for

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::ports::{Git, GitError};

/// One sync the runner asked for
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Synced {
    pub(crate) url: String,
    pub(crate) reference: Option<String>,
    pub(crate) into: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FakeGit {
    served: Arc<Mutex<BTreeMap<String, PathBuf>>>,
    asked: Arc<Mutex<Vec<Synced>>>,
    down: Arc<AtomicBool>,
}

impl FakeGit {
    /// Serves the repo at `url` as a copy of the folder `from`
    pub(crate) fn serve(&self, url: &str, from: &Path) {
        self.served
            .lock()
            .unwrap()
            .insert(url.to_owned(), from.to_owned());
    }

    /// Fails every sync from now on, as an unreachable network does
    pub(crate) fn go_down(&self) {
        self.down.store(true, Ordering::SeqCst);
    }

    /// Every sync the runner asked for, oldest first
    pub(crate) fn asked(&self) -> Vec<Synced> {
        self.asked.lock().unwrap().clone()
    }
}

impl Git for FakeGit {
    fn sync(&self, url: &str, reference: Option<&str>, into: &Path) -> Result<(), GitError> {
        self.asked.lock().unwrap().push(Synced {
            url: url.to_owned(),
            reference: reference.map(str::to_owned),
            into: into.to_owned(),
        });
        if self.down.load(Ordering::SeqCst) {
            return Err(GitError("could not resolve host".into()));
        }
        let served = self.served.lock().unwrap().get(url).cloned();
        let from = served.ok_or_else(|| GitError(format!("repository {url} not found")))?;
        let _ = fs::remove_dir_all(into);
        crate::skills::copy_folder(&from, into).map_err(|e| GitError(e.to_string()))
    }
}
