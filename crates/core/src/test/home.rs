//! The folder a rig works in

use std::path::{Path, PathBuf};

use tempfile::TempDir;

/// A rig's home: one it made and removes, or one an earlier rig made
#[derive(Debug)]
pub enum Home {
    /// A folder made for the test, removed when it drops
    Made(TempDir),
    /// A folder that was already there and is left alone
    Kept(PathBuf),
}

impl Home {
    /// A new folder made for the test
    pub fn made() -> Self {
        Self::Made(tempfile::tempdir().unwrap())
    }

    /// The folder's path
    pub fn path(&self) -> &Path {
        match self {
            Self::Made(dir) => dir.path(),
            Self::Kept(path) => path,
        }
    }
}
