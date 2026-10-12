//! The installed file as the last upgrade left it
//!
//! `cargo install` replaces the installed kelpie with no copy kept, so a bare
//! `upgrade` has to tell a file it has restarted onto already from one it has
//! not. After every upgrade it writes the file's size and modification time to
//! `builds/installed.seen`, and a bare run restarts only when they differ.

use std::fs;
use std::os::unix::fs::MetadataExt;

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

/// Records the installed file as it is now
///
/// # Errors
///
/// A message when the file cannot be read or the record cannot be written.
pub fn record(layout: &Layout) -> Result<(), String> {
    let path = layout.seen();
    let write = || -> Result<(), String> {
        if let Some(folder) = path.parent() {
            fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        }
        fs::write(&path, stamp(layout)?).map_err(|e| e.to_string())
    };
    write().map_err(|e| format!("cannot record {}: {e}", path.display()))
}

// The size and modification time of the file the installed path leads to.
fn stamp(layout: &Layout) -> Result<String, String> {
    let installed = layout.installed();
    let meta =
        fs::metadata(installed).map_err(|e| format!("cannot read {}: {e}", installed.display()))?;
    Ok(format!(
        "{} {}.{:09}",
        meta.len(),
        meta.mtime(),
        meta.mtime_nsec()
    ))
}
