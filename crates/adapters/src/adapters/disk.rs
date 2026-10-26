//! The machine's own disk readings through the disk leaf crate

use std::io;
use std::path::Path;

use crate::ports::Disk;

/// Measures the machine's own disk
#[derive(Debug, Clone, Copy, Default)]
pub struct StatDisk;

impl Disk for StatDisk {
    fn free_bytes(&self, on: &Path) -> io::Result<u64> {
        shep_kelpie_disk::disk::free_bytes(on)
    }

    fn folder_bytes(&self, folder: &Path) -> io::Result<u64> {
        shep_kelpie_disk::disk::folder_bytes(folder)
    }
}
