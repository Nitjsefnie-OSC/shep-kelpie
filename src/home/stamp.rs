//! The stamp of a file: its size and modification time
//!
//! An upgrade tells a file it has restarted onto already from one it has not
//! by its stamp, and a runner shows the stamp of the file it started from in
//! its `status`.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::OnceLock;

/// The size and modification time of the file `path` leads to
///
/// # Errors
///
/// A message when the file cannot be read.
pub fn stamp_of(path: &Path) -> Result<String, String> {
    let meta = fs::metadata(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    Ok(format!(
        "{} {}.{:09}",
        meta.len(),
        meta.mtime(),
        meta.mtime_nsec()
    ))
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
