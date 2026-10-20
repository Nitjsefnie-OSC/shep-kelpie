//! The runner's own git, for the marketplaces a repo's plugins come from

use std::fmt;
use std::path::Path;

/// Keeps a copy of a git repo that a plugin marketplace lives in
pub trait Git: Send + Sync {
    /// Makes `into` a copy of the repo at `url`, at `reference` where one is
    /// given and at the repo's own head otherwise: cloned if `into` holds
    /// none yet, fetched if it does
    ///
    /// A copy that cannot be brought up to date is left as it was.
    ///
    /// # Errors
    ///
    /// [`GitError`] when git cannot be started or the repo cannot be reached.
    fn sync(&self, url: &str, reference: Option<&str>, into: &Path) -> Result<(), GitError>;
}

/// Why a repo could not be cloned or fetched
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitError(pub String);

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl core::error::Error for GitError {}
