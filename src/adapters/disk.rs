//! The disk's free space from `statvfs`, and a folder's size from a walk of it

use std::collections::HashSet;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::ports::Disk;

/// The bytes in one block of `st_blocks`, whatever the file system's own size
const BLOCK: u64 = 512;

/// Measures the machine's own disk
#[derive(Debug, Clone, Copy, Default)]
pub struct StatDisk;

impl Disk for StatDisk {
    fn free_bytes(&self, on: &Path) -> io::Result<u64> {
        let stat = rustix::fs::statvfs(on)?;
        Ok(stat.f_bavail.saturating_mul(stat.f_frsize))
    }

    fn folder_bytes(&self, folder: &Path) -> io::Result<u64> {
        let mut seen = HashSet::new();
        let mut total = 0;
        let mut pending = vec![folder.to_owned()];
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(&dir)? {
                let path = entry?.path();
                // A file removed while the walk runs is no longer there to count.
                let Ok(meta) = fs::symlink_metadata(&path) else {
                    continue;
                };
                if meta.is_dir() {
                    pending.push(path);
                    continue;
                }
                // Cargo links a final artifact into `deps/` and the profile folder.
                if meta.nlink() > 1 && !seen.insert((meta.dev(), meta.ino())) {
                    continue;
                }
                total += meta.blocks().saturating_mul(BLOCK);
            }
        }
        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_holds_its_files_and_counts_a_hard_link_once() {
        let dir = tempfile::tempdir().unwrap();
        let one = dir.path().join("deps/one");
        fs::create_dir_all(one.parent().unwrap()).unwrap();
        fs::write(&one, vec![1u8; 100_000]).unwrap();
        let alone = StatDisk.folder_bytes(dir.path()).unwrap();
        assert!(alone >= 100_000, "{alone}");

        fs::hard_link(&one, dir.path().join("one")).unwrap();
        assert_eq!(StatDisk.folder_bytes(dir.path()).unwrap(), alone);
    }

    #[test]
    fn a_folder_that_is_not_there_cannot_be_measured() {
        let dir = tempfile::tempdir().unwrap();
        assert!(StatDisk.folder_bytes(&dir.path().join("gone")).is_err());
    }

    #[test]
    fn the_disk_holding_a_folder_has_free_space() {
        let dir = tempfile::tempdir().unwrap();
        assert!(StatDisk.free_bytes(dir.path()).unwrap() > 0);
    }
}
