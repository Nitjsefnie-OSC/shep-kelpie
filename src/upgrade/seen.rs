//! The installed file as the last upgrade left it
//!
//! `cargo install` replaces the installed kelpie with no copy kept, so a bare
//! `upgrade` has to tell a file it has restarted onto from one it has not.
//! After every upgrade that finishes its restarts it writes the file's size
//! and modification time to `builds/installed.seen`, and a bare run restarts
//! only when they differ. The stamp is taken before the restarts, so a file
//! replaced while they wait reads as changed. A runner shows the stamp of the
//! file it started from in its `status`, which tells a handover reached it.

use std::fs;

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
    crate::home::stamp_of(layout.installed())
}
