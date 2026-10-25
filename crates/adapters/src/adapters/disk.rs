//! The disk's free space from `statvfs`, and a folder's size from a walk of it
//!
//! Windows has neither reading yet (#1300), so there it says `Unsupported`.

use std::io;
use std::path::Path;

use crate::ports::Disk;

/// Measures the machine's own disk
#[derive(Debug, Clone, Copy, Default)]
pub struct StatDisk;

#[cfg(unix)]
impl Disk for StatDisk {
    fn free_bytes(&self, on: &Path) -> io::Result<u64> {
        let stat = rustix::fs::statvfs(on)?;
        Ok(stat.f_bavail.saturating_mul(stat.f_frsize))
    }

    fn folder_bytes(&self, folder: &Path) -> io::Result<u64> {
        use std::collections::HashSet;
        use std::fs;
        use std::os::unix::fs::MetadataExt;

        // The bytes in one block of `st_blocks`, whatever the file system's own size
        const BLOCK: u64 = 512;

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

// A build limit is never applied on a reading kelpie cannot make (#1300).
#[cfg(windows)]
impl Disk for StatDisk {
    fn free_bytes(&self, _on: &Path) -> io::Result<u64> {
        Err(unsupported())
    }

    fn folder_bytes(&self, _folder: &Path) -> io::Result<u64> {
        Err(unsupported())
    }
}

#[cfg(windows)]
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "the disk is not read on Windows yet (#1300)",
    )
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::fs;

    use super::*;

    #[cfg(windows)]
    #[test]
    fn the_disk_is_not_read_until_windows_has_a_reading_for_it() {
        let dir = tempfile::tempdir().unwrap();
        let free = StatDisk.free_bytes(dir.path()).unwrap_err();
        assert_eq!(free.kind(), io::ErrorKind::Unsupported);
        let used = StatDisk.folder_bytes(dir.path()).unwrap_err();
        assert_eq!(used.kind(), io::ErrorKind::Unsupported);
        assert!(used.to_string().contains("#1300"), "{used}");
    }

    #[cfg(unix)]
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

    #[cfg(unix)]
    #[test]
    fn a_folder_that_is_not_there_cannot_be_measured() {
        let dir = tempfile::tempdir().unwrap();
        assert!(StatDisk.folder_bytes(&dir.path().join("gone")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn the_disk_holding_a_folder_has_free_space() {
        let dir = tempfile::tempdir().unwrap();
        assert!(StatDisk.free_bytes(dir.path()).unwrap() > 0);
    }
}
