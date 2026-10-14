//! The disk that holds kelpie's home, and what a build folder takes of it

use std::io;
use std::path::Path;

/// Measures the disk and a work item's build folder
pub trait Disk: Send + Sync {
    /// The bytes free on the disk that holds `on`, to a process that is not root
    ///
    /// # Errors
    ///
    /// The OS's error, when the disk cannot be asked.
    fn free_bytes(&self, on: &Path) -> io::Result<u64>;

    /// The bytes `folder` takes on disk
    ///
    /// # Errors
    ///
    /// The OS's error, when `folder` cannot be read.
    fn folder_bytes(&self, folder: &Path) -> io::Result<u64>;
}
