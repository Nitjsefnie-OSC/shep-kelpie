//! The rig's disk: a free-space figure and a build folder's size a test sets

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::ports::Disk;

/// One GB, as `[builds]` counts it
pub(crate) const GB: u64 = 1_000_000_000;

/// Plenty of free space, so a rig's disk holds no turn unless a test says
const PLENTY: u64 = 500 * GB;

/// A disk that reports the free space a test sets, and a folder as big as
/// each file in it is, so removing files shrinks it
#[derive(Debug, Clone)]
pub(crate) struct FakeDisk {
    free: Arc<AtomicU64>,
    per_file: Arc<AtomicU64>,
    measured: Arc<Mutex<Vec<PathBuf>>>,
}

impl Default for FakeDisk {
    fn default() -> Self {
        Self {
            free: Arc::new(AtomicU64::new(PLENTY)),
            per_file: Arc::default(),
            measured: Arc::default(),
        }
    }
}

impl FakeDisk {
    /// Sets the bytes free on the disk
    pub(crate) fn set_free(&self, bytes: u64) {
        self.free.store(bytes, Ordering::SeqCst);
    }

    /// Makes every file in a measured folder count as `bytes`
    pub(crate) fn set_per_file(&self, bytes: u64) {
        self.per_file.store(bytes, Ordering::SeqCst);
    }

    /// The folders measured, in order
    pub(crate) fn measured(&self) -> Vec<PathBuf> {
        self.measured.lock().unwrap().clone()
    }
}

fn files_in(folder: &Path) -> io::Result<u64> {
    let mut files = 0;
    for entry in std::fs::read_dir(folder)? {
        let entry = entry?;
        files += if entry.file_type()?.is_dir() {
            files_in(&entry.path())?
        } else {
            1
        };
    }
    Ok(files)
}

impl Disk for FakeDisk {
    fn free_bytes(&self, _on: &Path) -> io::Result<u64> {
        Ok(self.free.load(Ordering::SeqCst))
    }

    fn folder_bytes(&self, folder: &Path) -> io::Result<u64> {
        self.measured.lock().unwrap().push(folder.to_owned());
        Ok(files_in(folder)? * self.per_file.load(Ordering::SeqCst))
    }
}
