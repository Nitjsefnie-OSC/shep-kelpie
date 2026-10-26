//! The disk's free space and a folder's size, counting each hard link target once

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as imp;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;
#[cfg(not(any(unix, windows)))]
compile_error!("kelpie reads disks on Unix and Windows only");

struct Allocation {
    bytes: u64,
    linked: Option<(u64, u64)>,
}

/// The bytes on the disk available to the caller
///
/// # Errors
/// Returns an error if the disk holding `on` cannot be read.
pub fn free_bytes(on: &Path) -> io::Result<u64> {
    imp::free_bytes(on)
}

/// The allocated bytes in a folder's files, counting each hard link target once
///
/// File readings skip missing or inaccessible entries.
///
/// # Errors
/// Returns an error if a folder or a file's allocation cannot be read.
pub fn folder_bytes(folder: &Path) -> io::Result<u64> {
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
            let Some(allocation) = imp::allocation(&path, &meta)? else {
                continue;
            };
            // Cargo links a final artifact into `deps/` and the profile folder.
            if allocation.linked.is_some_and(|key| !seen.insert(key)) {
                continue;
            }
            total += allocation.bytes;
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests;
