//! The installed file as the last upgrade left it
//!
//! `cargo install` replaces the installed kelpie with no copy kept, so a bare
//! `upgrade` has to tell a file it has restarted onto already from one it has
//! not. After every upgrade that finishes its restarts it writes the file's
//! size and modification time to `builds/installed.seen`, and a bare run
//! restarts only when they differ. The stamp written is the one taken before
//! the restarts, so a file replaced while they wait reads as changed.
//!
//! A runner shows the same stamp of the file it started from in its `status`,
//! which is how an upgrade tells that a handover reached the new build.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::OnceLock;

use super::install::Layout;

/// Whether the installed file is the one the last upgrade left
///
/// No record, or a file that cannot be read, counts as changed.
pub fn unchanged(layout: &Layout) -> bool {
    match (stamp(layout), fs::read_to_string(layout.seen())) {
        (Ok(now), Ok(seen)) => seen == now,
        _ => false,
    }
}

/// Records `stamp`, taken earlier, as the installed file the last upgrade left
///
/// # Errors
///
/// A message when the record cannot be written.
pub fn record(layout: &Layout, stamp: &str) -> Result<(), String> {
    let path = layout.seen();
    let write = || -> Result<(), String> {
        if let Some(folder) = path.parent() {
            fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        }
        fs::write(&path, stamp).map_err(|e| e.to_string())
    };
    write().map_err(|e| format!("cannot record {}: {e}", path.display()))
}

/// The size and modification time of the file the installed path leads to
///
/// # Errors
///
/// A message when the file cannot be read.
pub fn stamp(layout: &Layout) -> Result<String, String> {
    stamp_of(layout.installed())
}

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
