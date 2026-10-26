//! The Unix disk and file allocation readings

use std::fs::Metadata;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use super::Allocation;

// The bytes in one block of `st_blocks`, whatever the file system's own size
const BLOCK: u64 = 512;

pub(super) fn free_bytes(on: &Path) -> io::Result<u64> {
    let stat = rustix::fs::statvfs(on)?;
    Ok(stat.f_bavail.saturating_mul(stat.f_frsize))
}

pub(super) fn allocation(_path: &Path, meta: &Metadata) -> io::Result<Option<Allocation>> {
    Ok(Some(Allocation {
        bytes: meta.blocks().saturating_mul(BLOCK),
        linked: (meta.nlink() > 1).then(|| (meta.dev(), meta.ino())),
    }))
}
