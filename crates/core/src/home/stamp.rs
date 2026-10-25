//! The stamp of a file: its size and modification time
//!
//! An upgrade tells a file it has restarted onto already from one it has not
//! by its stamp, and a runner shows the stamp of the file it started from in
//! its `status`.

use std::fs;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::OnceLock;

/// The size and modification time of the file `path` leads to
///
/// # Errors
///
/// A message when the file cannot be read.
pub fn stamp_of(path: &Path) -> Result<String, String> {
    let cannot = |e: io::Error| format!("cannot read {}: {e}", path.display());
    let meta = fs::metadata(path).map_err(cannot)?;
    let (secs, nanos) = mtime(&meta).map_err(cannot)?;
    Ok(format!("{} {secs}.{nanos:09}", meta.len()))
}

// Whole seconds since the epoch and the nanoseconds after them.
#[cfg(unix)]
fn mtime(meta: &fs::Metadata) -> io::Result<(i64, i64)> {
    Ok((meta.mtime(), meta.mtime_nsec()))
}

#[cfg(windows)]
fn mtime(meta: &fs::Metadata) -> io::Result<(i64, i64)> {
    let since = meta
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?;
    let secs = i64::try_from(since.as_secs()).map_err(io::Error::other)?;
    Ok((secs, i64::from(since.subsec_nanos())))
}

/// The stamp of the file this process was started from, taken the first time
/// it is asked
///
/// A runner asks at its start, so a file replaced since is not mistaken for
/// the build it runs. `None` when the file cannot be read.
pub fn running() -> Option<&'static str> {
    static RUNNING: OnceLock<Option<String>> = OnceLock::new();
    RUNNING
        .get_or_init(|| {
            let stamp = std::env::current_exe()
                .map_err(|e| format!("cannot find its own file: {e}"))
                .and_then(|path| stamp_of(&path));
            // Said once, where `shep bleats` shows it: the upgrade can then
            // only tell that `status` names no build.
            stamp
                .inspect_err(|e| eprintln!("this runner's `status` shows no build: {e}"))
                .ok()
        })
        .as_deref()
}
